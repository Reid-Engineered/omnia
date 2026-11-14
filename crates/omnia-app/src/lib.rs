use gpui::{
    App, Bounds, Context, FontWeight, Window, WindowBounds, WindowHandle, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use std::{borrow::Cow, num::NonZeroU64};

// Temporary until T1.2 generates omnia-theme tokens. Theme/stage/paper and stage/ink in omnia.figcraft / Q2.
pub const STAGE_PAPER: u32 = 0xEFEDE6;
pub const STAGE_INK: u32 = 0x1D1D1F;

pub fn load_fonts(cx: &App) -> gpui::Result<()> {
    cx.text_system().add_fonts(vec![
        Cow::Borrowed(include_bytes!("../../../vendor/fonts/inter/Inter-Regular.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice()),
        Cow::Borrowed(include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice()),
        Cow::Borrowed(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Regular.ttf")
                .as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Medium.ttf")
                .as_slice(),
        ),
        Cow::Borrowed(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Semibold.ttf")
                .as_slice(),
        ),
    ])
}

pub struct Workspace {
    frames_remaining: Option<NonZeroU64>,
}

impl Workspace {
    pub fn new(frames_remaining: Option<NonZeroU64>) -> Self {
        Self { frames_remaining }
    }
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
        div()
            .size_full()
            .bg(rgb(STAGE_PAPER))
            .text_color(rgb(STAGE_INK))
            .p_4()
            .gap_2()
            .flex()
            .flex_col()
            .child(
                div()
                    .font_family("Inter")
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(17.0))
                    .child("Omnia — Inter SemiBold 17 px"),
            )
            .child(
                div()
                    .font_family("Source Code Pro")
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(13.0))
                    .child("grid 2.54  zoom 100% — Source Code Pro Medium 13 px"),
            )
    }
}

pub fn open_workspace(
    cx: &mut App,
    frames: Option<NonZeroU64>,
) -> gpui::Result<WindowHandle<Workspace>> {
    load_fonts(cx)?;
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
    let bounds = Bounds::centered(None, size(px(960.0), px(640.0)), cx);
    let window = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| cx.new(|_| Workspace::new(frames)),
    )?;
    cx.activate(true);
    Ok(window)
}
