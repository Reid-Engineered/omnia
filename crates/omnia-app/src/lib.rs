pub mod actions;
pub mod assets;
pub mod chrome;
pub mod keymap;
pub mod parts;
pub mod status_bar;
pub mod theme;
pub mod widgets;

use gpui::{
    App, Bounds, Context, Entity, FontWeight, StyleRefinement, Window, WindowBounds, WindowHandle,
    WindowOptions, div, prelude::*, px, size,
};
use omnia_canvas::{Camera, SheetCanvas};
use omnia_doc::Pt;
use status_bar::{STATUS_BAR_HEIGHT, StatusBar};
use std::{borrow::Cow, num::NonZeroU64};
use theme::ActiveTheme;

pub fn load_fonts(cx: &App) -> gpui::Result<()> {
    load_fonts_with(cx, Cow::Borrowed)
}

#[doc(hidden)]
pub fn load_fonts_with(
    cx: &App,
    mut transform: impl FnMut(&'static [u8]) -> Cow<'static, [u8]>,
) -> gpui::Result<()> {
    cx.text_system().add_fonts(vec![
        transform(include_bytes!("../../../vendor/fonts/inter/Inter-Regular.ttf").as_slice()),
        transform(include_bytes!("../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice()),
        transform(include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice()),
        transform(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Regular.ttf")
                .as_slice(),
        ),
        transform(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Medium.ttf")
                .as_slice(),
        ),
        transform(
            include_bytes!("../../../vendor/fonts/source-code-pro/SourceCodePro-Semibold.ttf")
                .as_slice(),
        ),
    ])
}

pub struct Workspace {
    frames_remaining: Option<NonZeroU64>,
    sheet: Option<Sheet>,
}

/// The sheet canvas and the status bar it feeds, each a separately cached view (D7).
struct Sheet {
    canvas: Entity<SheetCanvas>,
    status_bar: Entity<StatusBar>,
}

impl Workspace {
    /// The text-only workspace (the T0.2 font check renders this one).
    pub fn new(frames_remaining: Option<NonZeroU64>) -> Self {
        Self {
            frames_remaining,
            sheet: None,
        }
    }

    /// Adds the sheet below the text, starting at 100% centred on the sheet origin, with
    /// pan and zoom (T1.5) and the status bar under it.
    pub fn with_sheet(mut self, cx: &mut Context<Self>) -> Self {
        let theme = theme::theme(cx);
        let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
        let canvas = cx.new(|cx| SheetCanvas::new(camera, theme, cx));
        let status_bar = cx.new(|cx| StatusBar::new(&canvas, theme, cx));
        self.sheet = Some(Sheet { canvas, status_bar });
        self
    }

    pub fn canvas(&self) -> Option<&Entity<SheetCanvas>> {
        self.sheet.as_ref().map(|sheet| &sheet.canvas)
    }

    pub fn status_bar(&self) -> Option<&Entity<StatusBar>> {
        self.sheet.as_ref().map(|sheet| &sheet.status_bar)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = theme::theme(cx);
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
        let header = div()
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
            );
        div()
            .size_full()
            .bg(theme::color(active.stage_paper))
            .text_color(theme::color(active.stage_ink))
            .flex()
            .flex_col()
            .child(header)
            .when_some(self.sheet.as_ref(), |root, sheet| {
                root.child(
                    div().flex_1().min_h_0().child(
                        sheet
                            .canvas
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
                )
                .child(
                    sheet.status_bar.clone().cached(
                        StyleRefinement::default()
                            .w_full()
                            .h(px(STATUS_BAR_HEIGHT))
                            .flex_none(),
                    ),
                )
            })
    }
}

/// Builds the GPUI application with the embedded Tabler icon assets attached (UD17, UG14).
pub fn application() -> gpui::Application {
    gpui_platform::application().with_assets(assets::Assets)
}

pub fn open_workspace(
    cx: &mut App,
    frames: Option<NonZeroU64>,
) -> gpui::Result<WindowHandle<Workspace>> {
    ActiveTheme::install(cx);
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
        |_, cx| cx.new(|cx| Workspace::new(frames).with_sheet(cx)),
    )?;
    cx.activate(true);
    Ok(window)
}
