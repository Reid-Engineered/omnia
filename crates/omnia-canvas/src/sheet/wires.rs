//! D4: normalize wire orientation and choose the stroke in whole device pixels.

use crate::{
    camera::Camera,
    snap_px::{device_rect, round_device, stroke_device_px, to_device},
};
use gpui::{Bounds, Pixels};
use omnia_doc::{Pt, Wire};

pub(super) const WIDTH_NM: f64 = 228_600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Segment {
    vertical: bool,
    line: i32,
    start: i32,
    end: i32,
}

impl From<Wire> for Segment {
    fn from(wire: Wire) -> Self {
        let vertical = wire.a.x == wire.b.x;
        let (line, a, b) = if vertical {
            (wire.a.x, wire.a.y, wire.b.y)
        } else {
            (wire.a.y, wire.a.x, wire.b.x)
        };
        Self {
            vertical,
            line,
            start: a.min(b),
            end: a.max(b),
        }
    }
}

impl Segment {
    pub(super) fn merge(&mut self, next: Self) -> bool {
        if self.vertical != next.vertical || self.line != next.line || next.start > self.end {
            return false;
        }
        self.end = self.end.max(next.end);
        true
    }

    pub(super) fn bounds(self, camera: &Camera, sf: f32, width: u32) -> Bounds<Pixels> {
        let point = |along| {
            if self.vertical {
                Pt {
                    x: self.line,
                    y: along,
                }
            } else {
                Pt {
                    x: along,
                    y: self.line,
                }
            }
        };
        let start = camera.world_to_screen(point(self.start));
        let end = camera.world_to_screen(point(self.end));
        let stroke_start = |axis| round_device(to_device(axis, sf) - f64::from(width) / 2.0);
        let snap = |axis| round_device(to_device(axis, sf));
        // Square caps cover bends and keep a segment visible even if both ends snap
        // to the same pixel. Overlap cannot change the opaque ink's coverage.
        let cap = f64::from(width / 2);
        if self.vertical {
            let length = (snap(end.y) - snap(start.y)) as u32;
            device_rect(
                stroke_start(start.x),
                snap(start.y) - cap,
                width,
                length.saturating_add(width),
                sf,
            )
        } else {
            let length = (snap(end.x) - snap(start.x)) as u32;
            device_rect(
                snap(start.x) - cap,
                stroke_start(start.y),
                length.saturating_add(width),
                width,
                sf,
            )
        }
    }
}

pub(super) fn width(camera: &Camera, sf: f32) -> u32 {
    stroke_device_px(WIDTH_NM * camera.scale(), sf)
}
