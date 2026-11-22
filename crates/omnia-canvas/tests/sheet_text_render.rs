//! T3.1: Windows hidden-window verification (`render_to_image` + `rendered_scene`) for labels:
//! 1. Zoom sweep run twice leaves the glyph atlas the exact same size (same pages, bytes, and
//!    distinct tiles) after pass 2 as after pass 1 (T3.1 done-when, K11, G14).
//! 2. Visual sanity via `render_to_image` covering designators, values, net labels, pin numbers,
//!    K9 LOD hiding (net labels and pin numbers drop at 0.45x while designators remain; all text
//!    hides at 0.35x), and K9's 700-label cap.
//!
//! Skips under `OMNIA_HEADLESS_CI` and on Linux/FreeBSD (no `render_to_image`, T0.8), matching
//! `sheet_render.rs`.

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod render {
    use std::{
        borrow::Cow,
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        rc::Rc,
        time::Duration,
    };

    use gpui::{
        App, AppContext, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowHandle,
        WindowOptions, point, px, size,
    };
    use omnia_canvas::{Camera, SheetView, sheet::SheetDocument};
    use omnia_doc::{Command, Doc, GRID_NM, Label, Orient, Part, Pt};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;

    struct Probe {
        camera: Camera,
        document: SheetDocument,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            SheetView::with_document(self.camera.clone(), &GRAPHITE, self.document.clone())
        }
    }

    #[derive(Default)]
    struct AtlasStats {
        pages: BTreeMap<(u8, u32), (i32, i32)>,
        tiles: BTreeSet<(u8, u32, u32)>,
    }

    impl AtlasStats {
        fn record(&mut self, window: &Window) {
            let scene = window.rendered_scene();
            for tile in scene
                .monochrome_sprites
                .iter()
                .map(|s| s.tile)
                .chain(scene.subpixel_sprites.iter().map(|s| s.tile))
            {
                let kind = tile.texture_id.kind as u8;
                let tex = tile.texture_id.index;
                // Same page-size accounting rule as spike S5 (DirectX/wgpu min page 1024x1024).
                self.pages.entry((kind, tex)).or_insert((
                    1024.max(tile.bounds.size.width.0),
                    1024.max(tile.bounds.size.height.0),
                ));
                self.tiles.insert((kind, tex, tile.tile_id.0));
            }
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

    fn build_sample_doc(with_text: bool) -> Doc {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let opamp = library.get(library.by_name("IC-COM-OpAmp").unwrap());
        let cap = library.get(library.by_name("Capacitor-IEEE-NonPolarized").unwrap());
        let mut doc = Doc::new();
        for (def, at, orient, desig, val) in [
            (
                resistor,
                Pt {
                    x: -6 * GRID_NM,
                    y: 0,
                },
                Orient::IDENTITY,
                if with_text { "R1" } else { "" },
                if with_text { "10k" } else { "" },
            ),
            (
                opamp,
                Pt {
                    x: 4 * GRID_NM,
                    y: 0,
                },
                Orient::IDENTITY,
                if with_text { "U1" } else { "" },
                if with_text { "TL072" } else { "" },
            ),
            (
                cap,
                Pt {
                    x: -6 * GRID_NM,
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
            })
            .unwrap();
        }
        if with_text {
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: -11 * GRID_NM,
                    y: -3 * GRID_NM,
                },
                text: "VIN".into(),
            }))
            .unwrap();
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: 11 * GRID_NM,
                    y: 0,
                },
                text: "VOUT".into(),
            }))
            .unwrap();
        }
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
            std::thread::sleep(Duration::from_secs(90));
            eprintln!("sheet_text_render did not finish within 90 s");
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
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(400.0)));
            let sample_doc = SheetDocument::new(cx.new(|_| build_sample_doc(true)));
            let blank_doc = SheetDocument::new(cx.new(|_| build_sample_doc(false)));
            let cap_doc = SheetDocument::new(cx.new(|_| build_cap_doc()));

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
                     -> Result<(Vec<u8>, usize, usize), String> {
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
                            let scene = window.rendered_scene();
                            let sprites =
                                scene.monochrome_sprites.len() + scene.subpixel_sprites.len();
                            let quads = scene.quads.len();
                            let image = window.render_to_image().map_err(|e| e.to_string())?;
                            Ok::<_, String>((image.into_raw(), sprites, quads))
                        })
                        .map_err(|e| format!("update window: {e}"))?
                    };

                    // 1. Visual sanity: labels draw pixels at 1.0, 1.25, 1.5; K9 hides net labels
                    //    and pin numbers at 0.45x (while keeping 17 px designators/values), and
                    //    hides all text at 0.35x.
                    for sf in [1.0f32, 1.25, 1.5] {
                        let (with_labels, sprites_with, _) =
                            render_shot(cx, &sample_doc, 1.0, (0.0, 0.0), sf)?;
                        let (without_text, sprites_without, _) =
                            render_shot(cx, &blank_doc, 1.0, (0.0, 0.0), sf)?;
                        let changed = diff_pixels(&with_labels, &without_text);
                        if changed == 0 || sprites_with <= sprites_without {
                            return Err(format!(
                                "scale {sf}: labels drew no extra ink (diff={changed}, sprites {sprites_with} vs {sprites_without})"
                            ));
                        }
                    }

                    // At 0.45x: 17 * 0.45 = 7.65 px (visible), 13 * 0.45 = 5.85 px (hidden).
                    let (at_045, sprites_045, _) =
                        render_shot(cx, &sample_doc, 0.45, (0.0, 0.0), 1.0)?;
                    let (blank_045, sprites_blank_045, _) =
                        render_shot(cx, &blank_doc, 0.45, (0.0, 0.0), 1.0)?;
                    if diff_pixels(&at_045, &blank_045) == 0 || sprites_045 <= sprites_blank_045 {
                        return Err(format!(
                            "zoom 0.45: expected designators/values to remain visible (sprites {sprites_045} vs {sprites_blank_045})"
                        ));
                    }

                    // At 0.35x: 17 * 0.35 = 5.95 px < 6.5 px (all text hidden, including pin numbers).
                    let (at_035, sprites_035, _) =
                        render_shot(cx, &sample_doc, 0.35, (0.0, 0.0), 1.0)?;
                    let (blank_035, sprites_blank_035, _) =
                        render_shot(cx, &blank_doc, 0.35, (0.0, 0.0), 1.0)?;
                    if diff_pixels(&at_035, &blank_035) != 0 || sprites_035 != sprites_blank_035 {
                        return Err(format!(
                            "zoom 0.35: expected all text hidden below 6.5 px (diff={}, sprites {sprites_035} vs {sprites_blank_035})",
                            diff_pixels(&at_035, &blank_035)
                        ));
                    }

                    // 2. K9 visible-label cap: 400 parts in view (1,600 candidate labels, each
                    //    designator/value 4 chars). At most 700 labels paint -> at most
                    //    400 symbol sprites + 700 * 4 = 3,200 sprites (never all 1,600 labels).
                    let (_, cap_sprites, _) = render_shot(cx, &cap_doc, 0.55, (0.0, 0.0), 1.0)?;
                    if cap_sprites <= 400 || cap_sprites > 400 + 700 * 4 {
                        return Err(format!(
                            "K9 label cap: expected sprite count in (400, 3200], got {cap_sprites}"
                        ));
                    }

                    // 3. T3.1 done-when: a zoom sweep run twice leaves the glyph atlas the exact
                    //    same size after the second pass as after the first.
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

                    let mut stats = AtlasStats::default();
                    let mut run_pass = |stats: &mut AtlasStats, pan: (f64, f64)| -> Result<(), String> {
                        for &zoom in &zooms {
                            win.update(cx, |probe, window, cx| {
                                let viewport =
                                    Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                                let mut camera = Camera::new(Pt::ZERO, zoom, viewport);
                                camera.pan_by(point(pan.0, pan.1));
                                probe.camera = camera;
                                probe.document = sample_doc.clone();
                                cx.notify();
                            })
                            .map_err(|e| format!("sweep update: {e}"))?;
                            cx.update_window(win.into(), |_, window, cx| {
                                window.set_scale_factor(1.0);
                                window.draw(cx).clear(cx);
                                stats.record(window);
                            })
                            .map_err(|e| format!("sweep draw: {e}"))?;
                        }
                        Ok(())
                    };

                    run_pass(&mut stats, (0.0, 0.0))?;
                    let pass1_pages = stats.pages.len();
                    let pass1_bytes = stats.bytes();
                    let pass1_tiles = stats.tiles.len();
                    if pass1_pages == 0 || pass1_tiles == 0 {
                        return Err(format!(
                            "zoom sweep pass 1 populated no atlas entries (pages={pass1_pages}, tiles={pass1_tiles})"
                        ));
                    }

                    // Second pass of the exact same zoom sweep: pages, bytes, and distinct tiles
                    // must all be unchanged (T3.1 done-when).
                    run_pass(&mut stats, (0.0, 0.0))?;
                    let pass2_pages = stats.pages.len();
                    let pass2_bytes = stats.bytes();
                    let pass2_tiles = stats.tiles.len();
                    println!(
                        "  zoom sweep atlas: pass1 = {pass1_pages} pages / {pass1_bytes} bytes / {pass1_tiles} tiles; \
                         pass2 = {pass2_pages} pages / {pass2_bytes} bytes / {pass2_tiles} tiles"
                    );
                    if (pass1_pages, pass1_bytes, pass1_tiles)
                        != (pass2_pages, pass2_bytes, pass2_tiles)
                    {
                        return Err(format!(
                            "atlas grew on second zoom sweep: pass1=({pass1_pages} pages, {pass1_bytes} B, {pass1_tiles} tiles) \
                             vs pass2=({pass2_pages} pages, {pass2_bytes} B, {pass2_tiles} tiles)"
                        ));
                    }

                    // Also verify fractional pan at fixed zoom (with all labels well inside the
                    // viewport) allocates zero new glyph tiles thanks to device-pixel origin snap.
                    let mut pan_stats = AtlasStats::default();
                    let mut pan_step =
                        |stats: &mut AtlasStats, pan: (f64, f64)| -> Result<(), String> {
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
                                stats.record(window);
                            })
                            .map_err(|e| format!("pan draw: {e}"))
                        };
                    pan_step(&mut pan_stats, (0.0, 0.0))?;
                    let pan_tiles_before = pan_stats.tiles.len();
                    for pan in [(0.25, -0.15), (0.6, 0.4), (-1.35, 2.75)] {
                        pan_step(&mut pan_stats, pan)?;
                    }
                    if pan_stats.tiles.len() != pan_tiles_before {
                        return Err(format!(
                            "fractional pan allocated new atlas tiles: {pan_tiles_before} -> {}",
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
