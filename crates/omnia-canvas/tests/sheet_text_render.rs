//! T3.1: Windows hidden-window verification (`render_to_image` + `rendered_scene` + counting
//! allocator) on the real DirectX/DirectWrite rendering backend:
//! 1. B10 real-backend zero-allocation proof on the canonical `text` stress sheet (matching
//!    `omnia-bench/src/generate.rs` `StressSheet::TextHeavy`, seed 42, 1,000 parts, 2,000 wires,
//!    0 net labels, 12,000 chars), asserting every measured frame actually emits non-zero glyph
//!    sprites from the text layer.
//! 2. Visual sanity via `render_to_image` at scale factors 1.0, 1.25, 1.5; K9/D5 drop order
//!    verified on both sides of the ordinary-text floor (6.5 px) and the pin-number/junction
//!    threshold (2.5 px = 31.25% zoom); and K9's 700-label cap.
//! 3. Isolated glyph + emoji atlas stability across two full K4 zoom sweeps (excluding SVG symbol
//!    and dot tiles): proves at least two distinct glyph size buckets emit glyph tiles in pass 1,
//!    and pass 2 leaves glyph/emoji atlas pages, bytes, and distinct tiles identical to pass 1.
//!
//! Skips under `OMNIA_HEADLESS_CI` and on Linux/FreeBSD (no `render_to_image`, T0.8), matching
//! `sheet_render.rs`.

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod render {
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        borrow::Cow,
        cell::{Cell, RefCell},
        collections::{BTreeMap, BTreeSet},
        rc::Rc,
        time::Duration,
    };

    use gpui::{
        App, AppContext, Bounds, Context, IntoElement, Render, Scene, Window, WindowBounds,
        WindowHandle, WindowOptions, point, px, size,
    };
    use omnia_canvas::{
        Camera, SheetView,
        sheet::{SheetDocument, SheetPaintScope},
    };
    use omnia_doc::{Command, Doc, GRID_NM, Label, Orient, PIN_LATTICE_NM, Part, Pt, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;

    struct Counting;

    thread_local! {
        static OURS: Cell<bool> = const { Cell::new(false) };
        static ALLOCS: Cell<u64> = const { Cell::new(0) };
    }

    fn record_alloc() {
        if OURS.try_with(Cell::get).unwrap_or(false) {
            let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        }
    }

    // SAFETY: forwards every call to `System`; thread-local counters never allocate.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record_alloc();
            // SAFETY: forwarded contract.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record_alloc();
            // SAFETY: forwarded contract.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: forwarded contract.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            record_alloc();
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
            cx: &mut App,
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
            _cx: &mut App,
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
            cx: &mut App,
        ) {
            SheetView::with_document(self.camera.clone(), &GRAPHITE, self.document.clone())
                .paint_with_scope(bounds, window, cx, &TestScope);
        }
    }

    struct Probe {
        camera: Camera,
        document: SheetDocument,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            ScopedSheet {
                camera: self.camera.clone(),
                document: self.document.clone(),
            }
        }
    }

    /// Isolates glyph and emoji sprites (`paint_glyph` / `paint_emoji`) from SVG symbol and dot
    /// sprites (`paint_svg`) using pinned GPUI `window.rs` invariants:
    /// - `SubpixelSprite` is emitted exclusively by `paint_glyph` (`window.rs:5291`).
    /// - `PolychromeSprite` on the sheet is emitted exclusively by `paint_emoji` (`window.rs:5375`).
    /// - `MonochromeSprite` from `paint_glyph` has 1:1 `bounds.size == tile.bounds.size` and
    ///   `TransformationMatrix::unit()` (`window.rs:5286, 5308`), whereas `paint_svg` rasterises
    ///   at `SMOOTH_SVG_SCALE_FACTOR = 2.0` (`window.rs:5459`), so `tile.bounds.size` is 2x
    ///   `bounds.size`.
    fn glyph_and_emoji_sprites(scene: &Scene) -> Vec<(gpui::AtlasTile, bool)> {
        let mut out = Vec::new();
        for s in &scene.subpixel_sprites {
            out.push((s.tile, false));
        }
        for s in &scene.polychrome_sprites {
            out.push((s.tile, true));
        }
        let unit = gpui::TransformationMatrix::unit();
        for s in &scene.monochrome_sprites {
            let is_1_to_1 = s.bounds.size.width.0 == s.tile.bounds.size.width.0 as f32
                && s.bounds.size.height.0 == s.tile.bounds.size.height.0 as f32;
            if is_1_to_1 && s.transformation == unit {
                out.push((s.tile, false));
            }
        }
        out
    }

    #[derive(Default)]
    struct GlyphAtlasStats {
        pages: BTreeMap<(u8, u32), (i32, i32)>,
        tiles: BTreeSet<(u8, u32, u32)>,
        emoji_tiles: BTreeSet<(u8, u32, u32)>,
    }

    impl GlyphAtlasStats {
        /// Records every text-layer glyph/emoji tile emitted in `window`'s current scene,
        /// returning `(sprite_count, max_non_emoji_glyph_height_px)`.
        fn record_text_layer(&mut self, window: &Window) -> (usize, Option<i32>) {
            let sprites = glyph_and_emoji_sprites(window.rendered_scene());
            let mut max_glyph_h: Option<i32> = None;
            for &(tile, is_emoji) in &sprites {
                let kind = tile.texture_id.kind as u8;
                let tex = tile.texture_id.index;
                // Same page-size accounting rule as spike S5 (DirectX/wgpu min page 1024x1024).
                self.pages.entry((kind, tex)).or_insert((
                    1024.max(tile.bounds.size.width.0),
                    1024.max(tile.bounds.size.height.0),
                ));
                let key = (kind, tex, tile.tile_id.0);
                self.tiles.insert(key);
                if is_emoji {
                    self.emoji_tiles.insert(key);
                } else {
                    let h = tile.bounds.size.height.0;
                    max_glyph_h = Some(max_glyph_h.map_or(h, |prev| prev.max(h)));
                }
            }
            (sprites.len(), max_glyph_h)
        }

        fn bytes(&self) -> i64 {
            self.pages
                .iter()
                .map(|((kind, _), (w, h))| {
                    i64::from(*w) * i64::from(*h) * if *kind == 0 { 1 } else { 4 }
                })
                .sum()
        }
    }

    fn load_bundled_inter(cx: &App) -> Result<(), String> {
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice(),
                ),
            ])
            .map_err(|e| format!("register bundled Inter fonts: {e}"))
    }

    /// Canonical `text` stress sheet matching `crates/omnia-bench/src/generate.rs`
    /// (`StressSheet::TextHeavy`, seed 42): 1,000 parts, 2,000 wires, 0 net labels, 12,000 chars.
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

    fn build_sample_doc(with_text: bool, with_emoji: bool) -> Doc {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let opamp = library.get(library.by_name("IC-COM-OpAmp").unwrap());
        let cap = library.get(library.by_name("Capacitor-IEEE-NonPolarized").unwrap());
        let mut doc = Doc::new();
        // Place the op-amp centered at Pt::ZERO so SymbolsAndDots always emits a symbol sprite
        // across every zoom in K4 where text is visible (>= 31.25%).
        for (def, at, orient, desig, val) in [
            (
                opamp,
                Pt::ZERO,
                Orient::IDENTITY,
                if with_text { "U1" } else { "" },
                if with_text { "TL072" } else { "" },
            ),
            (
                resistor,
                Pt {
                    x: -9 * GRID_NM,
                    y: 0,
                },
                Orient::IDENTITY,
                if with_text { "R1" } else { "" },
                if with_text { "10k" } else { "" },
            ),
            (
                cap,
                Pt {
                    x: -9 * GRID_NM,
                    y: 5 * GRID_NM,
                },
                Orient::ROT_90,
                if with_text { "C1" } else { "" },
                if with_text { "22n" } else { "" },
            ),
        ] {
            doc.apply(Command::PlacePart {
                part: Part {
                    symbol: def.id,
                    at,
                    orient,
                    designator: desig.into(),
                    value: val.into(),
                },
                local_pins: def.pins.iter().map(|p| p.at).collect(),
                local_bounds: def.bounds,
                def: omnia_doc::PlaceDef::Unmodelled,
                params: omnia_doc::Params::new(),
            })
            .unwrap();
        }
        if with_text {
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: -11 * GRID_NM,
                    y: -3 * GRID_NM,
                },
                text: if with_emoji { "😀VIN" } else { "VIN" }.into(),
            }))
            .unwrap();
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: 8 * GRID_NM,
                    y: 0,
                },
                text: "VOUT".into(),
            }))
            .unwrap();
        }
        doc
    }

    fn build_net_only_doc() -> Doc {
        let mut doc = Doc::new();
        doc.apply(Command::AddLabel(Label {
            at: Pt::ZERO,
            text: "VCC".into(),
        }))
        .unwrap();
        doc
    }

    fn build_cap_doc() -> Doc {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let mut doc = Doc::new();
        // 400 parts inside the viewport -> 400 designators + 400 values + 800 pin numbers = 1,600
        // candidate labels; K9 caps painted labels at 700.
        for i in 0..400 {
            let at = Pt {
                x: (i % 20 - 10) * 2 * GRID_NM,
                y: (i / 20 - 10) * 2 * GRID_NM,
            };
            doc.apply(Command::PlacePart {
                part: Part {
                    symbol: resistor.id,
                    at,
                    orient: Orient::IDENTITY,
                    designator: format!("R{i:03}").into(),
                    value: format!("{i:03}k").into(),
                },
                local_pins: resistor.pins.iter().map(|p| p.at).collect(),
                local_bounds: resistor.bounds,
                def: omnia_doc::PlaceDef::Unmodelled,
                params: omnia_doc::Params::new(),
            })
            .unwrap();
        }
        doc
    }

    fn diff_pixels(a: &[u8], b: &[u8]) -> usize {
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .filter(|(pa, pb)| pa != pb)
            .count()
    }

    pub fn main() {
        if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
            eprintln!(
                "sheet_text_render skipped: OMNIA_HEADLESS_CI is set (no interactive desktop)"
            );
            return;
        }
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(120));
            eprintln!("sheet_text_render did not finish within 120 s");
            std::process::exit(3);
        });

        let outcome = Rc::new(RefCell::new(None::<Result<(), String>>));
        let slot = outcome.clone();
        gpui_platform::application().run(move |cx: &mut App| {
            if let Err(err) = load_bundled_inter(cx) {
                *slot.borrow_mut() = Some(Err(err));
                cx.quit();
                return;
            }
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0)));
            let sample_doc = SheetDocument::new(cx.new(|_| build_sample_doc(true, true)));
            let blank_doc = SheetDocument::new(cx.new(|_| build_sample_doc(false, false)));
            let net_only_doc = SheetDocument::new(cx.new(|_| build_net_only_doc()));
            let cap_doc = SheetDocument::new(cx.new(|_| build_cap_doc()));
            let stress_doc = SheetDocument::new(cx.new(|_| canonical_text_stress_doc()));

            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| Probe {
                        camera: Camera::new(Pt::ZERO, 1.0, bounds),
                        document: sample_doc.clone(),
                    })
                },
            );
            let win: WindowHandle<Probe> = match opened {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open hidden window: {e}")));
                    cx.quit();
                    return;
                }
            };

            cx.spawn(async move |cx| {
                let result = cx.update(|cx| -> Result<(), String> {
                    let render_shot = |cx: &mut App,
                                       doc: &SheetDocument,
                                       zoom: f64,
                                       pan: (f64, f64),
                                       sf: f32|
                     -> Result<(Vec<u8>, usize), String> {
                        win.update(cx, |probe, window, cx| {
                            let viewport =
                                Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                            let mut camera = Camera::new(Pt::ZERO, zoom, viewport);
                            camera.pan_by(point(pan.0, pan.1));
                            probe.camera = camera;
                            probe.document = doc.clone();
                            cx.notify();
                        })
                        .map_err(|e| format!("update probe: {e}"))?;
                        cx.update_window(win.into(), |_, window, cx| {
                            window.set_scale_factor(sf);
                            window.draw(cx).clear(cx);
                            let text_sprites =
                                glyph_and_emoji_sprites(window.rendered_scene()).len();
                            let image = window.render_to_image().map_err(|e| e.to_string())?;
                            Ok::<_, String>((image.into_raw(), text_sprites))
                        })
                        .map_err(|e| format!("update window: {e}"))?
                    };

                    // 1. B10 real-backend zero-allocation proof on canonical `text` stress sheet:
                    //    1,000 parts, 2,000 wires, 0 net labels, 12,000 chars, real DirectWrite
                    //    glyph rasterisation & atlas lookup (non-zero text-layer glyph sprites).
                    let draw_measured = |cx: &mut App,
                                         doc: &SheetDocument,
                                         zoom: f64,
                                         pan: (f64, f64)|
                     -> Result<(u64, usize), String> {
                        win.update(cx, |probe, window, cx| {
                            let viewport =
                                Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                            let mut camera = Camera::new(Pt::ZERO, zoom, viewport);
                            camera.pan_by(point(pan.0, pan.1));
                            probe.camera = camera;
                            probe.document = doc.clone();
                            cx.notify();
                        })
                        .map_err(|e| format!("stress update: {e}"))?;
                        cx.update_window(win.into(), |_, window, cx| {
                            window.set_scale_factor(1.0);
                            ALLOCS.with(|n| n.set(0));
                            OURS.with(|c| c.set(false));
                            window.draw(cx).clear(cx);
                            let allocs = ALLOCS.with(Cell::get);
                            let text_sprites =
                                glyph_and_emoji_sprites(window.rendered_scene()).len();
                            (allocs, text_sprites)
                        })
                        .map_err(|e| format!("stress draw: {e}"))
                    };

                    // Warm frame 0 at 100% zoom, then 60 fixed-zoom pan frames.
                    let (_, warm_sprites) = draw_measured(cx, &stress_doc, 1.0, (0.0, 0.0))?;
                    if warm_sprites == 0 {
                        return Err(
                            "canonical text stress sheet emitted 0 glyph sprites on real backend"
                                .into(),
                        );
                    }
                    let mut min_pan_sprites = usize::MAX;
                    for frame in 0..60 {
                        let (allocs, text_sprites) = draw_measured(
                            cx,
                            &stress_doc,
                            1.0,
                            (-f64::from(frame) * 3.5, f64::from(frame % 7) * 0.5),
                        )?;
                        min_pan_sprites = min_pan_sprites.min(text_sprites);
                        if allocs != 0 {
                            return Err(format!(
                                "real-backend canonical text pan frame {frame} allocated {allocs} times"
                            ));
                        }
                        if text_sprites == 0 {
                            return Err(format!(
                                "real-backend canonical text pan frame {frame} emitted 0 glyph sprites"
                            ));
                        }
                    }
                    // Warm query/merged-wire scratch at MIN_ZOOM and junction scratch just above
                    // K9's 2.5 px junction threshold (0.3125x), then run a 30-step real-backend
                    // zoom sweep asserting 0 Omnia allocations on every frame.
                    let _ = draw_measured(cx, &stress_doc, omnia_canvas::camera::MIN_ZOOM, (0.0, 0.0))?;
                    let _ = draw_measured(cx, &stress_doc, 0.3125, (0.0, 0.0))?;
                    let mut zoom_text_frames = 0usize;
                    let mut min_zoom_text_sprites = usize::MAX;
                    for step in 0..30 {
                        let t = f64::from(step) / 29.0;
                        let zoom = (omnia_canvas::camera::MIN_ZOOM.log2()
                            + t * (omnia_canvas::camera::MAX_ZOOM.log2()
                                - omnia_canvas::camera::MIN_ZOOM.log2()))
                        .exp2();
                        let (allocs, text_sprites) = draw_measured(cx, &stress_doc, zoom, (0.0, 0.0))?;
                        if allocs != 0 {
                            return Err(format!(
                                "real-backend canonical text zoom step {step} allocated {allocs} times"
                            ));
                        }
                        if text_sprites > 0 {
                            zoom_text_frames += 1;
                            min_zoom_text_sprites = min_zoom_text_sprites.min(text_sprites);
                        }
                    }
                    if zoom_text_frames < 10 {
                        return Err(format!(
                            "zoom sweep exercised too few text-visible frames ({zoom_text_frames})"
                        ));
                    }
                    println!(
                        "  real-backend canonical text B10: 0 Omnia allocs/frame across 60 pan (min {min_pan_sprites} glyph sprites/frame) + 30 zoom frames ({zoom_text_frames} text-visible, min {min_zoom_text_sprites} glyph sprites/frame)"
                    );

                    // 2. Visual sanity across scale factors 1.0, 1.25, 1.5.
                    for sf in [1.0f32, 1.25, 1.5] {
                        let (with_labels, sprites_with) =
                            render_shot(cx, &sample_doc, 1.0, (0.0, 0.0), sf)?;
                        let (without_text, sprites_without) =
                            render_shot(cx, &blank_doc, 1.0, (0.0, 0.0), sf)?;
                        let changed = diff_pixels(&with_labels, &without_text);
                        if changed == 0 || sprites_with <= sprites_without {
                            return Err(format!(
                                "scale {sf}: labels drew no extra ink (diff={changed}, text sprites {sprites_with} vs {sprites_without})"
                            ));
                        }
                    }

                    // 3. K9 / D5 drop order on both sides of the ordinary-text (6.5 px) and
                    //    pin-number/junction (2.5 px = 31.25%) boundaries:
                    //    - At 0.45x: net labels (13 * 0.45 = 5.85 px) hide, while designators
                    //      (17 * 0.45 = 7.65 px) and pin numbers remain visible.
                    let (_, net_045) = render_shot(cx, &net_only_doc, 0.45, (0.0, 0.0), 1.0)?;
                    if net_045 != 0 {
                        return Err(format!(
                            "zoom 0.45: expected 13 px net label hidden (< 6.5 px), got {net_045} sprites"
                        ));
                    }
                    let (at_045, sprites_045) =
                        render_shot(cx, &sample_doc, 0.45, (0.0, 0.0), 1.0)?;
                    let (blank_045, _) = render_shot(cx, &blank_doc, 0.45, (0.0, 0.0), 1.0)?;
                    if diff_pixels(&at_045, &blank_045) == 0 || sprites_045 == 0 {
                        return Err(format!(
                            "zoom 0.45: expected designators/values visible (sprites={sprites_045})"
                        ));
                    }

                    //    - At 0.35x: ordinary text (17 * 0.35 = 5.95 px < 6.5 px) is hidden
                    //      (`sample_doc` matches `blank_doc` pixel-for-pixel), WHILE pin numbers
                    //      remain visible above K9's 2.5 px pin/junction threshold (`text_sprites > 0`).
                    let (at_035, pins_035) =
                        render_shot(cx, &sample_doc, 0.35, (0.0, 0.0), 1.0)?;
                    let (blank_035, blank_pins_035) =
                        render_shot(cx, &blank_doc, 0.35, (0.0, 0.0), 1.0)?;
                    if diff_pixels(&at_035, &blank_035) != 0 {
                        return Err(format!(
                            "zoom 0.35: expected ordinary text hidden (< 6.5 px), got {} differing pixels",
                            diff_pixels(&at_035, &blank_035)
                        ));
                    }
                    if pins_035 == 0 || blank_pins_035 == 0 {
                        return Err(format!(
                            "zoom 0.35: expected pin numbers to stay visible above 2.5 px threshold (pins_035={pins_035}, blank_pins_035={blank_pins_035})"
                        ));
                    }

                    //    - At 0.30x (< 31.25%): junction dots and pin numbers drop together -> 0 text
                    //      layer sprites.
                    let (_, pins_030) = render_shot(cx, &sample_doc, 0.30, (0.0, 0.0), 1.0)?;
                    let (_, blank_pins_030) =
                        render_shot(cx, &blank_doc, 0.30, (0.0, 0.0), 1.0)?;
                    if pins_030 != 0 || blank_pins_030 != 0 {
                        return Err(format!(
                            "zoom 0.30: expected pin numbers hidden below 2.5 px threshold (got {pins_030}, {blank_pins_030})"
                        ));
                    }

                    // 4. K9 visible-label cap: 400 parts in view (1,600 candidate labels, each
                    //    designator/value 4 chars). At most 700 labels paint -> at most 700 * 4 =
                    //    2,800 text-layer glyph sprites (never all 1,600 labels).
                    let (_, cap_text_sprites) =
                        render_shot(cx, &cap_doc, 0.55, (0.0, 0.0), 1.0)?;
                    if cap_text_sprites == 0 || cap_text_sprites > 700 * 4 {
                        return Err(format!(
                            "K9 label cap: expected text-layer sprites in (0, 2800], got {cap_text_sprites}"
                        ));
                    }

                    // 5. T3.1 done-when: isolated glyph + emoji atlas stability across two full
                    //    K4 zoom sweeps (excluding SVG symbol & dot tiles).
                    let sweep_steps = 80usize;
                    let zooms: Vec<f64> = (0..sweep_steps)
                        .map(|i| {
                            let half = sweep_steps / 2;
                            let t = if i < half {
                                i as f64 / half as f64
                            } else {
                                (sweep_steps - i) as f64 / half as f64
                            };
                            let lo = omnia_canvas::camera::MIN_ZOOM.log2();
                            let hi = omnia_canvas::camera::MAX_ZOOM.log2();
                            (lo + t * (hi - lo)).exp2()
                        })
                        .collect();

                    let mut run_pass =
                        |stats: &mut GlyphAtlasStats| -> Result<(usize, BTreeSet<i32>), String> {
                            let mut emitted_steps = 0usize;
                            let mut frame_max_glyph_heights = BTreeSet::new();
                            for &zoom in &zooms {
                                win.update(cx, |probe, window, cx| {
                                    let viewport = Bounds::new(
                                        point(px(0.0), px(0.0)),
                                        window.viewport_size(),
                                    );
                                    probe.camera = Camera::new(Pt::ZERO, zoom, viewport);
                                    probe.document = sample_doc.clone();
                                    cx.notify();
                                })
                                .map_err(|e| format!("sweep update: {e}"))?;
                                let (step_sprites, max_glyph_h) = cx
                                    .update_window(win.into(), |_, window, cx| {
                                        window.set_scale_factor(1.0);
                                        window.draw(cx).clear(cx);
                                        stats.record_text_layer(window)
                                    })
                                    .map_err(|e| format!("sweep draw: {e}"))?;
                                if step_sprites > 0 {
                                    emitted_steps += 1;
                                    // Record the actual emitted peak glyph raster height for this
                                    // frame (cannot be spoofed by a single-size renderer).
                                    if let Some(h) = max_glyph_h {
                                        frame_max_glyph_heights.insert(h);
                                    }
                                }
                            }
                            Ok((emitted_steps, frame_max_glyph_heights))
                        };

                    let check_pass = |label: &str,
                                      pass: &GlyphAtlasStats,
                                      steps: usize,
                                      heights: &BTreeSet<i32>|
                     -> Result<(), String> {
                        let min_h = heights.first().copied().unwrap_or(0);
                        let max_h = heights.last().copied().unwrap_or(0);
                        if steps < 2
                            || heights.len() < 10
                            || min_h <= 0
                            || max_h < min_h * 4
                            || pass.tiles.is_empty()
                            || pass.emoji_tiles.is_empty()
                        {
                            return Err(format!(
                                "zoom sweep {label} did not emit scaled glyphs across multiple \
                                 actual raster sizes + emoji (steps={steps}, distinct_max_heights={}, \
                                 height_range={min_h}..={max_h} px, tiles={}, emoji={})",
                                heights.len(),
                                pass.tiles.len(),
                                pass.emoji_tiles.len()
                            ));
                        }
                        Ok(())
                    };

                    let mut pass1 = GlyphAtlasStats::default();
                    let (emitted_pass1, heights_pass1) = run_pass(&mut pass1)?;
                    check_pass("pass 1", &pass1, emitted_pass1, &heights_pass1)?;

                    // Independent second pass of the exact same zoom sweep: must independently
                    // emit scaled glyph raster sizes + emoji, and its tile set, emoji set, page
                    // count, and bytes must equal pass 1 exactly (zero new tiles on pass 2).
                    let mut pass2 = GlyphAtlasStats::default();
                    let (emitted_pass2, heights_pass2) = run_pass(&mut pass2)?;
                    check_pass("pass 2", &pass2, emitted_pass2, &heights_pass2)?;

                    println!(
                        "  zoom sweep glyph+emoji atlas: pass1 = {} pages / {} bytes / {} tiles ({} emitted peak glyph heights {}..={} px, {} emoji tiles); \
                         pass2 = {} pages / {} bytes / {} tiles ({} emitted peak glyph heights {}..={} px, {} emoji tiles)",
                        pass1.pages.len(),
                        pass1.bytes(),
                        pass1.tiles.len(),
                        heights_pass1.len(),
                        heights_pass1.first().unwrap(),
                        heights_pass1.last().unwrap(),
                        pass1.emoji_tiles.len(),
                        pass2.pages.len(),
                        pass2.bytes(),
                        pass2.tiles.len(),
                        heights_pass2.len(),
                        heights_pass2.first().unwrap(),
                        heights_pass2.last().unwrap(),
                        pass2.emoji_tiles.len(),
                    );
                    if pass1.tiles != pass2.tiles
                        || pass1.emoji_tiles != pass2.emoji_tiles
                        || heights_pass1 != heights_pass2
                        || (pass1.pages.len(), pass1.bytes()) != (pass2.pages.len(), pass2.bytes())
                    {
                        return Err(format!(
                            "glyph+emoji atlas differed between zoom sweeps: pass1=({} pages, {} B, {} tiles, {} emoji) \
                             vs pass2=({} pages, {} B, {} tiles, {} emoji)",
                            pass1.pages.len(),
                            pass1.bytes(),
                            pass1.tiles.len(),
                            pass1.emoji_tiles.len(),
                            pass2.pages.len(),
                            pass2.bytes(),
                            pass2.tiles.len(),
                            pass2.emoji_tiles.len(),
                        ));
                    }

                    // Fractional pan at fixed zoom allocates zero new glyph/emoji tiles.
                    let mut pan_stats = GlyphAtlasStats::default();
                    let mut pan_step =
                        |stats: &mut GlyphAtlasStats, pan: (f64, f64)| -> Result<usize, String> {
                            win.update(cx, |probe, window, cx| {
                                let viewport =
                                    Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                                let mut camera = Camera::new(Pt::ZERO, 1.0, viewport);
                                camera.pan_by(point(pan.0, pan.1));
                                probe.camera = camera;
                                probe.document = sample_doc.clone();
                                cx.notify();
                            })
                            .map_err(|e| format!("pan update: {e}"))?;
                            cx.update_window(win.into(), |_, window, cx| {
                                window.set_scale_factor(1.0);
                                window.draw(cx).clear(cx);
                                stats.record_text_layer(window).0
                            })
                            .map_err(|e| format!("pan draw: {e}"))
                        };
                    let pan_sprites = pan_step(&mut pan_stats, (0.0, 0.0))?;
                    if pan_sprites == 0 {
                        return Err("fractional pan baseline emitted 0 glyph sprites".into());
                    }
                    let pan_tiles_before = pan_stats.tiles.len();
                    for pan in [(0.25, -0.15), (0.6, 0.4), (-1.35, 2.75)] {
                        pan_step(&mut pan_stats, pan)?;
                    }
                    if pan_stats.tiles.len() != pan_tiles_before {
                        return Err(format!(
                            "fractional pan allocated new glyph tiles: {pan_tiles_before} -> {}",
                            pan_stats.tiles.len()
                        ));
                    }
                    Ok(())
                });
                *slot.borrow_mut() = Some(result);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });

        let result = outcome
            .borrow_mut()
            .take()
            .expect("render task did not run before the application exited");
        match result {
            Ok(()) => println!("test sheet_text_labels_lod_cap_and_atlas_stability ... ok"),
            Err(e) => panic!("{e}"),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    render::main();
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    println!(
        "test sheet_text_labels_lod_cap_and_atlas_stability ... skipped: render_to_image \
         unsupported on this platform"
    );
}
