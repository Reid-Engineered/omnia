//! Device-pixel rounding (D4, D5, G3). Every sheet primitive goes through these helpers.
//!
//! GPUI's `paint_quad` rounds each edge of a quad to the nearest device pixel on its own
//! (`round_half_toward_zero(logical * scale_factor)`, `window.rs` `snap_bounds`), so a quad's
//! device width can vary by a pixel as it moves. Instead we choose whole device pixels here,
//! then hand GPUI the logical values `device / scale_factor`; its snap maps those straight
//! back to the same integers (the f32 error is far below half a pixel for any on-screen
//! magnitude), so the width never varies.
//!
//! Positions round half up (`floor(x + 0.5)`), so shifting the input by a whole number of
//! device pixels shifts the output by exactly that many: panning never makes a dot jitter.

use gpui::{Bounds, Pixels, Point, point, px, size};

/// `logical * scale_factor`, in f64.
#[inline]
pub fn to_device(logical: f64, scale_factor: f32) -> f64 {
    logical * f64::from(scale_factor)
}

/// Rounds a device-pixel coordinate to a whole device pixel, ties toward +infinity.
#[inline]
pub fn round_device(device: f64) -> f64 {
    (device + 0.5).floor()
}

/// A logical coordinate snapped to the nearest device pixel, still in logical pixels.
#[inline]
pub fn snap_logical(logical: f64, scale_factor: f32) -> f64 {
    round_device(to_device(logical, scale_factor)) / f64::from(scale_factor)
}

/// A stroke or dot size in whole device pixels: `max(1, round(logical × scale_factor))`
/// (D4, K5, K6). Never zero, so thin strokes do not vanish.
#[inline]
pub fn stroke_device_px(logical: f64, scale_factor: f32) -> u32 {
    let device = round_device(to_device(logical.max(0.0), scale_factor));
    if device >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        (device as u32).max(1)
    }
}

/// Like [`stroke_device_px`], breaking exact half-pixel ties to the nearest even integer.
///
/// Used by K5 wires so exact ties at fractional scales (e.g. `2.0 × 1.25 = 2.5 → 2`,
/// `2.0 × 1.75 = 3.5 → 4`) stay even and centre symmetrically on integer device-pixel pin
/// and junction coordinates.
#[inline]
pub fn stroke_device_px_ties_even(logical: f64, scale_factor: f32) -> u32 {
    let device = to_device(logical.max(0.0), scale_factor).round_ties_even();
    if device >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        (device as u32).max(1)
    }
}

/// Adjusts `size_dev` by at most `+1` device pixel so its parity matches `reference_dev`,
/// keeping concentric strokes and markers centred on the exact same device pixel at fractional
/// scale factors.
#[inline]
pub fn match_parity(size_dev: u32, reference_dev: u32) -> u32 {
    if size_dev % 2 == reference_dev % 2 {
        size_dev
    } else {
        size_dev.saturating_add(1)
    }
}

/// The logical value GPUI's own snap maps back to `device` exactly.
#[inline]
pub fn device_to_logical(device: f64, scale_factor: f32) -> Pixels {
    px((device / f64::from(scale_factor)) as f32)
}

/// Logical bounds covering exactly the device rectangle with top-left corner
/// (`left`, `top`) and the given size, all in whole device pixels.
#[inline]
pub fn device_rect(
    left: f64,
    top: f64,
    width: u32,
    height: u32,
    scale_factor: f32,
) -> Bounds<Pixels> {
    Bounds {
        origin: point(
            device_to_logical(left, scale_factor),
            device_to_logical(top, scale_factor),
        ),
        size: size(
            device_to_logical(f64::from(width), scale_factor),
            device_to_logical(f64::from(height), scale_factor),
        ),
    }
}

