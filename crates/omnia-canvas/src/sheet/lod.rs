//! D4, D5 and K9. Dimensions are logical pixels, independent of the display scale.

use crate::camera::Camera;

pub(super) const JUNCTION_DIAMETER_NM: f64 = 1_016_000.0;
const MIN_JUNCTION_PX: f64 = 2.5;
/// K9 / D4: far-zoom merged wire runs strictly shorter than this many logical px are
/// hidden; runs at or above this length are drawn.
pub(super) const MIN_WIRE_RUN_PX: f64 = 2.5;
const MIN_SYMBOL_PX: f64 = 8.0;

pub(super) fn junction_diameter(camera: &Camera) -> Option<f64> {
    let diameter = JUNCTION_DIAMETER_NM * camera.scale();
    (diameter >= MIN_JUNCTION_PX).then_some(diameter)
}

/// D4 / K9: whether a merged wire run of `length_px` logical pixels is drawn at far zoom.
/// Runs strictly below [`MIN_WIRE_RUN_PX`] are hidden; runs at or above it are drawn.
pub(super) fn wire_run(length_px: f64) -> bool {
    length_px >= MIN_WIRE_RUN_PX
}

pub(crate) fn symbol_box(side_px: f64) -> bool {
    side_px < MIN_SYMBOL_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k9_wire_run_threshold_boundary() {
        assert_eq!(MIN_WIRE_RUN_PX, 2.5);
        assert!(!wire_run(0.0));
        assert!(!wire_run(-1.0));
        assert!(!wire_run(f64::NAN));
        assert!(!wire_run(MIN_WIRE_RUN_PX - 1e-9));
        assert!(wire_run(MIN_WIRE_RUN_PX));
        assert!(wire_run(MIN_WIRE_RUN_PX + 1e-9));
        assert!(wire_run(10.0));
    }
}
