//! U1.2 render verification for embedded Tabler icons and `Icon` (UD17, UG14).
//!
//! Follows `tests/fonts.rs`: headless asset + `Icon` draw checks run everywhere (CI included),
//! and the real hidden-window `render_to_image` check runs on Windows when `OMNIA_HEADLESS_CI`
//! is unset.

use gpui::{
    AppContext, AssetSource, Context, HeadlessAppContext, NoopTextSystem, Render, SharedString,
    Window, div, prelude::*, px, size,
};
use omnia_app::assets::{Assets, ICON_NAMES, icon_asset_path};
use omnia_app::theme::{self, ActiveTheme};
use omnia_app::widgets::Icon;
use omnia_theme::GRAPHITE;
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

#[derive(Default)]
struct TrackingAssets {
    missing: Mutex<Vec<String>>,
    errors: Mutex<Vec<String>>,
}

impl AssetSource for TrackingAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        match Assets.load(path) {
            Ok(Some(bytes)) => Ok(Some(bytes)),
            Ok(None) => {
                if path.starts_with("icons/") {
                    self.missing.lock().unwrap().push(path.to_owned());
                }
                Ok(None)
            }
            Err(err) => {
                self.errors.lock().unwrap().push(format!("{path}: {err}"));
                Err(err)
            }
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Assets.list(path)
    }
}

const COLS: usize = 10;
const CELL_PX: f32 = 32.0;

struct IconGrid {
    names: &'static [&'static str],
    icon_size: f32,
    color: omnia_theme::Rgba,
}

impl Render for IconGrid {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_row()
            .flex_wrap()
            .content_start()
            .bg(theme::color(GRAPHITE.stage_paper))
            .children(self.names.iter().map(|&name| {
                div()
                    .w(px(CELL_PX))
                    .h(px(CELL_PX))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(name).size(px(self.icon_size)).color(self.color))
            }))
    }
}

