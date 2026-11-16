//! `SheetView`: the sheet element. T1.4 paints the paper and the dot grid.
//!
//! The element implements [`Element`] directly (G8: `canvas()` boxes two closures per frame)
//! and paints inside one [`Window::paint_layer`] (G5). Grid dots are plain square quads of
//! whole device pixels (D4, G3), placed with [`crate::snap_px`] so they never move by a
//! fraction of a pixel and never vary in size (D5).
//!
//! Grid density (K10): every `2^k`-th grid point is shown, with `k` chosen so the on-screen
//! pitch stays in [20, 40) logical px. Above 200% zoom `k` is negative and the dots sit on
//! the binary subdivisions of the grid (`k = -1` is the K2 pin lattice); `2,540,000 nm` is
//! `2^5 × 79,375`, so every level down to `k = -5`, which K4's 3200% needs, is a whole number
//! of nm. Levels are nested: zooming only adds or removes dots, it never moves one. While
//! the pitch is in the top quarter of the range, [35, 40), the dots of the next finer level
//! (the ones that drop when zooming out past 40 px) are drawn too, their alpha rising from
//! 0 at 35 px to full at 40 px, so the switch between levels is continuous.
//!
//! The paint body allocates nothing (B10, INV7): it walks the grid directly and emits each
//! quad as it goes, so there is no buffer to grow.

use crate::camera::Camera;
use crate::snap_px::{centered_square, stroke_device_px};
use gpui::{
    App, Bounds, ContentMask, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    LayoutId, PaintQuad, Pixels, Style, Window, fill, relative,
};
use omnia_doc::{GRID_NM, Pt, Rect};
use omnia_theme::Theme;

/// K10: the on-screen grid pitch stays at or above this many logical px.
pub const MIN_PITCH: f64 = 20.0;
/// K10: the on-screen grid pitch stays below this many logical px.
pub const MAX_PITCH: f64 = 40.0;
/// K10: the next finer level fades in over the top quarter of the pitch range.
pub const FADE_START: f64 = MAX_PITCH - (MAX_PITCH - MIN_PITCH) / 4.0;
/// K6: grid dot size in logical px; at least one device pixel.
pub const GRID_DOT_PX: f64 = 1.2;
/// Finest grid level: `GRID_NM / 32`, reached at K4's 3200%.
pub const MIN_LEVEL: i32 = -5;
/// Coarsest grid level: `GRID_NM × 32`, reached at K4's 5%.
pub const MAX_LEVEL: i32 = 5;

const _: () = assert!(GRID_NM % (1 << -MIN_LEVEL) == 0);
const _: () = assert!((GRID_NM as i64) << MAX_LEVEL <= i32::MAX as i64);

/// The grid level shown at one camera scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLevel {
    /// `k`: every `2^k`-th grid point is shown (negative: grid subdivisions).
    pub level: i32,
    /// World distance between shown dots, `GRID_NM × 2^k`.
    pub step_nm: i32,
    /// On-screen distance between shown dots, in logical px.
    pub pitch: f64,
    /// Alpha factor in [0, 1] for the dots of the next finer level, which are drawn only
    /// when this is above zero. It reaches 1 only by f32 rounding just below 40 px.
    pub fade: f32,
}

/// K10 level selection for a camera scale in logical px per nm.
pub fn grid_level(scale: f64) -> GridLevel {
    assert!(scale.is_finite() && scale > 0.0);
    let mut level = 0;
    let mut pitch = f64::from(GRID_NM) * scale;
    // Doubling and halving are exact in f64, so `pitch == step_nm × scale` to the last bit.
    while pitch < MIN_PITCH && level < MAX_LEVEL {
        pitch *= 2.0;
        level += 1;
    }
    while pitch >= MAX_PITCH && level > MIN_LEVEL {
        pitch /= 2.0;
        level -= 1;
    }
    let step_nm = if level >= 0 {
        GRID_NM << level
    } else {
        GRID_NM >> -level
    };
    let fade = if level > MIN_LEVEL && pitch > FADE_START {
        ((pitch - FADE_START) / (MAX_PITCH - FADE_START)) as f32
    } else {
        0.0
    };
    GridLevel {
        level,
        step_nm,
        pitch,
        fade,
    }
}

