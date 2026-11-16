//! The status bar (T1.5): cursor position in mm, the drawn grid step and the zoom.
//!
//! It is its own view, embedded with `.cached(..)`, and fed only by
//! [`CanvasEvent::Status`]: a status update re-renders this view and nothing else (D7). It
//! never reads the canvas while rendering, so the canvas's cached frame does not depend on
//! it either.

use gpui::{Context, Entity, Hsla, IntoElement, Render, Subscription, Window, div, prelude::*, px};
use omnia_canvas::{CanvasEvent, CanvasStatus, SheetCanvas};
use omnia_theme::Theme;

/// Mockup status bar height (`build-omnia.mjs`, "Status bar").
pub const STATUS_BAR_HEIGHT: f32 = 40.0;

const NM_PER_MM: f64 = 1_000_000.0;

pub struct StatusBar {
    status: CanvasStatus,
    theme: &'static Theme,
    renders: u64,
    _subscription: Subscription,
}

impl StatusBar {
    pub fn new(
        canvas: &Entity<SheetCanvas>,
        theme: &'static Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(canvas, |this, _, event: &CanvasEvent, cx| match event {
            CanvasEvent::Status(status) => {
                if this.status != *status {
                    this.status = *status;
                    cx.notify();
                }
            }
        });
        Self {
            status: canvas.read(cx).status(),
            theme,
            renders: 0,
            _subscription: subscription,
        }
    }

    pub fn status(&self) -> CanvasStatus {
        self.status
    }

    /// How many times this view has rendered (tests check D7 with it).
    pub fn render_count(&self) -> u64 {
        self.renders
    }
}

/// `x, y mm` to 0.01 mm, or a dash before the pointer has been over the sheet. World y grows
/// downward, as on screen.
pub fn format_cursor(cursor: Option<omnia_doc::Pt>) -> String {
    match cursor {
        Some(p) => format!(
            "{:.2}, {:.2} mm",
            f64::from(p.x) / NM_PER_MM,
            f64::from(p.y) / NM_PER_MM
        ),
        None => "—".into(),
    }
}

/// A grid step in mm, exact (every K10 level is a whole number of nm, at most 6 decimals),
/// trailing zeros dropped but at least two decimals: `2.54`, `0.079375`, `81.28`.
pub fn format_grid(step_nm: i32) -> String {
    let mut text = format!("{:.6}", f64::from(step_nm) / NM_PER_MM);
    while text.ends_with('0') && text.len() - text.find('.').unwrap_or(text.len()) > 3 {
        text.pop();
    }
    text
}

/// Zoom as a whole percentage: `100%`, `119%`, `5%`.
pub fn format_zoom(zoom: f32) -> String {
    format!("{:.0}%", f64::from(zoom) * 100.0)
}

/// The whole right-hand status text, in the mockup's order.
pub fn format_status(status: &CanvasStatus) -> String {
    format!(
        "{}     grid {}     zoom {}",
        format_cursor(status.cursor),
        format_grid(status.grid),
        format_zoom(status.zoom)
    )
}

fn color(c: omnia_theme::Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div()
            .w_full()
            .h(px(STATUS_BAR_HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .px(px(14.0))
            .bg(color(self.theme.bg_panel))
            .border_t_1()
            .border_color(color(self.theme.rule))
            .font_family("Source Code Pro")
            .text_size(px(11.5))
            .text_color(color(self.theme.text_secondary))
            .child(format_status(&self.status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{GRID_NM, Pt};

    #[test]
    fn formats() {
        assert_eq!(format_grid(GRID_NM), "2.54");
        assert_eq!(format_grid(GRID_NM >> 1), "1.27");
        assert_eq!(format_grid(GRID_NM >> 5), "0.079375");
        assert_eq!(format_grid(GRID_NM << 5), "81.28");
        assert_eq!(format_zoom(1.0), "100%");
        assert_eq!(format_zoom(2f32.powf(0.25)), "119%");
        assert_eq!(format_zoom(0.05), "5%");
        assert_eq!(format_zoom(32.0), "3200%");
        assert_eq!(format_cursor(None), "—");
        assert_eq!(
            format_cursor(Some(Pt {
                x: 12_700_000,
                y: -5_080_000
            })),
            "12.70, -5.08 mm"
        );
        assert_eq!(
            format_status(&CanvasStatus {
                cursor: None,
                grid: GRID_NM,
                zoom: 1.0
            }),
            "—     grid 2.54     zoom 100%"
        );
    }
}
