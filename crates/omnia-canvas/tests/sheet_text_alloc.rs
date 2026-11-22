//! T3.1 / B10 / INV7: `SheetView`'s full paint path (including labels) makes zero heap
//! allocations from our code once warm on the canonical `text` stress sheet at a fixed zoom
//! (and across repeated pans and zoom sweeps), plus multiline net labels.
//!
//! Headless `TestApp` companion to `sheet_text_render.rs` (which verifies B10 on the real Windows
//! rendering backend with non-zero glyph sprite emission). Uses a test-local counting allocator and
//! `SheetPaintScope` (Q5), matching `sheet_alloc.rs` and `omnia-bench`'s `BenchPaintScope`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use gpui::{
    AppContext, Bounds, Context, Entity, IntoElement, Render, TestApp, Window, WindowBounds,
    WindowOptions, point, px, size,
};
use omnia_canvas::{
    Camera, SheetView,
    sheet::{SheetDocument, SheetPaintScope},
};
use omnia_doc::{Doc, Label, Orient, PIN_LATTICE_NM, Part, Pt, Wire};
use omnia_symbols::SymbolLibrary;

struct Counting;

thread_local! {
    static OURS: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn record() {
    if OURS.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: every method forwards to `System` unchanged; thread-local counters never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct TestScope;

impl SheetPaintScope for TestScope {
    fn omnia(&self, paint: impl FnOnce()) {
        let prev = OURS.with(|c| c.replace(true));
        paint();
        OURS.with(|c| c.set(prev));
    }

    fn gpui(&self, paint: impl FnOnce()) {
        let prev = OURS.with(|c| c.replace(false));
        paint();
        OURS.with(|c| c.set(prev));
    }
}

/// Canonical `text` stress sheet matching `crates/omnia-bench/src/generate.rs`
/// (`StressSheet::TextHeavy`, seed 42): 1,000 parts with 6-char designators and 6-char values
/// (12,000 characters), 2,000 wires, and 0 net labels.
fn canonical_text_stress_doc() -> Doc {
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
            z ^ (z >> 31)
        }
    }

    let symbols = SymbolLibrary::builtin();
    let defs: Vec<_> = symbols.iter().collect();
    let count = 1_000usize;
    let wire_count = 2_000usize;
    let columns = 32usize;
    let rows = count.div_ceil(columns);
    let pitch = 12 * PIN_LATTICE_NM;
    let mut rng = SplitMix64(42);
    let mut parts = Vec::with_capacity(count);
    let mut wires = Vec::with_capacity(wire_count);
    for i in 0..count {
        let def = defs[(rng.next() % defs.len() as u64) as usize];
        let orient = Orient::ALL[(rng.next() % 8) as usize];
        let at = Pt {
            x: (i % columns) as i32 * pitch
                - (columns as i32 - 1) * pitch / 2 / PIN_LATTICE_NM * PIN_LATTICE_NM,
            y: (i / columns) as i32 * pitch
                - (rows as i32 - 1) * pitch / 2 / PIN_LATTICE_NM * PIN_LATTICE_NM,
        };
        let pin = at + orient.apply(def.pins[0].at);
        for j in 0..wire_count / count {
            let delta = match j % 4 {
                0 => Pt {
                    x: PIN_LATTICE_NM,
                    y: 0,
                },
                1 => Pt {
                    x: 0,
                    y: PIN_LATTICE_NM,
                },
                2 => Pt {
                    x: -PIN_LATTICE_NM,
                    y: 0,
                },
                _ => Pt {
                    x: 0,
                    y: -PIN_LATTICE_NM,
                },
            };
            let a = pin
                + Pt {
                    x: delta.x * (j / 4) as i32,
                    y: delta.y * (j / 4) as i32,
                };
            wires.push(Wire { a, b: a + delta });
        }
        parts.push((
            Part {
                symbol: def.id,
                at,
                orient,
                designator: format!("R{i:05}").into(),
                value: format!("{:06}", rng.next() % 1_000_000).into(),
            },
            def.pins.iter().map(|p| p.at).collect(),
            def.bounds,
        ));
    }
    Doc::from_items(parts, wires, std::iter::empty::<Label>()).expect("stress sheet valid")
}

struct ScopedSheet {
    camera: Camera,
    document: SheetDocument,
}

impl gpui::Element for ScopedSheet {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let mut style = gpui::Style::default();
        style.size.width = gpui::relative(1.0).into();
        style.size.height = gpui::relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut gpui::App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<gpui::Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut gpui::App,
    ) {
        SheetView::with_document(
            self.camera.clone(),
            &omnia_theme::GRAPHITE,
            self.document.clone(),
        )
        .paint_with_scope(bounds, window, cx, &TestScope);
    }
}

