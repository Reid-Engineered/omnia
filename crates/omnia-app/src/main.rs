use gpui::App;
use std::{num::NonZeroU64, process::ExitCode, time::Duration};

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
    if let Some(frames) = frames_remaining {
        // Rule 9: a bounded run must end even if the window is never drawn (GPUI skips
        // drawing hidden or minimized windows), so a stalled run exits non-zero.
        let per_frame_ms = frames.get().saturating_mul(50);
        let limit = Duration::from_secs(30) + Duration::from_millis(per_frame_ms);
        std::thread::spawn(move || {
            std::thread::sleep(limit);
            eprintln!("omnia-app: --frames {frames} did not finish within {limit:?}");
            std::process::exit(3);
        });
    }
    omnia_app::application().run(move |cx: &mut App| {
        omnia_app::open_workspace(cx, frames_remaining).expect("open Omnia workspace window");
    });
    ExitCode::SUCCESS
}
