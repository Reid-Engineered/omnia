//! What the bench window draws: the T0.1 paper window, or a placeholder scene painted from
//! a custom `Element` until T2.5 renders real sheets.

use crate::alloc::{not_ours, ours};
use crate::driver::Camera;
use gpui::{
    App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId, LayoutId, Pixels,
    Style, Window, fill, point, px, relative, rgb, rgba, size,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

// Temporary until T1.2 generates omnia-theme tokens. PAPER is Theme/stage/paper in
// omnia.figcraft (the same constant as omnia-app); INK and GRID are the Q2 defaults.
pub const PAPER: u32 = 0xEFEDE6;
const INK: u32 = 0x1D1D1F;
const GRID: u32 = 0x00000022;

/// Grid pitch at 100% zoom, in logical pixels (K3).
const GRID_PITCH: f32 = 20.0;
/// Grid dot size in logical pixels (K6); never below one device pixel.
const GRID_DOT: f32 = 1.2;
/// Wire stroke in logical pixels (K5).
const WIRE_STROKE: f32 = 1.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sheet {
    /// The T0.1 window: one `stage/paper` rectangle, drawn exactly as `omnia-app` draws it.
    Empty,
    /// Paper, a dot grid and orthogonal wire quads that follow the camera.
    Placeholder,
    Spike(crate::spike::Config),
}

impl Sheet {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "empty" => Ok(Self::Empty),
            "placeholder" => Ok(Self::Placeholder),
            "100" | "1k" | "10k" | "dense" | "text" => Err(format!(
                "sheet `{name}` needs the T2.4 generators and T2.5 sheet rendering"
            )),
            _ => Err(format!(
                "unknown sheet `{name}` (available: empty, placeholder)"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Placeholder => "placeholder",
            Self::Spike(_) => "spike",
        }
    }

    /// The scene painted by a [`SceneElement`], or `None` for the T0.1 window.
    pub fn scene(self) -> Option<Rc<RefCell<dyn BenchScene>>> {
        match self {
            Self::Empty => None,
            Self::Spike(config) => Some(Rc::new(RefCell::new(crate::spike::Scene::new(config)))),
            Self::Placeholder => Some(Rc::new(RefCell::new(Placeholder::new(0x0A11_CE5E)))),
        }
    }
}

/// A scene the harness can drive. T0.4's spike scenes implement this.
pub trait BenchScene: 'static {
    fn after_frame(&mut self, _window: &Window) {}
    fn details(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Paints one frame filling `bounds`. Runs inside [`ours`] within one `paint_layer`;
    /// wrap calls into GPUI in [`not_ours`] so GPUI's allocations count only in "total" (Q5).
    fn paint(&mut self, bounds: Bounds<Pixels>, camera: Camera, window: &mut Window, cx: &mut App);
}

/// Paints a [`BenchScene`] into the whole window inside one `paint_layer`, and records how
/// long our paint took in `paint_time`.
pub struct SceneElement {
    pub scene: Rc<RefCell<dyn BenchScene>>,
    pub camera: Camera,
    pub paint_time: Rc<Cell<Option<Duration>>>,
}

impl Element for SceneElement {
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
        cx: &mut App,
    ) {
        let start = Instant::now();
        let camera = self.camera;
        let scene = &self.scene;
        window.paint_layer(|window| {
            ours(|| scene.borrow_mut().paint(bounds, camera, window, cx));
        });
        self.paint_time.set(Some(start.elapsed()));
    }
}

/// An axis-aligned wire quad in sheet space, in logical pixels at 100% zoom.
#[derive(Clone, Copy)]
struct WireQuad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

/// Paper, a dot grid at the K3 pitch and 2,000 seeded orthogonal wires over a
/// 3,000 x 2,000 px sheet. Allocates only in `new`.
pub struct Placeholder {
    wires: Vec<WireQuad>,
    paper: Hsla,
    ink: Hsla,
    grid: Hsla,
}

impl Placeholder {
    pub const WIRES: usize = 2_000;
    const SHEET: (f32, f32) = (3_000.0, 2_000.0);

    pub fn new(seed: u64) -> Self {
        let mut state = seed;
        let mut next = move || {
            // SplitMix64: deterministic, no dependency.
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        let grid_steps = |limit: f32, r: u64| (r % (limit / GRID_PITCH) as u64) as f32 * GRID_PITCH;
        let wires = (0..Self::WIRES)
            .map(|_| {
                let x = grid_steps(Self::SHEET.0, next());
                let y = grid_steps(Self::SHEET.1, next());
                let len = (1 + next() % 12) as f32 * GRID_PITCH;
                let half = WIRE_STROKE / 2.0;
                if next() % 2 == 0 {
                    WireQuad {
                        x,
                        y: y - half,
                        w: len,
                        h: WIRE_STROKE,
                    }
                } else {
                    WireQuad {
                        x: x - half,
                        y,
                        w: WIRE_STROKE,
                        h: len,
                    }
                }
            })
            .collect();
        Self {
            wires,
            paper: rgb(PAPER).into(),
            ink: rgb(INK).into(),
            grid: rgba(GRID).into(),
        }
    }
}

impl BenchScene for Placeholder {
    fn paint(
        &mut self,
        bounds: Bounds<Pixels>,
        camera: Camera,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let scale = window.scale_factor();
        let (cam_x, cam_y) = camera.logical(scale);
        let origin_x = f32::from(bounds.origin.x);
        let origin_y = f32::from(bounds.origin.y);
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);

        not_ours(|| window.paint_quad(fill(bounds, self.paper)));

        let dot = GRID_DOT.max(1.0 / scale);
        let first_x = (-cam_x).rem_euclid(GRID_PITCH);
        let first_y = (-cam_y).rem_euclid(GRID_PITCH);
        let mut y = first_y;
        while y < height {
            let mut x = first_x;
            while x < width {
                let quad = fill(
                    Bounds::new(
                        point(px(origin_x + x), px(origin_y + y)),
                        size(px(dot), px(dot)),
                    ),
                    self.grid,
                );
                not_ours(|| window.paint_quad(quad));
                x += GRID_PITCH;
            }
            y += GRID_PITCH;
        }

        for wire in &self.wires {
            let x = wire.x - cam_x;
            let y = wire.y - cam_y;
            if x + wire.w < 0.0 || y + wire.h < 0.0 || x > width || y > height {
                continue;
            }
            let quad = fill(
                Bounds::new(
                    point(px(origin_x + x), px(origin_y + y)),
                    size(px(wire.w), px(wire.h)),
                ),
                self.ink,
            );
            not_ours(|| window.paint_quad(quad));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_seeded_and_on_grid() {
        let a = Placeholder::new(1);
        let b = Placeholder::new(1);
        assert_eq!(a.wires.len(), Placeholder::WIRES);
        for (wa, wb) in a.wires.iter().zip(&b.wires) {
            assert_eq!((wa.x, wa.y, wa.w, wa.h), (wb.x, wb.y, wb.w, wb.h));
            let horizontal = wa.h == WIRE_STROKE;
            let along = if horizontal { wa.x } else { wa.y };
            assert_eq!(along % GRID_PITCH, 0.0);
        }
    }

    #[test]
    fn sheet_names() {
        assert_eq!(Sheet::parse("empty"), Ok(Sheet::Empty));
        assert_eq!(Sheet::parse("placeholder"), Ok(Sheet::Placeholder));
        assert!(Sheet::parse("10k").unwrap_err().contains("T2.5"));
        assert!(Sheet::parse("nope").is_err());
        assert!(Sheet::Empty.scene().is_none() && Sheet::Placeholder.scene().is_some());
    }
}