/// Converts an `omnia-theme` token into GPUI's colour type.
fn gpui_color(c: omnia_theme::Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

/// The sheet element: paper and grid (T1.4). Cheap to build every frame; it owns no heap
/// data.
#[derive(Clone, Debug)]
pub struct SheetView {
    camera: Camera,
    theme: &'static Theme,
}

impl SheetView {
    /// The camera's viewport is replaced by the element's bounds at paint time.
    pub fn new(camera: Camera, theme: &'static Theme) -> Self {
        Self { camera, theme }
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// The whole paint body, with the GPUI call left to `sink`, so benches and tests can
    /// separate our allocations from GPUI's (Q5). [`Element::paint`] passes
    /// `Window::paint_quad`. Emits the paper first, then one quad per visible dot.
    pub fn paint_quads(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        mut sink: impl FnMut(PaintQuad),
    ) {
        assert!(scale_factor.is_finite() && scale_factor > 0.0);
        sink(fill(bounds, gpui_color(self.theme.stage_paper)));

        let mut camera = self.camera.clone();
        camera.set_viewport(bounds);
        let level = grid_level(camera.scale());
        let side = stroke_device_px(GRID_DOT_PX, scale_factor);
        let grid = gpui_color(self.theme.stage_grid);
        let area = camera.visible();
        if level.fade > 0.0 {
            // The finer level, with the dots that are not on the shown level faded.
            let faded = grid.opacity(level.fade);
            for_each_dot(&camera, area, level.step_nm / 2, |i, j, p| {
                let color = if i % 2 == 0 && j % 2 == 0 {
                    grid
                } else {
                    faded
                };
                sink(fill(centered_square(p, side, scale_factor), color));
            });
        } else {
            for_each_dot(&camera, area, level.step_nm, |_, _, p| {
                sink(fill(centered_square(p, side, scale_factor), grid));
            });
        }
    }
}

/// Calls `f(i, j, screen)` for every lattice point `(i × step, j × step)` inside `area`.
fn for_each_dot(
    camera: &Camera,
    area: Rect,
    step: i32,
    mut f: impl FnMut(i64, i64, gpui::Point<f64>),
) {
    let step = i64::from(step);
    let range = |min: i32, max: i32| {
        let first =
            i64::from(min).div_euclid(step) + i64::from(i64::from(min).rem_euclid(step) != 0);
        let last = i64::from(max).div_euclid(step);
        first..=last
    };
    let columns = range(area.min.x, area.max.x);
    for j in range(area.min.y, area.max.y) {
        for i in columns.clone() {
            // In range: `area` is clipped to K1 and the products lie inside it.
            let p = Pt {
                x: (i * step) as i32,
                y: (j * step) as i32,
            };
            f(i, j, camera.world_to_screen(p));
        }
    }
}

impl Element for SheetView {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let scale_factor = window.scale_factor();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_layer(|window| {
                self.paint_quads(bounds, scale_factor, |quad| window.paint_quad(quad));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{BASE_SCALE, MAX_ZOOM, MIN_ZOOM};
    use gpui::{Background, point, px, size};

    fn check(level: GridLevel, scale: f64) {
        assert!(
            (MIN_PITCH..MAX_PITCH).contains(&level.pitch),
            "pitch {} at scale {scale}",
            level.pitch
        );
        assert_eq!(level.pitch, f64::from(level.step_nm) * scale);
        assert!((MIN_LEVEL..=MAX_LEVEL).contains(&level.level));
        assert!((0.0..=1.0).contains(&level.fade));
        assert_eq!(level.fade > 0.0, level.pitch > FADE_START);
        if level.fade > 0.0 {
            assert_eq!(level.step_nm % 2, 0, "the finer level is whole nm");
        }
    }

    #[test]
    fn pitch_stays_in_range_across_k4() {
        // Dense log sweep, both ends, and every wheel notch (K4: 2^(1/4)).
        let steps = 100_000;
        let ratio = (MAX_ZOOM / MIN_ZOOM).ln();
        for n in 0..=steps {
            let zoom = (MIN_ZOOM * (ratio * f64::from(n) / f64::from(steps)).exp())
                .clamp(MIN_ZOOM, MAX_ZOOM);
            let scale = BASE_SCALE * zoom;
            check(grid_level(scale), scale);
        }
        let mut zoom = 1.0f64;
        while zoom > MIN_ZOOM {
            zoom = (zoom / 2f64.powf(0.25)).max(MIN_ZOOM);
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
        let mut zoom = 1.0f64;
        while zoom < MAX_ZOOM {
            zoom = (zoom * 2f64.powf(0.25)).min(MAX_ZOOM);
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
        for zoom in [MIN_ZOOM, MAX_ZOOM] {
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
    }

    #[test]
    fn known_levels() {
        let at = |zoom: f64| grid_level(BASE_SCALE * zoom);
        assert_eq!(
            (at(1.0).level, at(1.0).pitch, at(1.0).step_nm),
            (0, 20.0, GRID_NM)
        );
        assert_eq!((at(0.5).level, at(0.5).pitch), (1, 20.0));
        assert_eq!((at(2.0).level, at(2.0).pitch), (-1, 20.0));
        assert_eq!(at(2.0).step_nm, omnia_doc::PIN_LATTICE_NM);
        // BASE_SCALE × zoom is not exact in f64; the ends of K4 land within an ulp or so.
        assert_eq!(at(MIN_ZOOM).level, 5);
        assert!((at(MIN_ZOOM).pitch - 32.0).abs() < 1e-9);
        assert_eq!(at(MAX_ZOOM).level, -5);
        assert!((at(MAX_ZOOM).pitch - 20.0).abs() < 1e-9);
        assert_eq!(at(MAX_ZOOM).step_nm, GRID_NM / 32);
        assert_eq!(at(1.75).fade, 0.0);
        assert_eq!(at(1.875).fade, 0.5);
    }

    #[test]
    fn levels_nest_and_fade_is_continuous() {
        // Just below a level switch the faded dots are nearly opaque; just above it the
        // finer level is shown at full alpha with no fade: no dot pops.
        for k in MIN_LEVEL + 1..=MAX_LEVEL {
            let switch = MAX_PITCH / (f64::from(GRID_NM) * 2f64.powi(k));
            let below = grid_level(switch * (1.0 - 1e-9));
            let above = grid_level(switch * (1.0 + 1e-9));
            assert_eq!(below.level, k);
            assert!(below.fade > 0.999);
            assert_eq!(above.level, k - 1);
            assert_eq!(above.fade, 0.0);
            assert_eq!(below.step_nm / 2, above.step_nm);
        }
    }

    fn sheet(zoom: f64, pan: (f64, f64)) -> (SheetView, Bounds<Pixels>) {
        let bounds = Bounds {
            origin: point(px(10.0), px(20.0)),
            size: size(px(800.0), px(600.0)),
        };
        let mut camera = Camera::new(Pt::ZERO, zoom, bounds);
        camera.pan_by(point(pan.0, pan.1));
        (SheetView::new(camera, &omnia_theme::GRAPHITE), bounds)
    }

    #[test]
    fn paints_paper_then_every_visible_dot() {
        let (view, bounds) = sheet(1.0, (0.25, -0.5));
        let mut quads = Vec::new();
        view.paint_quads(bounds, 1.0, |q| quads.push(q));
        assert_eq!(quads[0].bounds, bounds);
        assert!(
            quads[0].background == Background::from(gpui_color(omnia_theme::GRAPHITE.stage_paper))
        );
        // 800 x 600 px at a 20 px pitch: 40 or 41 columns, 30 or 31 rows.
        let dots = &quads[1..];
        assert!((40 * 30..=41 * 31).contains(&dots.len()), "{}", dots.len());
        let grid = Background::from(gpui_color(omnia_theme::GRAPHITE.stage_grid));
        assert!(dots.iter().all(|q| q.background == grid));
        for q in dots {
            assert_eq!(q.bounds.size, size(px(1.0), px(1.0)));
        }
    }

    #[test]
    fn fade_draws_the_finer_level_faded() {
        let (view, bounds) = sheet(1.875, (0.0, 0.0));
        let mut quads = Vec::new();
        view.paint_quads(bounds, 1.5, |q| quads.push(q));
        let grid = gpui_color(omnia_theme::GRAPHITE.stage_grid);
        let full = quads[1..]
            .iter()
            .filter(|q| q.background == Background::from(grid))
            .count();
        let faded = quads[1..]
            .iter()
            .filter(|q| q.background == Background::from(grid.opacity(0.5)))
            .count();
        assert_eq!(full + faded, quads.len() - 1);
        // The faded dots are three of every four lattice points.
        assert!(faded > 2 * full && faded < 4 * full, "{full} {faded}");
    }
}
