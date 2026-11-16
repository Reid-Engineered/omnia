//! D1/INV8 camera mapping in logical pixels, independent of display scale factor.
//!
//! Keep screen coordinates in f64 through inverse mapping. GPUI's f32 `Pixels`
//! cannot represent every nanometre even at 100% zoom; narrow only for painting.

use gpui::{Bounds, Pixels, Point, point, px};
use omnia_doc::{GRID_NM, Pt, Rect, SHEET_LIMIT_NM};

/// K3: logical pixels per nanometre at 100% zoom.
pub const BASE_SCALE: f64 = 20.0 / GRID_NM as f64;
/// K4: minimum zoom multiplier (5%).
pub const MIN_ZOOM: f64 = 0.05;
/// K4: maximum zoom multiplier (3200%).
pub const MAX_ZOOM: f64 = 32.0;

/// An integer world reference plus a fractional screen translation.
///
/// `center` is the initial world reference; `translation` preserves fractional
/// nanometres during gestures instead of rounding the centre after each event.
/// Positive pan deltas move the sheet right/down. Inverse points are rounded to
/// the nearest nm and clamped to K1; visible bounds round outward for culling.
#[derive(Clone, Debug)]
pub struct Camera {
    center: Pt,
    zoom: f64,
    viewport: Bounds<Pixels>,
    translation: Point<f64>,
}

impl Camera {
    /// Construct a camera centred in `viewport`. `zoom` is a multiplier (1 = 100%).
    ///
    /// Panics for an out-of-sheet centre, non-finite zoom, or invalid viewport.
    pub fn new(center: Pt, zoom: f64, viewport: Bounds<Pixels>) -> Self {
        assert!(center.is_in_bounds());
        assert!(zoom.is_finite());
        validate_viewport(viewport);
        Self {
            center,
            zoom: zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            viewport,
            translation: point(0.0, 0.0),
        }
    }

    /// Current zoom multiplier.
    pub fn zoom(&self) -> f64 {
        self.zoom
    }

    /// Current logical pixels per nm.
    pub fn scale(&self) -> f64 {
        BASE_SCALE * self.zoom
    }

    /// Resize/reposition the viewport, keeping the world point at its centre.
    pub fn set_viewport(&mut self, viewport: Bounds<Pixels>) {
        validate_viewport(viewport);
        self.viewport = viewport;
    }

    fn screen_origin(&self) -> Point<f64> {
        point(
            f64::from(self.viewport.origin.x) + f64::from(self.viewport.size.width) / 2.0,
            f64::from(self.viewport.origin.y) + f64::from(self.viewport.size.height) / 2.0,
        )
    }

    /// Map world nm to logical pixels without the lossy f32 paint conversion.
    pub fn world_to_screen(&self, p: Pt) -> Point<f64> {
        let origin = self.screen_origin();
        // Subtract in i64 before converting: no f32 ever holds world coordinates.
        point(
            (i64::from(p.x) - i64::from(self.center.x)) as f64 * self.scale()
                + origin.x
                + self.translation.x,
            (i64::from(p.y) - i64::from(self.center.y)) as f64 * self.scale()
                + origin.y
                + self.translation.y,
        )
    }

    /// Final paint conversion; do not feed this lossy result into inverse mapping.
    pub fn world_to_pixels(&self, p: Pt) -> Point<Pixels> {
        let screen = self.world_to_screen(p);
        point(px(screen.x as f32), px(screen.y as f32))
    }

    fn world_at(&self, p: Point<f64>) -> Point<f64> {
        let origin = self.screen_origin();
        point(
            (p.x - origin.x - self.translation.x) / self.scale() + f64::from(self.center.x),
            (p.y - origin.y - self.translation.y) / self.scale() + f64::from(self.center.y),
        )
    }

    /// Inverse map, rounding to the nearest nm (ties away from zero), bounded by K1.
    /// Convert GPUI input with `point(f64::from(p.x), f64::from(p.y))`.
    pub fn screen_to_world(&self, p: Point<f64>) -> Pt {
        assert!(p.x.is_finite() && p.y.is_finite());
        let p = self.world_at(p);
        Pt {
            x: sheet_coord(p.x.round()),
            y: sheet_coord(p.y.round()),
        }
    }

    /// Conservative visible world rectangle, clipped to the sheet limits.
    /// A viewport entirely outside the sheet gives a degenerate boundary rect.
    pub fn visible(&self) -> Rect {
        let min = self.world_at(point(
            f64::from(self.viewport.origin.x),
            f64::from(self.viewport.origin.y),
        ));
        let max = self.world_at(point(
            f64::from(self.viewport.origin.x) + f64::from(self.viewport.size.width),
            f64::from(self.viewport.origin.y) + f64::from(self.viewport.size.height),
        ));
        Rect {
            min: Pt {
                x: sheet_coord(min.x.floor()),
                y: sheet_coord(min.y.floor()),
            },
            max: Pt {
                x: sheet_coord(max.x.ceil()),
                y: sheet_coord(max.y.ceil()),
            },
        }
    }

