use gpui::{
    App, Bounds, Context, FontWeight, Window, WindowBounds, WindowHandle, WindowOptions, div,
    point, prelude::*, px, rgb, size,
};
use omnia_app::{STAGE_INK, STAGE_PAPER, Workspace, load_fonts};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct FontProbe {
    family: &'static str,
    weight: FontWeight,
    text: &'static str,
}

impl Render for FontProbe {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(STAGE_PAPER))
            .text_color(rgb(STAGE_INK))
            .p_4()
            .font_family(self.family)
            .font_weight(self.weight)
            .text_size(px(20.0))
            .child(self.text)
    }
}

fn count_non_paper_pixels(raw: &[u8], width: u32, height: u32, paper_rgba: [u8; 4]) -> usize {
    let mut count = 0;
    for y in 0..height {
        let row_start = (y * width * 4) as usize;
        let row_end = ((y + 1) * width * 4) as usize;
        for pixel in raw[row_start..row_end].as_chunks::<4>().0 {
            if *pixel != paper_rgba {
                count += 1;
            }
        }
    }
    count
}

fn count_ink_row_bands(raw: &[u8], width: u32, height: u32, paper_rgba: [u8; 4]) -> usize {
    let mut bands = 0;
    let mut in_band = false;
    for y in 0..height {
        let row_start = (y * width * 4) as usize;
        let row_end = ((y + 1) * width * 4) as usize;
        let row_has_ink = raw[row_start..row_end]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| *pixel != paper_rgba);
        if row_has_ink && !in_band {
            bands += 1;
            in_band = true;
        } else if !row_has_ink {
            in_band = false;
        }
    }
    bands
}

fn main() {
    // Rule 9: ensure even a stalled hidden-window test exits cleanly in CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        eprintln!("fonts test did not finish within 30s");
        std::process::exit(3);
    });

    let outcome = Arc::new(Mutex::new(None::<Result<(), String>>));
    let outcome_for_run = outcome.clone();

    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(err) = load_fonts(cx) {
            *outcome_for_run.lock().unwrap() =
                Some(Err(format!("register bundled Omnia fonts: {err}")));
            cx.quit();
            return;
        }

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(160.0)));
        let workspace_win = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| Workspace::new(None))
                },
            )
            .expect("open hidden workspace window");

        let open_probe = |cx: &mut App, family: &'static str, weight: FontWeight| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| FontProbe {
                        family,
                        weight,
                        text: "Omnia 0123456789 R1 10 kOhm",
                    })
                },
            )
            .expect("open hidden probe window")
        };

        let inter_win = open_probe(cx, "Inter", FontWeight::NORMAL);
        let scp_win = open_probe(cx, "Source Code Pro", FontWeight::NORMAL);

        cx.spawn(async move |cx| {
            let res = cx.update(|cx| -> Result<(), String> {
                let font_names = cx.text_system().all_font_names();
                if !font_names.iter().any(|n| n == "Inter") {
                    return Err(format!("expected Inter in registered fonts: {font_names:?}"));
                }
                if !font_names.iter().any(|n| n == "Source Code Pro") {
                    return Err(format!(
                        "expected Source Code Pro in registered fonts: {font_names:?}"
                    ));
                }

                for weight in [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD] {
                    let mut inter_font = gpui::font("Inter");
                    inter_font.weight = weight;
                    let inter_id = cx.text_system().resolve_font(&inter_font);

                    let mut scp_font = gpui::font("Source Code Pro");
                    scp_font.weight = weight;
                    let scp_id = cx.text_system().resolve_font(&scp_font);

                    if inter_id == scp_id {
                        return Err(format!(
                            "expected Inter and Source Code Pro at weight {weight:?} to resolve to distinct FontIds"
                        ));
                    }
                }

                let paper_rgba = [
                    ((STAGE_PAPER >> 16) & 0xFF) as u8,
                    ((STAGE_PAPER >> 8) & 0xFF) as u8,
                    (STAGE_PAPER & 0xFF) as u8,
                    0xFF,
                ];

                let workspace_img = cx
                    .update_window(workspace_win.into(), |_, window, cx| {
                        window.draw(cx).clear(cx);
                        window
                            .render_to_image()
                            .expect("render workspace window to image")
                    })
                    .map_err(|e| format!("update workspace window: {e}"))?;

                let w = workspace_img.width();
                let h = workspace_img.height();
                if w < 640 || h < 160 {
                    return Err(format!(
                        "expected resized hidden workspace window >= 640x160, got {w}x{h}"
                    ));
                }

                let total_ink = count_non_paper_pixels(workspace_img.as_raw(), w, h, paper_rgba);
                let bands = count_ink_row_bands(workspace_img.as_raw(), w, h, paper_rgba);
                if total_ink == 0 || bands < 2 {
                    return Err(format!(
                        "expected two rendered text lines in Workspace (w={w}, h={h}, total_ink={total_ink}, bands={bands})"
                    ));
                }

                let render_probe_img =
                    |cx: &mut App, win: WindowHandle<FontProbe>| -> Result<_, String> {
                        cx.update_window(win.into(), |_, window, cx| {
                            window.draw(cx).clear(cx);
                            window
                                .render_to_image()
                                .expect("render font probe to image")
                        })
                        .map_err(|e| format!("update probe window: {e}"))
                    };

                let inter_img = render_probe_img(cx, inter_win)?;
                let scp_img = render_probe_img(cx, scp_win)?;

                let inter_ink = count_non_paper_pixels(
                    inter_img.as_raw(),
                    inter_img.width(),
                    inter_img.height(),
                    paper_rgba,
                );
                let scp_ink = count_non_paper_pixels(
                    scp_img.as_raw(),
                    scp_img.width(),
                    scp_img.height(),
                    paper_rgba,
                );
                if inter_ink == 0 {
                    return Err("expected Inter render to draw non-background pixels".into());
                }
                if scp_ink == 0 {
                    return Err("expected Source Code Pro render to draw non-background pixels".into());
                }
                if inter_img.as_raw() == scp_img.as_raw() {
                    return Err(
                        "expected Inter and Source Code Pro to produce different renders".into(),
                    );
                }

                Ok(())
            });

            *outcome_for_run.lock().unwrap() = Some(res);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let res = outcome
        .lock()
        .unwrap()
        .take()
        .expect("test async task did not run before application exited");
    if let Err(err) = res {
        panic!("{err}");
    }
    println!("test bundled_fonts_register_and_render_distinctly ... ok");
}