/// The square of `side` whole device pixels whose centre is nearest to `center` (logical
/// pixels), with its corner on a device pixel.
#[inline]
pub fn centered_square(center: Point<f64>, side: u32, scale_factor: f32) -> Bounds<Pixels> {
    let half = f64::from(side) / 2.0;
    let left = round_device(to_device(center.x, scale_factor) - half);
    let top = round_device(to_device(center.y, scale_factor) - half);
    device_rect(left, top, side, side, scale_factor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const SCALES: [f32; 7] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 3.0];

    /// GPUI's snap at the pin (`util.rs` `round_to_device_pixel`), restated for the test.
    fn gpui_snap(logical: Pixels, scale_factor: f32) -> f32 {
        let v = f32::from(logical) * scale_factor;
        (v.abs() - 0.5).ceil().copysign(v)
    }

    /// The device rectangle GPUI paints for `bounds` (`window.rs` `snap_bounds`).
    fn gpui_device(bounds: Bounds<Pixels>, scale_factor: f32) -> [f32; 4] {
        let left = gpui_snap(bounds.left(), scale_factor);
        let top = gpui_snap(bounds.top(), scale_factor);
        let right = gpui_snap(bounds.right(), scale_factor).max(left);
        let bottom = gpui_snap(bounds.bottom(), scale_factor).max(top);
        [left, top, right - left, bottom - top]
    }

    #[test]
    fn stroke_never_below_one_device_pixel() {
        assert_eq!(stroke_device_px(0.0, 1.0), 1);
        assert_eq!(stroke_device_px(0.1, 1.0), 1);
        assert_eq!(stroke_device_px(-3.0, 2.0), 1);
        // K6 grid dot, 1.2 px: 1, 1.5 and 1.8 device pixels round to 1, 2 and 2.
        assert_eq!(stroke_device_px(1.2, 1.0), 1);
        assert_eq!(stroke_device_px(1.2, 1.25), 2);
        assert_eq!(stroke_device_px(1.2, 1.5), 2);
        // K5 wire and symbol stroke, 2.0 px (ties-to-even keeps 125% and 175% even-width).
        assert_eq!(stroke_device_px(2.0, 1.0), 2);
        assert_eq!(stroke_device_px(2.0, 1.25), 3);
        assert_eq!(stroke_device_px(2.0, 1.5), 3);
        assert_eq!(stroke_device_px_ties_even(2.0, 1.0), 2);
        assert_eq!(stroke_device_px_ties_even(2.0, 1.25), 2);
        assert_eq!(stroke_device_px_ties_even(2.0, 1.5), 3);
        assert_eq!(stroke_device_px_ties_even(2.0, 1.75), 4);
        assert_eq!(stroke_device_px_ties_even(2.0, 2.0), 4);
        assert_eq!(stroke_device_px_ties_even(0.0, 1.0), 1);
        assert_eq!(match_parity(4, 2), 4);
        assert_eq!(match_parity(3, 2), 4);
        assert_eq!(match_parity(4, 3), 5);
        assert_eq!(match_parity(5, 3), 5);
    }

    #[test]
    fn rounding_is_translation_invariant() {
        for x in [-2.5, -0.5, 0.5, 1.5, 7.49, 7.5] {
            for shift in [-3.0, 1.0, 1000.0] {
                assert_eq!(round_device(x + shift), round_device(x) + shift);
            }
        }
        assert_eq!(snap_logical(10.3, 1.25), 10.4);
    }

    proptest! {
        #[test]
        fn gpui_keeps_our_device_rects(left in -4000i32..8000, top in -4000i32..8000,
            w in 1u32..64, h in 1u32..64, scale in 0usize..SCALES.len()) {
            let scale = SCALES[scale];
            let bounds = device_rect(f64::from(left), f64::from(top), w, h, scale);
            prop_assert_eq!(gpui_device(bounds, scale),
                [left as f32, top as f32, w as f32, h as f32]);
        }

        #[test]
        fn centered_square_keeps_its_size_and_nearest_centre(x in -4000.0f64..8000.0,
            y in -4000.0f64..8000.0, side in 1u32..8, scale in 0usize..SCALES.len()) {
            let scale = SCALES[scale];
            let bounds = centered_square(point(x, y), side, scale);
            let device = gpui_device(bounds, scale);
            prop_assert_eq!(device[2], side as f32);
            prop_assert_eq!(device[3], side as f32);
            prop_assert_eq!(device[0].fract(), 0.0);
            let centre = f64::from(device[0]) + f64::from(side) / 2.0;
            prop_assert!((centre - to_device(x, scale)).abs() <= 0.5 + 1e-6);
        }
    }
}
