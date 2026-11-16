//! T1.4: a real hidden window renders `SheetView` with `render_to_image` (G12) at scale
//! factors 1.0, 1.25 and 1.5 (`Window::set_scale_factor`, test-support), and every grid dot
//! must cover whole device pixels: each dot in the image is a solid square of exactly the
//! device rectangle `SheetView::paint_quads` asked for, with no partly covered pixel around
//! it and nothing else drawn on the paper.
//!
//! Follows `omnia-app/tests/fonts.rs`: OMNIA_HEADLESS_CI skips it on hosted CI (no
//! interactive desktop), and Linux/FreeBSD skip it because their platform window does not
//! implement `render_to_image` at this pin.

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod render {
    use gpui::{
        App, AppContext, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowHandle,
        WindowOptions, point, px, size,
    };
    use omnia_canvas::{
        Camera, SheetView,
        sheet::{GRID_DOT_PX, grid_level},
        snap_px::stroke_device_px,
    };
    use omnia_doc::Pt;
    use omnia_theme::GRAPHITE;
    use std::{
        cell::RefCell,
        collections::{HashMap, HashSet},
        rc::Rc,
        time::Duration,
    };

    const SCALE_FACTORS: [f32; 3] = [1.0, 1.25, 1.5];
    /// (zoom, pan in logical px): 100%, a faded level, coarse, and sub-grid levels, each
    /// panned by fractions of a pixel.
    const CAMERAS: [(f64, (f64, f64)); 4] = [
        (1.0, (0.3, 0.7)),
        (1.875, (-5.55, 2.2)),
        (0.37, (13.1, -7.9)),
        (6.1, (0.5, 0.5)),
    ];

    struct Probe {
        camera: Camera,
    }

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            SheetView::new(self.camera.clone(), &GRAPHITE)
        }
    }

    type DeviceRect = (i64, i64, i64, i64);

    fn channel(c: f32) -> u8 {
        (c * 255.0).round() as u8
    }

    /// The device rectangles of the dots `paint_quads` emits for this window.
    fn expected_dots(camera: &Camera, viewport: Bounds<gpui::Pixels>, sf: f32) -> Vec<DeviceRect> {
        let view = SheetView::new(camera.clone(), &GRAPHITE);
        let mut rects = Vec::new();
        let mut first = true;
        view.paint_quads(viewport, sf, |quad| {
            if std::mem::take(&mut first) {
                return; // the paper
            }
            let d = |v: gpui::Pixels| (f64::from(v) * f64::from(sf)).round() as i64;
            let (l, t) = (d(quad.bounds.left()), d(quad.bounds.top()));
            rects.push((
                l,
                t,
                d(quad.bounds.right()) - l,
                d(quad.bounds.bottom()) - t,
            ));
        });
        rects
    }

    /// Checks one rendered frame; returns the number of dots compared.
    fn check(
        raw: &[u8],
        stride: i64,
        (w, h): (i64, i64),
        side: i64,
        expected: &[DeviceRect],
        label: &str,
    ) -> Result<usize, String> {
        let paper = GRAPHITE.stage_paper;
        let paper = [channel(paper.r), channel(paper.g), channel(paper.b), 255];
        assert!(
            raw.len() as i64 >= stride * h * 4 && w <= stride,
            "RGBA image size"
        );
        let pixel = |x: i64, y: i64| -> [u8; 4] {
            let i = ((y * stride + x) * 4) as usize;
            raw[i..i + 4].try_into().unwrap()
        };

        // Expected dots that lie wholly inside the image without touching its border.
        let inside: HashSet<DeviceRect> = expected
            .iter()
            .copied()
            .filter(|&(l, t, rw, rh)| l > 0 && t > 0 && l + rw < w && t + rh < h)
            .collect();
        if inside.len() < 50 {
            return Err(format!(
                "{label}: only {} dots inside the image",
                inside.len()
            ));
        }

        // Every 4-connected non-paper component that does not touch the border.
        let mut seen = vec![false; (w * h) as usize];
        let mut found = HashSet::new();
        let mut colors = HashMap::new();
        for y0 in 0..h {
            for x0 in 0..w {
                if seen[(y0 * w + x0) as usize] || pixel(x0, y0) == paper {
                    continue;
                }
                let color = pixel(x0, y0);
                let (mut min_x, mut min_y, mut max_x, mut max_y) = (x0, y0, x0, y0);
                let mut count = 0;
                let mut uniform = true;
                let mut stack = vec![(x0, y0)];
                seen[(y0 * w + x0) as usize] = true;
                while let Some((x, y)) = stack.pop() {
                    count += 1;
                    uniform &= pixel(x, y) == color;
                    (min_x, min_y) = (min_x.min(x), min_y.min(y));
                    (max_x, max_y) = (max_x.max(x), max_y.max(y));
                    for (nx, ny) in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                        if (0..w).contains(&nx)
                            && (0..h).contains(&ny)
                            && !seen[(ny * w + nx) as usize]
                            && pixel(nx, ny) != paper
                        {
                            seen[(ny * w + nx) as usize] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                if min_x == 0 || min_y == 0 || max_x == w - 1 || max_y == h - 1 {
                    continue;
                }
                let rect = (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1);
                if !uniform || count != rect.2 * rect.3 {
                    return Err(format!(
                        "{label}: dot at {rect:?} is not a solid square of one colour \
                         ({count} pixels, uniform {uniform}): partly covered pixels"
                    ));
                }
                if (rect.2, rect.3) != (side, side) {
                    return Err(format!(
                        "{label}: dot at {rect:?} is not {side}x{side} device pixels (K6)"
                    ));
                }
                if !inside.contains(&rect) {
                    return Err(format!(
                        "{label}: painted {rect:?}, which is not a requested dot rectangle"
                    ));
                }
                found.insert(rect);
                *colors.entry(color).or_insert(0usize) += 1;
            }
        }
        if let Some(missing) = inside.difference(&found).next() {
            return Err(format!(
                "{label}: requested dot {missing:?} missing ({} of {} found)",
                found.len(),
                inside.len()
            ));
        }
        println!("  {label}: {} dots, colours {colors:?}", found.len());
        Ok(found.len())
    }

    pub fn main() {
        if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
            eprintln!("sheet_render skipped: OMNIA_HEADLESS_CI is set (no interactive desktop)");
            return;
        }
        // Rule 9: a stalled hidden-window test still exits by itself.
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(60));
            eprintln!("sheet_render did not finish within 60 s");
            std::process::exit(3);
        });

        let outcome = Rc::new(RefCell::new(None::<Result<usize, String>>));
        let slot = outcome.clone();
        gpui_platform::application().run(move |cx: &mut App| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(400.0)));
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    let camera = Camera::new(Pt::ZERO, 1.0, bounds);
                    cx.new(|_| Probe { camera })
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
                let result = cx.update(|cx| -> Result<usize, String> {
                    let mut dots = 0;
                    for sf in SCALE_FACTORS {
                        for (zoom, pan) in CAMERAS {
                            let label = format!("scale factor {sf}, zoom {zoom}");
                            win.update(cx, |probe, window, cx| {
                                let viewport =
                                    Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                                let mut camera = Camera::new(Pt::ZERO, zoom, viewport);
                                camera.pan_by(point(pan.0, pan.1));
                                probe.camera = camera;
                                cx.notify();
                            })
                            .map_err(|e| format!("{label}: update probe: {e}"))?;
                            let (image, camera, viewport) = cx
                                .update_window(win.into(), |_, window, cx| {
                                    window.set_scale_factor(sf);
                                    window.draw(cx).clear(cx);
                                    assert_eq!(window.scale_factor(), sf, "scale factor reset");
                                    let viewport = Bounds::new(
                                        point(px(0.0), px(0.0)),
                                        window.viewport_size(),
                                    );
                                    let image = window.render_to_image().map_err(|e| e.to_string());
                                    (image, viewport)
                                })
                                .map(|(image, viewport)| {
                                    let camera = win
                                        .read_with(cx, |probe, _| probe.camera.clone())
                                        .expect("read probe");
                                    (image, camera, viewport)
                                })
                                .map_err(|e| format!("{label}: draw: {e}"))?;
                            let image =
                                image.map_err(|e| format!("{label}: render_to_image: {e}"))?;
                            let level = grid_level(camera.scale());
                            let label = format!(
                                "{label}, level {} pitch {:.2} px fade {:.2}",
                                level.level, level.pitch, level.fade
                            );
                            // The image is the platform window at its real DPI; the scene
                            // covers `viewport × sf` device pixels of it. Check only where both
                            // are, so dots at the edge of either count as touching the border.
                            let stride = i64::from(image.width());
                            let scene =
                                |v: gpui::Pixels| (f64::from(v) * f64::from(sf)).round() as i64;
                            let dims = (
                                stride.min(scene(viewport.size.width)),
                                i64::from(image.height()).min(scene(viewport.size.height)),
                            );
                            let expected = expected_dots(&camera, viewport, sf);
                            let side = i64::from(stroke_device_px(GRID_DOT_PX, sf));
                            dots += check(image.as_raw(), stride, dims, side, &expected, &label)?;
                        }
                    }
                    Ok(dots)
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
            Ok(dots) => println!("test sheet_dots_cover_whole_device_pixels ... ok ({dots} dots)"),
            Err(e) => panic!("{e}"),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() {
    render::main();
}

// Linux/FreeBSD PlatformWindow inherits the unsupported readback default at this pin.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() {
    println!(
        "test sheet_dots_cover_whole_device_pixels ... skipped: render_to_image unsupported \
         on this platform"
    );
}
