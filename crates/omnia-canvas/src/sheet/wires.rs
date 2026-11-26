//! D4: normalize wire orientation and choose the stroke in whole device pixels.

use crate::{
    camera::Camera,
    snap_px::{device_rect, round_device, stroke_device_px_ties_even, to_device},
};
use gpui::{Bounds, Pixels};
use omnia_doc::{Pt, Wire};

/// K5 wire stroke width in world nanometres (2.0 px at 100% zoom, matching `symbols.rs`).
pub(crate) const WIDTH_NM: f64 = 254_000.0;

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

    /// On-screen length of this run in logical pixels (D4, INV8).
    pub(super) fn length_px(self, camera: &Camera) -> f64 {
        (i64::from(self.end) - i64::from(self.start)) as f64 * camera.scale()
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

pub(crate) fn width(camera: &Camera, sf: f32) -> u32 {
    stroke_device_px_ties_even(WIDTH_NM * camera.scale(), sf)
}

// Keep world coordinates so every frame still uses the current camera and stroke.
// Compare geometry in stable wire-id order, including edits that reuse a wire id.
#[derive(Debug, Default)]
pub(super) struct MergedSegments {
    source: Vec<Segment>,
    runs: Vec<Segment>,
}

impl MergedSegments {
    pub(super) fn visible(
        &mut self,
        segments: impl Iterator<Item = Segment> + Clone,
    ) -> &[Segment] {
        if !segments.clone().eq(self.source.iter().copied()) {
            self.source.clear();
            self.source.extend(segments);
            self.runs.clone_from(&self.source);
            self.runs.sort_unstable();
            self.runs.dedup_by(|next, current| current.merge(*next));
        }
        &self.runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k5_wire_stroke_matches_symbol_stroke() {
        assert_eq!(WIDTH_NM, super::super::symbols::SYMBOL_STROKE_NM);
        let cam_100 = Camera::new(Pt::ZERO, 1.0, Bounds::default());
        assert_eq!(width(&cam_100, 1.0), 2);
        assert_eq!(width(&cam_100, 1.25), 2);
        assert_eq!(width(&cam_100, 1.5), 3);
        assert_eq!(width(&cam_100, 1.75), 4);
        assert_eq!(width(&cam_100, 2.0), 4);
        let cam_3200 = Camera::new(Pt::ZERO, 32.0, Bounds::default());
        assert_eq!(width(&cam_3200, 1.0), 64);
        assert_eq!(width(&cam_3200, 1.25), 80);
        assert_eq!(width(&cam_3200, 1.5), 96);
    }

    #[test]
    fn d4_visible_runs_follow_geometry_and_view_changes() {
        let segment = |start, end| Segment {
            vertical: false,
            line: 0,
            start,
            end,
        };
        let mut cache = MergedSegments::default();
        let input = [segment(2, 3), segment(0, 1), segment(1, 2)];
        for _ in 0..2 {
            assert_eq!(cache.visible(input.into_iter()), &[segment(0, 3)]);
        }
        let edited = [segment(2, 3), segment(0, 1), segment(5, 6)];
        assert_eq!(
            cache.visible(edited.into_iter()),
            &[edited[1], edited[0], edited[2]]
        );
        assert_eq!(cache.visible(input[..1].iter().copied()), &input[..1]);
        assert!(cache.visible(std::iter::empty()).is_empty());
    }
}
