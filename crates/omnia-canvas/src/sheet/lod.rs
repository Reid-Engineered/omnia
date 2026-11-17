//! D5 and K9. Dimensions are logical pixels, independent of the display scale.

use crate::camera::Camera;

pub(super) const JUNCTION_DIAMETER_NM: f64 = 1_016_000.0;
const MIN_JUNCTION_PX: f64 = 2.5;
const MIN_SYMBOL_PX: f64 = 8.0;

pub(super) fn junction_diameter(camera: &Camera) -> Option<f64> {
    let diameter = JUNCTION_DIAMETER_NM * camera.scale();
    (diameter >= MIN_JUNCTION_PX).then_some(diameter)
}

pub(super) fn symbol_box(side_px: f64) -> bool {
    side_px < MIN_SYMBOL_PX
}
