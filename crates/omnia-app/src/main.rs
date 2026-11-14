use gpui::{
    App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use std::{num::NonZeroU64, process::ExitCode};

// Temporary until T1.2 generates omnia-theme tokens. Theme/stage/paper in omnia.figcraft.
const STAGE_PAPER: u32 = 0xEFEDE6;

struct Workspace {
    frames_remaining: Option<NonZeroU64>,
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(remaining) = self.frames_remaining {
            self.frames_remaining = NonZeroU64::new(remaining.get() - 1);
            if self.frames_remaining.is_some() {
                window.request_animation_frame();
            } else {
                // At this pin next-frame callbacks run before the following draw, so the
                // final render has been presented before we quit. No timer estimates.
                window.on_next_frame(|_, cx| cx.quit());
            }
        }
        div().size_full().bg(rgb(STAGE_PAPER))
    }
}

fn frame_limit(args: impl IntoIterator<Item = String>) -> Result<Option<NonZeroU64>, String> {
    let mut args = args.into_iter();
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    if flag != "--frames" {
        return Err("usage: omnia-app [--frames N] (N must be a positive integer)".into());
    }
    let frames = args
        .next()
        .and_then(|n| n.parse::<NonZeroU64>().ok())
        .ok_or("--frames requires a positive integer")?;
    if args.next().is_some() {
        return Err("unexpected arguments after --frames N".into());
    }
    Ok(Some(frames))
}

fn main() -> ExitCode {
    let frames_remaining = match frame_limit(std::env::args().skip(1)) {
        Ok(limit) => limit,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    gpui_platform::application().run(move |cx: &mut App| {
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(960.0), px(640.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Workspace { frames_remaining }),
        )
        .expect("open Omnia workspace window");
        cx.activate(true);
    });
    ExitCode::SUCCESS
}