struct Host {
    camera: Camera,
    document: SheetDocument,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        ScopedSheet {
            camera: self.camera.clone(),
            document: self.document.clone(),
        }
    }
}

#[test]
fn t3_1_canonical_text_stress_sheet_zero_omnia_allocations_once_warm() {
    let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0)));
    let doc = canonical_text_stress_doc();
    assert_eq!(doc.part_count(), 1_000);
    assert_eq!(doc.wires().count(), 2_000);
    assert_eq!(doc.labels().count(), 0);
    let chars: usize = doc
        .parts()
        .map(|(_, p)| p.designator.len() + p.value.len())
        .sum();
    assert_eq!(chars, 12_000);

    let mut app = TestApp::new();
    let doc_entity: Entity<Doc> = app.update(|cx| cx.new(|_| doc));
    let document = SheetDocument::new(doc_entity);
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, _| Host {
            camera: Camera::new(Pt::ZERO, 1.0, bounds),
            document,
        },
    );

    // Warm up frame 0 at 100% zoom.
    window.draw();

    // Fixed-zoom pan across 120 frames (bringing previously offscreen parts into view):
    // our code must allocate zero times on every frame.
    for frame in 0..120 {
        window.update(|host, _, cx| {
            let mut cam = Camera::new(Pt::ZERO, 1.0, bounds);
            cam.pan_by(point(-f64::from(frame) * 3.5, f64::from(frame % 7) * 0.5));
            host.camera = cam;
            cx.notify();
        });
        ALLOCS.with(|n| n.set(0));
        OURS.with(|c| c.set(false));
        window.draw();
        let allocs = ALLOCS.with(Cell::get);
        assert_eq!(
            allocs, 0,
            "fixed-zoom pan frame {frame} allocated {allocs} times in our paint code"
        );
    }

    // Warm up query scratch at far zoom once so `Doc::query` has peak capacity.
    window.update(|host, _, cx| {
        host.camera = Camera::new(Pt::ZERO, omnia_canvas::camera::MIN_ZOOM, bounds);
        cx.notify();
    });
    window.draw();

    for step in 0..60 {
        let t = f64::from(step) / 59.0;
        let zoom = (omnia_canvas::camera::MIN_ZOOM.log2()
            + t * (omnia_canvas::camera::MAX_ZOOM.log2() - omnia_canvas::camera::MIN_ZOOM.log2()))
        .exp2();
        window.update(|host, _, cx| {
            host.camera = Camera::new(Pt::ZERO, zoom, bounds);
            cx.notify();
        });
        ALLOCS.with(|n| n.set(0));
        OURS.with(|c| c.set(false));
        window.draw();
        let allocs = ALLOCS.with(Cell::get);
        assert_eq!(
            allocs, 0,
            "zoom-sweep step {step} (zoom {zoom:.3}) allocated {allocs} times in our paint code"
        );
    }
}

#[test]
fn multiline_net_labels_zero_omnia_allocations_once_warm() {
    let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(400.0)));
    let labels = [
        Label {
            at: Pt {
                x: -4 * PIN_LATTICE_NM,
                y: 0,
            },
            text: "BUS\nA".into(),
        },
        Label {
            at: Pt {
                x: 4 * PIN_LATTICE_NM,
                y: 0,
            },
            text: "BUS\r\nB".into(),
        },
    ];
    let doc = Doc::from_items(std::iter::empty(), std::iter::empty(), labels).unwrap();

    let mut app = TestApp::new();
    let doc_entity: Entity<Doc> = app.update(|cx| cx.new(|_| doc));
    let document = SheetDocument::new(doc_entity);
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        move |_, _| Host {
            camera: Camera::new(Pt::ZERO, 1.0, bounds),
            document,
        },
    );

    window.draw();
    for frame in 0..20 {
        window.update(|host, _, cx| {
            let mut cam = Camera::new(Pt::ZERO, 1.0, bounds);
            cam.pan_by(point(f64::from(frame) * 1.25, -f64::from(frame) * 0.75));
            host.camera = cam;
            cx.notify();
        });
        ALLOCS.with(|n| n.set(0));
        OURS.with(|c| c.set(false));
        window.draw();
        assert_eq!(ALLOCS.with(Cell::get), 0);
    }
    // Changing font-size bucket (reshaping multiline/escaped labels) must also make 0 Omnia
    // allocations because display strings are prepared at revision sync, never during shaping.
    for &zoom in &[1.25f64, 1.75, 0.8] {
        window.update(|host, _, cx| {
            host.camera = Camera::new(Pt::ZERO, zoom, bounds);
            cx.notify();
        });
        ALLOCS.with(|n| n.set(0));
        OURS.with(|c| c.set(false));
        window.draw();
        assert_eq!(
            ALLOCS.with(Cell::get),
            0,
            "multiline reshape at zoom {zoom} allocated in Omnia code"
        );
    }
}