    /// Translate by logical pixels without quantising to integer world nm.
    /// Panics for non-finite deltas or translation overflow.
    pub fn pan_by(&mut self, delta: Point<f64>) {
        let translation = point(self.translation.x + delta.x, self.translation.y + delta.y);
        assert!(translation.x.is_finite() && translation.y.is_finite());
        self.translation = translation;
    }

    /// Zoom about a logical screen anchor, clamping the result to K4.
    /// Non-positive or non-finite factors/anchors are ignored (invalid input event).
    pub fn zoom_about(&mut self, anchor: Point<f64>, factor: f64) {
        if !factor.is_finite() || factor <= 0.0 || !anchor.x.is_finite() || !anchor.y.is_finite() {
            return;
        }
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let ratio = zoom / self.zoom;
        let origin = self.screen_origin();
        let translation = point(
            (anchor.x - origin.x) * (1.0 - ratio) + self.translation.x * ratio,
            (anchor.y - origin.y) * (1.0 - ratio) + self.translation.y * ratio,
        );
        if translation.x.is_finite() && translation.y.is_finite() {
            self.translation = translation;
            self.zoom = zoom;
        }
    }
}

fn sheet_coord(nm: f64) -> i32 {
    nm.clamp(-f64::from(SHEET_LIMIT_NM), f64::from(SHEET_LIMIT_NM)) as i32
}

