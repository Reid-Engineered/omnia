//! T7.4: Windows hidden-window verification (`render_to_image` + `rendered_scene` + counting
//! allocator) on the real DirectX/DirectWrite backend that a mid-run scale-factor change from
//! `1.25` to `1.5` renders a frame pixel-for-pixel identical to a fresh window opened at `1.5`,
//! including grid dots, wires, symbol/junction sprites, text, and `OverlayView` (`.cached()`),
//! while preserving `B10` zero-allocation steady-state painting at `1.5`.
//!
//! Skips under `OMNIA_HEADLESS_CI` and on Linux/FreeBSD (no `render_to_image`, T0.8).

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod windows {
    use gpui::{
        App, AppContext, Bounds, Context, Element, ElementId, Entity, GlobalElementId,
        InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, Render, Scene, Styled,
        TransformationMatrix, Window, WindowBounds, WindowHandle, WindowOptions, div, point, px,
        size,
    };
    use omnia_canvas::{
        Camera, OverlayView, SheetView,
        sheet::{SheetDocument, SheetPaintScope},
    };
    use omnia_doc::{Command, Doc, GRID_NM, ItemId, Label, Orient, Part, Pt, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        borrow::Cow,
        cell::Cell,
        rc::Rc,
        time::Duration,
    };

    const CONTENT_W: f32 = 640.0;
    const CONTENT_H: f32 = 400.0;
    const WINDOW_W: f32 = 1024.0;
    const WINDOW_H: f32 = 768.0;
    const INVERTING_AMP_RON: &str = include_str!("../../../tests/golden/inverting-amp.ron");

    // Allocator wrapper isolating Omnia paint work (`TestScope::omnia`) from GPUI work (B10).
    thread_local! {
        static IN_OMNIA: Cell<bool> = const { Cell::new(false) };
        static OMNIA_ALLOCS: Cell<Option<usize>> = const { Cell::new(None) };
        static SHEET_PAINTS: Cell<usize> = const { Cell::new(0) };
    }

    struct Counting;

    #[global_allocator]
    static ALLOC: Counting = Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let _ = IN_OMNIA.try_with(|inside| {
                if inside.get() {
                    let _ = OMNIA_ALLOCS.try_with(|counter| {
                        if let Some(count) = counter.get() {
                            counter.set(Some(count + 1));
                        }
                    });
                }
            });
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    struct TestScope;

    impl SheetPaintScope for TestScope {
        fn omnia(&self, paint: impl FnOnce()) {
            let prev = IN_OMNIA.with(|flag| flag.replace(true));
            paint();
            IN_OMNIA.with(|flag| flag.set(prev));
        }
    }

    #[derive(Clone)]
    struct SceneCounts {
        quads: usize,
        symbol_sprites: usize,
        text_sprites: usize,
    }

    fn count_scene(scene: &Scene) -> SceneCounts {
        let mut text_1to1 = 0usize;
        for sprite in &scene.monochrome_sprites {
            if sprite.bounds.size.width.0 == sprite.tile.bounds.size.width.0 as f32
                && sprite.bounds.size.height.0 == sprite.tile.bounds.size.height.0 as f32
                && sprite.transformation == TransformationMatrix::unit()
            {
                text_1to1 += 1;
            }
        }
        SceneCounts {
            quads: scene.quads.len(),
            symbol_sprites: scene.monochrome_sprites.len() - text_1to1,
            text_sprites: text_1to1 + scene.subpixel_sprites.len() + scene.polychrome_sprites.len(),
        }
    }

    struct SheetElement {
        camera: Camera,
        document: SheetDocument,
    }

    impl Element for SheetElement {
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
            let mut style = gpui::Style::default();
            style.size.width = px(CONTENT_W).into();
            style.size.height = px(CONTENT_H).into();
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
            SHEET_PAINTS.with(|paints| paints.set(paints.get() + 1));
            SheetView::with_document(self.camera.clone(), &GRAPHITE, self.document.clone())
                .paint_with_scope(bounds, window, cx, &TestScope);
        }
    }

    struct SheetProbe {
        camera: Camera,
        document: SheetDocument,
    }

    impl Render for SheetProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(CONTENT_W))
                .h(px(CONTENT_H))
                .overflow_hidden()
                .child(SheetElement {
                    camera: self.camera.clone(),
                    document: self.document.clone(),
                })
        }
    }

    struct OverlayHost {
        overlay: Entity<OverlayView>,
    }

    impl Render for OverlayHost {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(CONTENT_W))
                .h(px(CONTENT_H))
                .overflow_hidden()
                .child(self.overlay.clone())
        }
    }

    type CroppedRgba = (u32, u32, Vec<u8>);

    fn crop_content(
        raw: &[u8],
        stride: u32,
        height: u32,
        sf: f32,
        label: &str,
    ) -> Result<CroppedRgba, String> {
        let crop_w = (f64::from(CONTENT_W) * f64::from(sf)).round() as u32;
        let crop_h = (f64::from(CONTENT_H) * f64::from(sf)).round() as u32;
        if stride < crop_w || height < crop_h {
            return Err(format!(
                "{label}: backbuffer {stride}x{height} smaller than expected {crop_w}x{crop_h}"
            ));
        }
        let row_bytes = (crop_w as usize) * 4;
        let stride_bytes = (stride as usize) * 4;
        let mut out = Vec::with_capacity(row_bytes * (crop_h as usize));
        for y in 0..crop_h as usize {
            let start = y * stride_bytes;
            out.extend_from_slice(&raw[start..start + row_bytes]);
        }
        Ok((crop_w, crop_h, out))
    }

    fn assert_images_identical(
        a: &CroppedRgba,
        b: &CroppedRgba,
        label: &str,
    ) -> Result<(), String> {
        if a.0 != b.0 || a.1 != b.1 {
            return Err(format!(
                "{label}: dimensions differ ({}x{} vs {}x{})",
                a.0, a.1, b.0, b.1
            ));
        }
        let mut differing = 0usize;
        let mut max_delta = 0u8;
        let mut first_diff = None;
        for (idx, (pa, pb)) in
            a.2.as_chunks::<4>()
                .0
                .iter()
                .zip(b.2.as_chunks::<4>().0.iter())
                .enumerate()
        {
            if pa != pb {
                differing += 1;
                for c in 0..4 {
                    max_delta = max_delta.max(pa[c].abs_diff(pb[c]));
                }
                if first_diff.is_none() {
                    let x = idx as u32 % a.0;
                    let y = idx as u32 / a.0;
                    first_diff = Some((x, y, *pa, *pb));
                }
            }
        }
        if differing > 0 {
            return Err(format!(
                "{label}: {differing} differing pixels (max channel delta {max_delta}); first at {:?} (migrated vs fresh)",
                first_diff
            ));
        }
        Ok(())
    }

    fn register_inter_fonts(cx: &App) -> Result<(), String> {
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice(),
                ),
                Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice(),
                ),
            ])
            .map_err(|e| format!("register Inter fonts: {e}"))
    }

    fn hidden_window_options() -> WindowOptions {
        let size_px = size(px(WINDOW_W), px(WINDOW_H));
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.0), px(0.0)),
                size_px,
            ))),
            window_min_size: Some(size_px),
            show: false,
            focus: false,
            ..Default::default()
        }
    }

    fn build_dense_text_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let def_id = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(def_id);
        let pins: Vec<Pt> = def.pins.iter().map(|p| p.at).collect();
        let g = GRID_NM;

        let mut doc = Doc::new();
        for idx in 0..16i32 {
            let col = idx % 4;
            let row = idx / 4;
            let x = (col - 2) * 8 * g;
            let y = (row - 2) * 6 * g;
            doc.apply(Command::PlacePart {
                part: Part {
                    symbol: def_id,
                    at: Pt { x, y },
                    orient: Orient::IDENTITY,
                    designator: format!("R{}", idx + 1).into(),
                    value: "10k".into(),
                },
                local_pins: pins.clone(),
                local_bounds: def.bounds,
            })
            .unwrap();
            doc.apply(Command::AddWires(vec![
                Wire {
                    a: Pt { x: x - 2 * g, y },
                    b: Pt { x: x + 2 * g, y },
                },
                Wire {
                    a: Pt { x, y },
                    b: Pt { x, y: y + 2 * g },
                },
            ]))
            .unwrap();
            doc.apply(Command::AddLabel(Label {
                at: Pt { x, y: y - 2 * g },
                text: format!("NET{}", idx + 1).into(),
            }))
            .unwrap();
        }
        doc
    }

    fn load_golden_doc() -> Result<Doc, String> {
        Doc::load(INVERTING_AMP_RON.as_bytes())
            .map_err(|e| format!("load inverting-amp.ron: {e:?}"))
    }

    fn camera_at(zoom: f64, pan_px: (f64, f64)) -> Camera {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(CONTENT_W), px(CONTENT_H)));
        let mut cam = Camera::new(Pt::ZERO, zoom, bounds);
        cam.pan_by(point(pan_px.0, pan_px.1));
        cam
    }

    fn draw_sheet_at(
        win: WindowHandle<SheetProbe>,
        cx: &mut App,
        sf: f32,
        zoom: f64,
        pan: (f64, f64),
        label: &str,
    ) -> Result<(CroppedRgba, SceneCounts), String> {
        let cam = camera_at(zoom, pan);
        win.update(cx, |probe, _window, cx| {
            probe.camera = cam;
            cx.notify();
        })
        .map_err(|e| format!("{label}: update probe: {e}"))?;
        let (image, counts) = cx
            .update_window(win.into(), |_, window, cx| {
                window.set_scale_factor(sf);
                window.draw(cx).clear(cx);
                assert_eq!(window.scale_factor(), sf, "{label}: scale factor reset");
                let counts = count_scene(window.rendered_scene());
                let image = window.render_to_image().map_err(|e| e.to_string());
                (image, counts)
            })
            .map_err(|e| format!("{label}: draw: {e}"))?;
        let image = image.map_err(|e| format!("{label}: render_to_image: {e}"))?;
        let cropped = crop_content(image.as_raw(), image.width(), image.height(), sf, label)?;
        Ok((cropped, counts))
    }

    pub(super) fn run() {
        if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
            println!("test scale_factor_migration ... skipped: headless CI runner");
            return;
        }

        // Rule 9: abort with a clear failure if any window call stalls.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(120));
            eprintln!("scale_factor test timed out after 120s");
            std::process::exit(1);
        });

        let result: Rc<std::cell::RefCell<Option<Result<(), String>>>> =
            Rc::new(std::cell::RefCell::new(None));
        let slot = result.clone();

        gpui_platform::application().run(move |cx| {
            if let Err(err) = register_inter_fonts(cx) {
                *slot.borrow_mut() = Some(Err(err));
                cx.quit();
                return;
            }
            let golden_doc = match load_golden_doc() {
                Ok(doc) => doc,
                Err(err) => {
                    *slot.borrow_mut() = Some(Err(err));
                    cx.quit();
                    return;
                }
            };
            let Some(first_part) = golden_doc.parts().next().map(|(id, _)| ItemId::Part(id))
            else {
                *slot.borrow_mut() = Some(Err("inverting-amp.ron has no parts".to_string()));
                cx.quit();
                return;
            };
            let Some(first_wire) = golden_doc.wires().next().map(|(id, _)| ItemId::Wire(id))
            else {
                *slot.borrow_mut() = Some(Err("inverting-amp.ron has no wires".to_string()));
                cx.quit();
                return;
            };

            let fresh_golden = match load_golden_doc() {
                Ok(doc) => doc,
                Err(err) => {
                    *slot.borrow_mut() = Some(Err(err));
                    cx.quit();
                    return;
                }
            };
            let migrated_overlay_golden = match load_golden_doc() {
                Ok(doc) => doc,
                Err(err) => {
                    *slot.borrow_mut() = Some(Err(err));
                    cx.quit();
                    return;
                }
            };
            let fresh_overlay_golden = match load_golden_doc() {
                Ok(doc) => doc,
                Err(err) => {
                    *slot.borrow_mut() = Some(Err(err));
                    cx.quit();
                    return;
                }
            };

            let sheet_doc_entity = cx.new(|_| golden_doc);
            let migrated_sheet = match cx.open_window(hidden_window_options(), |window, cx| {
                window.resize(size(px(WINDOW_W), px(WINDOW_H)));
                cx.new(|_| SheetProbe {
                    camera: camera_at(1.0, (0.3, 0.7)),
                    document: SheetDocument::new(sheet_doc_entity.clone()),
                })
            }) {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open migrated sheet window: {e}")));
                    cx.quit();
                    return;
                }
            };

            let fresh_doc_entity = cx.new(|_| fresh_golden);
            let fresh_sheet = match cx.open_window(hidden_window_options(), |window, cx| {
                window.resize(size(px(WINDOW_W), px(WINDOW_H)));
                cx.new(|_| SheetProbe {
                    camera: camera_at(1.0, (0.3, 0.7)),
                    document: SheetDocument::new(fresh_doc_entity),
                })
            }) {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open fresh sheet window: {e}")));
                    cx.quit();
                    return;
                }
            };

            let migrated_overlay_doc = cx.new(|_| migrated_overlay_golden);
            let migrated_overlay = match cx.open_window(hidden_window_options(), |window, cx| {
                window.resize(size(px(WINDOW_W), px(WINDOW_H)));
                let doc_ref = migrated_overlay_doc.clone();
                let overlay = cx.new(|cx| {
                    let mut view =
                        OverlayView::new(doc_ref, camera_at(1.0, (0.3, 0.7)), &GRAPHITE, cx);
                    view.set_selection([first_part], cx);
                    view.set_hovered(Some(first_wire), cx);
                    view
                });
                let focus = overlay.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
                cx.new(|_| OverlayHost { overlay })
            }) {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open migrated overlay window: {e}")));
                    cx.quit();
                    return;
                }
            };

            let fresh_overlay_doc = cx.new(|_| fresh_overlay_golden);
            let fresh_overlay = match cx.open_window(hidden_window_options(), |window, cx| {
                window.resize(size(px(WINDOW_W), px(WINDOW_H)));
                let overlay = cx.new(|cx| {
                    let mut view = OverlayView::new(
                        fresh_overlay_doc,
                        camera_at(1.0, (0.3, 0.7)),
                        &GRAPHITE,
                        cx,
                    );
                    view.set_selection([first_part], cx);
                    view.set_hovered(Some(first_wire), cx);
                    view
                });
                let focus = overlay.read(cx).focus_handle().clone();
                window.focus(&focus, cx);
                cx.new(|_| OverlayHost { overlay })
            }) {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open fresh overlay window: {e}")));
                    cx.quit();
                    return;
                }
            };

            let dense_doc = cx.new(|_| build_dense_text_doc());
            let b10_win = match cx.open_window(hidden_window_options(), |window, cx| {
                window.resize(size(px(WINDOW_W), px(WINDOW_H)));
                cx.new(|_| SheetProbe {
                    camera: camera_at(1.0, (0.0, 0.0)),
                    document: SheetDocument::new(dense_doc),
                })
            }) {
                Ok(win) => win,
                Err(e) => {
                    *slot.borrow_mut() = Some(Err(format!("open B10 window: {e}")));
                    cx.quit();
                    return;
                }
            };

            // Run checks on a spawned task so all pending `window.resize` Win32 `WM_SIZE`
            // messages settle before `render_to_image` reads the DirectX swap chain.
            cx.spawn(async move |cx| {
                let outcome = cx.update(|cx| -> Result<(), String> {
                    // 1. Mid-run 1.25 -> 1.5 migration on SheetView (near & far zoom) vs fresh 1.5 window.
                    let (_warm_near_125, near_125_counts) = draw_sheet_at(
                        migrated_sheet,
                        cx,
                        1.25,
                        1.0,
                        (0.3, 0.7),
                        "migrated near 1.25",
                    )?;
                    let (_warm_far_125, _) = draw_sheet_at(
                        migrated_sheet,
                        cx,
                        1.25,
                        0.2,
                        (0.3, 0.7),
                        "migrated far 1.25",
                    )?;

                    assert!(
                        near_125_counts.quads > 0
                            && near_125_counts.symbol_sprites > 0
                            && near_125_counts.text_sprites > 0,
                        "expected quads, symbol sprites, and text sprites at 1.25; got quads={}, symbols={}, text={}",
                        near_125_counts.quads,
                        near_125_counts.symbol_sprites,
                        near_125_counts.text_sprites
                    );

                    let (migrated_near_15, near_counts_15) = draw_sheet_at(
                        migrated_sheet,
                        cx,
                        1.5,
                        1.0,
                        (0.3, 0.7),
                        "migrated near 1.5",
                    )?;
                    let (migrated_far_15, far_counts_15) = draw_sheet_at(
                        migrated_sheet,
                        cx,
                        1.5,
                        0.2,
                        (0.3, 0.7),
                        "migrated far 1.5",
                    )?;

                    assert!(
                        near_counts_15.quads > 0
                            && near_counts_15.symbol_sprites > 0
                            && near_counts_15.text_sprites > 0,
                        "expected quads, symbol sprites, and text sprites at 1.5; got quads={}, symbols={}, text={}",
                        near_counts_15.quads,
                        near_counts_15.symbol_sprites,
                        near_counts_15.text_sprites
                    );
                    assert!(far_counts_15.quads > 0, "expected quads at far zoom 0.2");

                    let (fresh_near_15, _) =
                        draw_sheet_at(fresh_sheet, cx, 1.5, 1.0, (0.3, 0.7), "fresh near 1.5")?;
                    let (fresh_far_15, _) =
                        draw_sheet_at(fresh_sheet, cx, 1.5, 0.2, (0.3, 0.7), "fresh far 1.5")?;

                    assert_images_identical(
                        &migrated_near_15,
                        &fresh_near_15,
                        "sheet near zoom 1.0 (1.25->1.5 vs fresh 1.5)",
                    )?;
                    assert_images_identical(
                        &migrated_far_15,
                        &fresh_far_15,
                        "sheet far zoom 0.2 (1.25->1.5 vs fresh 1.5)",
                    )?;
                    let paper_px: [u8; 4] = migrated_near_15.2[0..4].try_into().unwrap();
                    assert!(
                        migrated_near_15.2.as_chunks::<4>().0.iter().any(|px| *px != paper_px),
                        "migrated near 1.5 frame must contain non-paper rendered content"
                    );

                    // 2. Mid-run 1.25 -> 1.5 migration on OverlayView (.cached() CachedSheet + selection + hover).
                    let overlay_entity = migrated_overlay
                        .read_with(cx, |host, _| host.overlay.clone())
                        .map_err(|e| format!("read migrated overlay entity: {e}"))?;

                    cx.update_window(migrated_overlay.into(), |_, window, cx| {
                        window.set_scale_factor(1.25);
                        window.draw(cx).clear(cx);
                        assert_eq!(window.scale_factor(), 1.25, "overlay initial scale factor");
                    })
                    .map_err(|e| format!("draw overlay at 1.25: {e}"))?;
                    let paints_125 = overlay_entity.read(cx).sheet_paint_count(cx);
                    assert!(paints_125 >= 1);

                    // Hover transitions keep CachedSheet cached (INV11).
                    overlay_entity.update(cx, |view, cx| view.set_hovered(None, cx));
                    cx.update_window(migrated_overlay.into(), |_, window, cx| {
                        window.draw(cx).clear(cx);
                    })
                    .map_err(|e| format!("hover off overlay at 1.25: {e}"))?;
                    overlay_entity.update(cx, |view, cx| view.set_hovered(Some(first_wire), cx));
                    cx.update_window(migrated_overlay.into(), |_, window, cx| {
                        window.draw(cx).clear(cx);
                    })
                    .map_err(|e| format!("hover on overlay at 1.25: {e}"))?;
                    assert_eq!(
                        overlay_entity.read(cx).sheet_paint_count(cx),
                        paints_125,
                        "INV11: overlay hover changes must not repaint CachedSheet"
                    );

                    // Switch 1.25 -> 1.5 via window.set_scale_factor without calling cx.notify().
                    let migrated_overlay_img = cx
                        .update_window(migrated_overlay.into(), |_, window, cx| {
                            window.set_scale_factor(1.5);
                            window.draw(cx).clear(cx);
                            assert_eq!(window.scale_factor(), 1.5, "overlay migrated scale factor");
                            window.render_to_image().map_err(|e| e.to_string())
                        })
                        .map_err(|e| format!("draw overlay after 1.25->1.5: {e}"))?
                        .map_err(|e| format!("migrated overlay render_to_image: {e}"))?;
                    let paints_15 = overlay_entity.read(cx).sheet_paint_count(cx);
                    assert_eq!(
                        paints_15,
                        paints_125 + 1,
                        "CachedSheet must repaint on scale_factor change 1.25 -> 1.5 without manual notify"
                    );
                    let migrated_overlay_crop = crop_content(
                        migrated_overlay_img.as_raw(),
                        migrated_overlay_img.width(),
                        migrated_overlay_img.height(),
                        1.5,
                        "migrated overlay 1.5",
                    )?;

                    // Subsequent hover at 1.5 still keeps CachedSheet cached (INV11).
                    overlay_entity.update(cx, |view, cx| view.set_hovered(None, cx));
                    cx.update_window(migrated_overlay.into(), |_, window, cx| {
                        window.draw(cx).clear(cx);
                    })
                    .map_err(|e| format!("hover overlay at 1.5: {e}"))?;
                    assert_eq!(
                        overlay_entity.read(cx).sheet_paint_count(cx),
                        paints_15,
                        "INV11: hover at 1.5 must not repaint CachedSheet"
                    );

                    let fresh_overlay_img = cx
                        .update_window(fresh_overlay.into(), |_, window, cx| {
                            window.set_scale_factor(1.5);
                            window.draw(cx).clear(cx);
                            assert_eq!(window.scale_factor(), 1.5, "fresh overlay scale factor");
                            window.render_to_image().map_err(|e| e.to_string())
                        })
                        .map_err(|e| format!("draw fresh overlay 1.5: {e}"))?
                        .map_err(|e| format!("fresh overlay render_to_image: {e}"))?;
                    let fresh_overlay_crop = crop_content(
                        fresh_overlay_img.as_raw(),
                        fresh_overlay_img.width(),
                        fresh_overlay_img.height(),
                        1.5,
                        "fresh overlay 1.5",
                    )?;
                    assert_images_identical(
                        &migrated_overlay_crop,
                        &fresh_overlay_crop,
                        "OverlayView 1.25->1.5 vs fresh 1.5",
                    )?;

                    // 3. B10 real-backend zero-allocation proof at 1.5 after switching from 1.25.
                    let _ = draw_sheet_at(b10_win, cx, 1.25, 1.0, (0.0, 0.0), "B10 warm 1.25")?;
                    let (_, b10_counts) =
                        draw_sheet_at(b10_win, cx, 1.5, 1.0, (0.5, 0.5), "B10 warm 1.5a")?;
                    assert!(
                        b10_counts.text_sprites > 0
                            && b10_counts.symbol_sprites > 0
                            && b10_counts.quads > 0,
                        "B10 sheet must emit text, symbol sprites, and quads"
                    );
                    let _ = draw_sheet_at(b10_win, cx, 1.5, 1.0, (1.0, 0.5), "B10 warm 1.5b")?;

                    SHEET_PAINTS.with(|paints| paints.set(0));
                    OMNIA_ALLOCS.set(Some(0));
                    for step in 0..20 {
                        let pan = (1.0 + f64::from(step) * 0.25, 0.5 + f64::from(step) * 0.15);
                        b10_win
                            .update(cx, |probe, _, cx| {
                                probe.camera = camera_at(1.0, pan);
                                cx.notify();
                            })
                            .map_err(|e| format!("B10 pan update: {e}"))?;
                        cx.update_window(b10_win.into(), |_, window, cx| {
                            assert_eq!(window.scale_factor(), 1.5, "B10 fixed scale factor");
                            window.draw(cx).clear(cx);
                        })
                        .map_err(|e| format!("B10 pan draw: {e}"))?;
                    }
                    let allocs = OMNIA_ALLOCS.replace(None).unwrap_or(usize::MAX);
                    assert_eq!(
                        SHEET_PAINTS.with(Cell::get),
                        20,
                        "B10 loop must actually paint all 20 frames"
                    );
                    assert_eq!(
                        allocs, 0,
                        "B10: warm Omnia frames at 1.5 after 1.25->1.5 transition must not heap-allocate (saw {allocs})"
                    );

                    Ok(())
                });

                *slot.borrow_mut() = Some(outcome);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });

        match result.borrow_mut().take() {
            Some(Ok(())) => {}
            Some(Err(err)) => panic!("{err}"),
            None => panic!("application exited before completing scale_factor test"),
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    println!(
        "test scale_factor_migration ... skipped: render_to_image unsupported on this platform"
    );
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    windows::run();
}
