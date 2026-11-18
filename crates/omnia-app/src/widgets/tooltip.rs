//! Shell tooltip widget (UK7, UK12, UD20, UINV1).
//!
//! Renders a themed tooltip containing a label and an optional keyboard shortcut hint after
//! [`TOOLTIP_DELAY`] (500 ms).

use crate::theme;
use gpui::{
    AnyView, App, BoxShadow, Context, IntoElement, Render, Role, SharedString, Window, div, point,
    prelude::*, px,
};
use std::time::Duration;

/// Delay before showing a tooltip on hover (UK12).
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// Corner radius of the tooltip container.
pub const TOOLTIP_RADIUS: f32 = 6.0;

/// In-window tooltip view showing a primary label and an optional monospace shortcut hint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tooltip {
    label: SharedString,
    shortcut: Option<SharedString>,
}

impl Tooltip {
    /// Creates a tooltip with `label` and no shortcut.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
        }
    }

    /// Creates a tooltip with `label` and an optional shortcut hint.
    pub fn with_shortcut(
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
    ) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.map(Into::into),
        }
    }

    /// Attaches a keyboard shortcut hint to display beside the label.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn shortcut_text(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    /// Returns a closure suitable for [`StatefulInteractiveElement::tooltip`].
    pub fn builder(
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
    ) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
        let label: SharedString = label.into();
        let shortcut: Option<SharedString> = shortcut.map(Into::into);
        move |_window, cx| {
            let label = label.clone();
            let shortcut = shortcut.clone();
            cx.new(move |_| Self::with_shortcut(label, shortcut)).into()
        }
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = theme::theme(cx);
        let aria = match &self.shortcut {
            Some(shortcut) => SharedString::from(format!("{} ({})", self.label, shortcut)),
            None => self.label.clone(),
        };
        div()
            .id("omnia-tooltip")
            .debug_selector(|| "omnia-tooltip".to_owned())
            .role(Role::Tooltip)
            .aria_label(aria)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(TOOLTIP_RADIUS))
            .bg(theme::color(active.bg_card))
            .border_1()
            .border_color(theme::color(active.rule))
            .shadow(vec![BoxShadow {
                color: theme::color(active.shadow),
                offset: point(px(0.0), px(4.0)),
                blur_radius: px(12.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(
                div()
                    .font_family("Inter")
                    .text_size(px(12.0))
                    .text_color(theme::color(active.text_primary))
                    .child(self.label.clone()),
            )
            .when_some(self.shortcut.clone(), |this, shortcut| {
                this.child(
                    div()
                        .font_family("Source Code Pro")
                        .text_size(px(10.5))
                        .text_color(theme::color(active.text_secondary))
                        .child(shortcut),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ActiveTheme;
    use gpui::{
        AppContext, Entity, Modifiers, MouseButton, StyleRefinement, TestAppContext,
        VisualTestContext, point,
    };
    use std::{cell::Cell, rc::Rc};

    struct SiblingView {
        renders: u64,
    }

    impl Render for SiblingView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            div().size_full().bg(theme::color(active.stage_paper))
        }
    }

    struct WidgetView {
        renders: u64,
    }

    impl Render for WidgetView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            div()
                .id("tooltip-target")
                .debug_selector(|| "tooltip-target".to_owned())
                .w(px(80.0))
                .h(px(30.0))
                .bg(theme::color(active.bg_raised))
                .tooltip_show_delay(TOOLTIP_DELAY)
                .tooltip(Tooltip::builder("Save", Some("Ctrl+S")))
                .child("Save")
        }
    }

    struct RootView {
        renders: u64,
        widget: Entity<WidgetView>,
        sibling: Entity<SiblingView>,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(theme::color(active.bg_backdrop))
                .child(
                    self.widget
                        .clone()
                        .cached(StyleRefinement::default().w(px(80.0)).h(px(30.0))),
                )
                .child(
                    self.sibling
                        .clone()
                        .cached(StyleRefinement::default().flex_1().w_full()),
                )
        }
    }

    #[test]
    fn tooltip_delay_and_accessors_match_uk12() {
        assert_eq!(TOOLTIP_DELAY, Duration::from_millis(500));
        let tip = Tooltip::new("Save").shortcut("Ctrl+S");
        assert_eq!(tip.label(), "Save");
        assert_eq!(tip.shortcut_text(), Some("Ctrl+S"));

        let plain = Tooltip::new("Fit");
        assert_eq!(plain.label(), "Fit");
        assert_eq!(plain.shortcut_text(), None);
    }

    fn observe_count<T: 'static>(cx: &mut VisualTestContext, entity: &Entity<T>) -> Rc<Cell<u32>> {
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        cx.update(|_, cx| {
            cx.observe(entity, move |_, _| counter.set(counter.get() + 1))
                .detach()
        });
        count
    }

    #[gpui::test]
    fn tooltip_shows_after_500ms_hides_on_exit_and_never_notifies_siblings(
        cx: &mut TestAppContext,
    ) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let widget = cx.new(|_| WidgetView { renders: 0 });
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                renders: 0,
                widget,
                sibling,
            }
        });

        let (widget, sibling) = cx.update(|_, cx| {
            let r = root.read(cx);
            (r.widget.clone(), r.sibling.clone())
        });
        let root_notifies = observe_count(cx, &root);
        let sibling_notifies = observe_count(cx, &sibling);
        let initial_sibling_renders = cx.update(|_, cx| sibling.read(cx).renders);
        let target_center = cx.debug_bounds("tooltip-target").unwrap().center();

        cx.simulate_mouse_move(target_center, None, Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_none(),
            "tooltip appeared before hover delay"
        );

        cx.background_executor
            .advance_clock(TOOLTIP_DELAY - Duration::from_millis(1));
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_none(),
            "tooltip appeared at 499 ms before 500 ms UK12 delay"
        );
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            initial_sibling_renders,
            "hover wait before 500 ms re-rendered cached sibling"
        );

        cx.background_executor
            .advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
        let tooltip_bounds = cx
            .debug_bounds("omnia-tooltip")
            .expect("tooltip visible at 500 ms");
        assert!(tooltip_bounds.size.width > px(0.0));
        assert!(tooltip_bounds.size.height > px(0.0));
        let renders_after_show = cx.update(|_, cx| sibling.read(cx).renders);

        // Moving outside the target triggers hide on the tooltip's owning view only.
        cx.simulate_mouse_move(point(px(300.0), px(300.0)), None, Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_none(),
            "tooltip remained visible after pointer left target"
        );
        assert_eq!(root_notifies.get(), 0, "tooltip hover/show notified root");
        assert_eq!(
            sibling_notifies.get(),
            0,
            "tooltip hover/show notified sibling view"
        );
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            renders_after_show,
            "tooltip hide on pointer exit re-rendered cached sibling view"
        );

        // Re-hover, show again, then verify MouseDown hides the visible tooltip and moving off
        // before 500 ms cancels a pending tooltip.
        cx.simulate_mouse_move(target_center, None, Modifiers::none());
        cx.background_executor.advance_clock(TOOLTIP_DELAY);
        cx.run_until_parked();
        assert!(cx.debug_bounds("omnia-tooltip").is_some());

        cx.simulate_mouse_down(target_center, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(target_center, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_none(),
            "tooltip must hide on mouse down"
        );

        let renders_before_cancel = cx.update(|_, cx| sibling.read(cx).renders);
        cx.simulate_mouse_move(target_center, None, Modifiers::none());
        cx.background_executor
            .advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        cx.simulate_mouse_move(point(px(300.0), px(300.0)), None, Modifiers::none());
        cx.background_executor.advance_clock(TOOLTIP_DELAY);
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_none(),
            "tooltip must not appear after leaving target before 500 ms"
        );
        assert_eq!(root_notifies.get(), 0, "tooltip hover/cancel notified root");
        assert_eq!(
            sibling_notifies.get(),
            0,
            "tooltip hover/cancel notified sibling view"
        );
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            renders_before_cancel,
            "tooltip hover/cancel re-rendered cached sibling view"
        );
        let _ = widget;
    }
}