fn validate_viewport(viewport: Bounds<Pixels>) {
    assert!(f64::from(viewport.origin.x).is_finite());
    assert!(f64::from(viewport.origin.y).is_finite());
    for dimension in [viewport.size.width, viewport.size.height] {
        assert!(f64::from(dimension).is_finite() && dimension >= px(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    use proptest::prelude::*;

    fn viewport() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(37.0), px(59.0)),
            size: size(px(3840.0), px(2160.0)),
        }
    }

    fn world_point() -> impl Strategy<Value = Pt> {
        (
            -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
        )
            .prop_map(|(x, y)| Pt { x, y })
    }

    #[test]
    fn limits_and_adjacent_nanometres_round_trip_at_all_zoom_extremes() {
        for zoom in [MIN_ZOOM, 1.0, MAX_ZOOM] {
            for center in [
                Pt::ZERO,
                Pt {
                    x: SHEET_LIMIT_NM,
                    y: -SHEET_LIMIT_NM,
                },
            ] {
                let camera = Camera::new(center, zoom, viewport());
                for x in [
                    -SHEET_LIMIT_NM,
                    -SHEET_LIMIT_NM + 1,
                    -1,
                    0,
                    1,
                    SHEET_LIMIT_NM - 1,
                    SHEET_LIMIT_NM,
                ] {
                    for y in [-SHEET_LIMIT_NM, -1, 0, 1, SHEET_LIMIT_NM] {
                        let p = Pt { x, y };
                        assert_eq!(camera.screen_to_world(camera.world_to_screen(p)), p);
                    }
                }
            }
        }
    }

    #[test]
    fn base_scale_and_exact_pixel_pan() {
        let mut camera = Camera::new(Pt::ZERO, 1.0, viewport());
        let p = Pt {
            x: GRID_NM,
            y: -GRID_NM,
        };
        let before = camera.world_to_screen(p);
        assert_eq!(before, point(1977.0, 1119.0));
        camera.pan_by(point(0.125, -3.5));
        assert_eq!(
            camera.world_to_screen(p),
            point(before.x + 0.125, before.y - 3.5)
        );
        let painted = camera.world_to_pixels(p);
        assert_eq!(f64::from(painted.x), before.x + 0.125);
    }

    #[test]
    fn zoom_clamps_and_ignores_invalid_events() {
        let mut camera = Camera::new(Pt::ZERO, 1.0, viewport());
        let anchor = point(37.25, 2139.5);
        for factor in [f64::MAX, f64::MIN_POSITIVE, 1.0, 640.0] {
            let before = camera.world_at(anchor);
            camera.zoom_about(anchor, factor);
            let after = camera.world_at(anchor);
            assert!((before.x - after.x).abs() <= 1.0);
            assert!((before.y - after.y).abs() <= 1.0);
            assert!((MIN_ZOOM..=MAX_ZOOM).contains(&camera.zoom()));
        }
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let before = camera.world_to_screen(Pt::ZERO);
            camera.zoom_about(anchor, factor);
            assert_eq!(camera.world_to_screen(Pt::ZERO), before);
        }
        assert_eq!(Camera::new(Pt::ZERO, -1.0, viewport()).zoom(), MIN_ZOOM);
        assert_eq!(Camera::new(Pt::ZERO, 100.0, viewport()).zoom(), MAX_ZOOM);
    }

    #[test]
    fn fractional_pan_and_repeated_zoom_do_not_drift_at_sheet_corner() {
        let mut camera = Camera::new(
            Pt {
                x: SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
            MIN_ZOOM,
            viewport(),
        );
        camera.pan_by(point(0.125, -0.0625));
        let anchor = point(1957.25, 1139.5);
        let before = camera.world_at(anchor);
        for _ in 0..1000 {
            camera.zoom_about(anchor, MAX_ZOOM / MIN_ZOOM);
            camera.zoom_about(anchor, MIN_ZOOM / MAX_ZOOM);
        }
        let after = camera.world_at(anchor);
        assert!((before.x - after.x).abs() <= 1.0);
        assert!((before.y - after.y).abs() <= 1.0);
        let p = Pt {
            x: SHEET_LIMIT_NM - 1,
            y: -SHEET_LIMIT_NM + 1,
        };
        assert_eq!(camera.screen_to_world(camera.world_to_screen(p)), p);
    }

    #[test]
    fn visible_rounds_outward_clips_and_tracks_resize() {
        let mut camera = Camera::new(Pt::ZERO, MAX_ZOOM, viewport());
        camera.pan_by(point(0.1, -0.3));
        let area = camera.visible();
        let min = camera.world_to_screen(area.min);
        let max = camera.world_to_screen(area.max);
        assert!(min.x <= 37.0 && min.y <= 59.0);
        assert!(max.x >= 3877.0 && max.y >= 2219.0);
        let before = camera.world_at(camera.screen_origin());
        camera.set_viewport(Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(10.0), px(10.0)),
        });
        assert_eq!(camera.world_at(camera.screen_origin()), before);
        camera.pan_by(point(1e6, -1e6));
        let area = camera.visible();
        assert_eq!(area.min.x, -SHEET_LIMIT_NM);
        assert_eq!(area.max.y, SHEET_LIMIT_NM);
    }

    proptest! {
        #[test]
        fn round_trip_across_sheet_and_zoom_range(p in world_point(), center in world_point(), zoom in MIN_ZOOM..=MAX_ZOOM,
            dx in -3840.0f64..3840.0, dy in -2160.0f64..2160.0) {
            let mut camera = Camera::new(center, zoom, viewport());
            camera.pan_by(point(dx, dy));
            prop_assert_eq!(camera.screen_to_world(camera.world_to_screen(p)), p);
        }

        #[test]
        fn anchored_zoom_within_one_nm(center in world_point(), zoom in MIN_ZOOM..=MAX_ZOOM,
            x in 37.0f64..3877.0, y in 59.0f64..2219.0, factor in 0.001f64..1000.0,
            dx in -3840.0f64..3840.0, dy in -2160.0f64..2160.0) {
            let mut camera = Camera::new(center, zoom, viewport());
            camera.pan_by(point(dx, dy));
            let anchor = point(x, y);
            let before = camera.world_at(anchor);
            let rounded = camera.screen_to_world(anchor);
            camera.zoom_about(anchor, factor);
            let after = camera.world_at(anchor);
            prop_assert!((before.x - after.x).abs() <= 1.0);
            prop_assert!((before.y - after.y).abs() <= 1.0);
            let after_rounded = camera.screen_to_world(anchor);
            prop_assert!((i64::from(rounded.x) - i64::from(after_rounded.x)).abs() <= 1);
            prop_assert!((i64::from(rounded.y) - i64::from(after_rounded.y)).abs() <= 1);
        }

        #[test]
        fn pan_moves_every_on_screen_point(center in world_point(), zoom in MIN_ZOOM..=MAX_ZOOM,
            x in 37.0f64..3877.0, y in 59.0f64..2219.0,
            dx in -3840i32..3840, dy in -2160i32..2160) {
            let mut camera = Camera::new(center, zoom, viewport());
            let p = camera.screen_to_world(point(x, y));
            let before = camera.world_to_screen(p);
            camera.pan_by(point(f64::from(dx), f64::from(dy)));
            // Same arithmetic order for expected and actual, without pixel snapping.
            prop_assert_eq!(camera.world_to_screen(p), point(before.x + f64::from(dx), before.y + f64::from(dy)));
        }
    }
}