fn check_headless_all_icons() {
    let tracker = Arc::new(TrackingAssets::default());
    let mut cx =
        HeadlessAppContext::with_asset_source(Arc::new(NoopTextSystem::new()), tracker.clone());
    cx.update(|cx| ActiveTheme::install(cx));

    let rows = ICON_NAMES.len().div_ceil(COLS);
    let window = cx
        .open_window(
            size(px(COLS as f32 * CELL_PX), px(rows as f32 * CELL_PX)),
            |_, cx| {
                cx.new(|_| IconGrid {
                    names: ICON_NAMES,
                    icon_size: 18.0,
                    color: GRAPHITE.text_primary,
                })
            },
        )
        .expect("open headless icon window");

    cx.update_window(window.into(), |_, window, cx| {
        window.draw(cx).clear(cx);
    })
    .expect("draw headless icon window");

    let missing = tracker.missing.lock().unwrap();
    assert!(missing.is_empty(), "missing icon assets: {missing:?}");
    let errors = tracker.errors.lock().unwrap();
    assert!(errors.is_empty(), "asset load errors: {errors:?}");
    for &name in ICON_NAMES {
        assert!(
            Assets
                .load(&icon_asset_path(name))
                .expect("load icon")
                .is_some(),
            "missing icon {name}"
        );
    }
    println!("test headless_all_icons_render_without_missing_asset ... ok");
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
mod window_render {
    use super::*;
    use gpui::{App, Bounds, WindowBounds, WindowHandle, WindowOptions, point};
    use std::time::Duration;

    struct SingleIcon {
        name: &'static str,
        icon_size: f32,
        color: omnia_theme::Rgba,
    }

    impl Render for SingleIcon {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme::color(GRAPHITE.stage_paper))
                .child(
                    Icon::new(self.name)
                        .size(px(self.icon_size))
                        .color(self.color),
                )
        }
    }

    fn channel(c: f32) -> u8 {
        (c * 255.0).round() as u8
    }

    fn count_cell_non_paper(
        raw: &[u8],
        img_w: u32,
        cell_x: u32,
        cell_y: u32,
        cell_w: u32,
        cell_h: u32,
        paper: [u8; 4],
    ) -> (usize, Option<[u8; 4]>) {
        let mut count = 0usize;
        let mut brightest: Option<([u8; 4], u32)> = None;
        for y in cell_y..cell_y + cell_h {
            for x in cell_x..cell_x + cell_w {
                let idx = ((y * img_w + x) * 4) as usize;
                let px_rgba: [u8; 4] = raw[idx..idx + 4].try_into().unwrap();
                if px_rgba != paper {
                    count += 1;
                    let dist = px_rgba[0].abs_diff(paper[0]) as u32
                        + px_rgba[1].abs_diff(paper[1]) as u32
                        + px_rgba[2].abs_diff(paper[2]) as u32;
                    if brightest.is_none_or(|(_, best)| dist > best) {
                        brightest = Some((px_rgba, dist));
                    }
                }
            }
        }
        (count, brightest.map(|(px, _)| px))
    }

    pub fn run() {
        if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
            eprintln!("icons window test skipped: OMNIA_HEADLESS_CI is set");
            return;
        }
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(30));
            eprintln!("icons test did not finish within 30s");
            std::process::exit(3);
        });

        let outcome = Arc::new(Mutex::new(None::<Result<(), String>>));
        let slot = outcome.clone();

        omnia_app::application().run(move |cx: &mut App| {
            ActiveTheme::install(cx);

            let rows = ICON_NAMES.len().div_ceil(COLS);
            let grid_bounds = Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(COLS as f32 * CELL_PX), px(rows as f32 * CELL_PX)),
            );
            let probe_bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(64.0), px(64.0)));

            let grid_win: WindowHandle<IconGrid> = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(grid_bounds)),
                        show: false,
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        window.resize(grid_bounds.size);
                        cx.new(|_| IconGrid {
                            names: ICON_NAMES,
                            icon_size: 20.0,
                            color: GRAPHITE.text_primary,
                        })
                    },
                )
                .expect("open hidden icon grid window");

            let open_single =
                |cx: &mut App, icon_size: f32, color: omnia_theme::Rgba| -> WindowHandle<SingleIcon> {
                    cx.open_window(
                        WindowOptions {
                            window_bounds: Some(WindowBounds::Windowed(probe_bounds)),
                            show: false,
                            focus: false,
                            ..Default::default()
                        },
                        |window, cx| {
                            window.resize(probe_bounds.size);
                            cx.new(|_| SingleIcon {
                                name: "file-plus",
                                icon_size,
                                color,
                            })
                        },
                    )
                    .expect("open hidden single icon window")
                };

            let small_win = open_single(cx, 16.0, GRAPHITE.accent_mint);
            let large_win = open_single(cx, 24.0, GRAPHITE.stage_select);

            cx.spawn(async move |cx| {
                let res = cx.update(|cx| -> Result<(), String> {
                    let paper = GRAPHITE.stage_paper;
                    let paper_rgba = [
                        channel(paper.r),
                        channel(paper.g),
                        channel(paper.b),
                        channel(paper.a),
                    ];

                    let grid_img = cx
                        .update_window(grid_win.into(), |_, window, cx| {
                            window.set_scale_factor(1.0);
                            window.draw(cx).clear(cx);
                            window.render_to_image().map_err(|e| e.to_string())
                        })
                        .map_err(|e| format!("update grid window: {e}"))?
                        .map_err(|e| format!("render grid window: {e}"))?;

                    let w = grid_img.width();
                    let raw = grid_img.as_raw();
                    for (idx, &name) in ICON_NAMES.iter().enumerate() {
                        let col = (idx % COLS) as u32;
                        let row = (idx / COLS) as u32;
                        let cell = CELL_PX as u32;
                        let (ink_px, _) = count_cell_non_paper(
                            raw,
                            w,
                            col * cell,
                            row * cell,
                            cell,
                            cell,
                            paper_rgba,
                        );
                        if ink_px == 0 {
                            return Err(format!(
                                "icon {name:?} at cell ({col}, {row}) produced 0 non-paper pixels"
                            ));
                        }
                    }

                    let render_single = |cx: &mut App, win: WindowHandle<SingleIcon>| {
                        cx.update_window(win.into(), |_, window, cx| {
                            window.set_scale_factor(1.0);
                            window.draw(cx).clear(cx);
                            window.render_to_image().map_err(|e| e.to_string())
                        })
                    };

                    let small_img = render_single(cx, small_win)
                        .map_err(|e| format!("update small icon window: {e}"))?
                        .map_err(|e| format!("render small icon window: {e}"))?;
                    let large_img = render_single(cx, large_win)
                        .map_err(|e| format!("update large icon window: {e}"))?
                        .map_err(|e| format!("render large icon window: {e}"))?;

                    let (small_px, small_peak) = count_cell_non_paper(
                        small_img.as_raw(),
                        small_img.width(),
                        0,
                        0,
                        small_img.width(),
                        small_img.height(),
                        paper_rgba,
                    );
                    let (large_px, large_peak) = count_cell_non_paper(
                        large_img.as_raw(),
                        large_img.width(),
                        0,
                        0,
                        large_img.width(),
                        large_img.height(),
                        paper_rgba,
                    );

                    if small_px == 0 || large_px == 0 {
                        return Err(format!(
                            "expected non-zero pixels for small ({small_px}) and large ({large_px}) icon probes"
                        ));
                    }
                    if large_px <= small_px {
                        return Err(format!(
                            "expected 24 px icon ({large_px} px) to cover more pixels than 16 px icon ({small_px} px)"
                        ));
                    }
                    let mint = small_peak.expect("small peak pixel");
                    let select = large_peak.expect("large peak pixel");
                    if mint[1] <= mint[0] {
                        return Err(format!(
                            "expected accent_mint tint to have green > red, got {mint:?}"
                        ));
                    }
                    if select[2] <= select[0] {
                        return Err(format!(
                            "expected stage_select tint to have blue > red, got {select:?}"
                        ));
                    }
                    Ok(())
                });

                *slot.lock().unwrap() = Some(res);
                cx.update(|cx| cx.quit());
            })
            .detach();
        });

        let res = outcome
            .lock()
            .unwrap()
            .take()
            .expect("icons render test async task did not finish");
        if let Err(err) = res {
            panic!("{err}");
        }
        println!("test window_all_icons_render_tinted_at_size ... ok");
    }
}

fn main() {
    check_headless_all_icons();
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    window_render::run();
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    println!(
        "test window_all_icons_render_tinted_at_size ... skipped: render_to_image unsupported"
    );
}
