//! Scripted camera and pointer drivers. Every frame differs from the one before, so every
//! frame repaints and no frame is skipped as identical.
//!
//! `drag-part` drives `OverlayView` in `drag_part.rs` (T5.4); `drag-wire` drives `OverlayView`'s
//! wire tool in `drag_wire.rs` (T6.4); `hover` drives `OverlayView` over real sheets in `hover.rs` (T4.6).

/// K4: the zoom range the `zoom` script sweeps (5% to 3200%).
pub use omnia_canvas::camera::{MAX_ZOOM, MIN_ZOOM};
/// Frames in one `zoom` cycle: 100% up to 3200%, down to 5%, back to 100%.
pub const ZOOM_PERIOD: u32 = 600;

/// Camera for one frame: an offset from the sheet origin in device pixels, and a zoom
/// multiplier about the viewport centre (1 = 100%). Scenes without a zoom ignore it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub x: f32,
    pub y: f32,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// The offset in logical pixels for a window at `scale_factor`.
    pub fn logical(self, scale_factor: f32) -> (f32, f32) {
        (self.x / scale_factor, self.y / scale_factor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    /// Pans right by one device pixel per frame at 100% zoom.
    Pan,
    /// Zooms about the viewport centre, geometrically, over the whole K4 range: from 100%
    /// up to 3200%, down to 5% and back to 100% every [`ZOOM_PERIOD`] frames.
    Zoom,
    /// Sweeps the pointer across visible parts, wires, labels (when present), and empty space
    /// over a focused `OverlayView` (T4.6, INV11).
    Hover,
    /// Drags a visible part's ghost across grid-step waypoints over a focused `OverlayView`
    /// (T5.4, B6, INV5, INV11).
    DragPart,
    /// Drags a visible wire segment perpendicular to its axis across grid-step waypoints over a
    /// focused `OverlayView` (W3.3, WB6, WD7, INV5, INV11).
    DragSegment,
    /// Sweeps an active `Tool::Wire` L-route preview and WD5 snap marker across in-viewport snap
    /// targets over a focused `OverlayView` (T6.4, B6, INV5, INV11).
    DragWire,
}

impl Script {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "pan" => Ok(Self::Pan),
            "zoom" => Ok(Self::Zoom),
            "hover" => Ok(Self::Hover),
            "drag-part" => Ok(Self::DragPart),
            "drag-segment" => Ok(Self::DragSegment),
            "drag-wire" => Ok(Self::DragWire),
            _ => Err(format!(
                "unknown script `{name}` (available: pan, zoom, hover, drag-part, drag-segment, drag-wire)"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Pan => "pan",
            Self::Zoom => "zoom",
            Self::Hover => "hover",
            Self::DragPart => "drag-part",
            Self::DragSegment => "drag-segment",
            Self::DragWire => "drag-wire",
        }
    }

    /// The camera for frame `frame`, counting from the first rendered frame.
    pub fn camera(self, frame: u32) -> Camera {
        match self {
            Self::Pan => Camera {
                x: frame as f32,
                ..Camera::default()
            },
            Self::Zoom => Camera {
                zoom: zoom_at(frame),
                ..Camera::default()
            },
            Self::Hover | Self::DragPart | Self::DragSegment | Self::DragWire => Camera::default(),
        }
    }
}

/// A triangle wave in log2(zoom), starting at 0 (100%), with constant speed.
fn zoom_at(frame: u32) -> f64 {
    let (hi, lo) = (MAX_ZOOM.log2(), MIN_ZOOM.log2());
    let path = 2.0 * (hi - lo);
    let t = f64::from(frame % ZOOM_PERIOD) / f64::from(ZOOM_PERIOD) * path;
    let log = if t < hi {
        t
    } else if t < hi + (hi - lo) {
        hi - (t - hi)
    } else {
        lo + (t - hi - (hi - lo))
    };
    log.exp2().clamp(MIN_ZOOM, MAX_ZOOM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_moves_one_device_pixel_per_frame() {
        for frame in [0_u32, 1, 2, 599, 10_000] {
            let step = Script::Pan.camera(frame + 1).x - Script::Pan.camera(frame).x;
            assert_eq!(step, 1.0);
            assert_eq!(Script::Pan.camera(frame).y, 0.0);
            assert_eq!(Script::Pan.camera(frame).zoom, 1.0);
        }
        let (x, _) = Script::Pan.camera(3).logical(1.5);
        assert_eq!(x, 2.0);
    }

    #[test]
    fn zoom_sweeps_k4_and_changes_every_frame() {
        assert!((zoom_at(0) - 1.0).abs() < 1e-12);
        assert!((zoom_at(ZOOM_PERIOD) - 1.0).abs() < 1e-12);
        let zooms: Vec<f64> = (0..ZOOM_PERIOD).map(zoom_at).collect();
        let max = zooms.iter().copied().fold(f64::MIN, f64::max);
        let min = zooms.iter().copied().fold(f64::MAX, f64::min);
        assert!((MAX_ZOOM * 0.97..=MAX_ZOOM).contains(&max), "max {max}");
        assert!((MIN_ZOOM..MIN_ZOOM * 1.03).contains(&min), "min {min}");
        for pair in zooms.windows(2) {
            assert_ne!(pair[0], pair[1]);
            // Constant speed in log2: about 18.6 octaves per period.
            assert!((pair[1] / pair[0]).log2().abs() < 0.04);
        }
        assert_eq!(Script::Zoom.camera(7).x, 0.0);
    }

    #[test]
    fn parse_scripts() {
        assert_eq!(Script::parse("pan"), Ok(Script::Pan));
        assert_eq!(Script::parse("zoom"), Ok(Script::Zoom));
        assert_eq!(Script::parse("hover"), Ok(Script::Hover));
        assert_eq!(Script::parse("drag-part"), Ok(Script::DragPart));
        assert_eq!(Script::parse("drag-segment"), Ok(Script::DragSegment));
        assert_eq!(Script::parse("drag-wire"), Ok(Script::DragWire));
        assert!(Script::parse("spin").is_err());
    }
}
