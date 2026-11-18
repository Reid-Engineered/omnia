//! Anchored in-window popover widget (UG2, UG7, UD2, UINV1, UINV12).
//!
//! Renders inside a `deferred` + `anchored` overlay layer, occludes pointer events beneath it,
//! and closes on `Escape` or pointer-down outside its bounds.

use crate::theme;
use gpui::{
    Anchor, AnchoredPositionMode, AnyElement, App, Bounds, BoxShadow, Edges, ElementId,
    FocusHandle, IntoElement, ParentElement, Pixels, Point, RenderOnce, Role, SharedString, Window,
    anchored, deferred, div, point, prelude::*, px,
};
use std::rc::Rc;

/// Reason a [`Popover`] requested dismissal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PopoverCloseReason {
    Escape,
    ClickOutside,
}

/// Event passed to [`Popover::on_close`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopoverCloseEvent {
    pub reason: PopoverCloseReason,
}

/// Corner radius of a popover panel.
pub const POPOVER_RADIUS: f32 = 8.0;
/// Default inset from window edges when clamping a popover inside the viewport.
pub const POPOVER_WINDOW_MARGIN: f32 = 8.0;
/// Default `deferred` draw priority for popovers.
pub const POPOVER_DEFAULT_PRIORITY: usize = 1;

type CloseHandler = Rc<dyn Fn(&PopoverCloseEvent, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug)]
enum PopoverPlacement {
    InPlace,
    At(Point<Pixels>),
    Attach {
        target: Bounds<Pixels>,
        corner: Anchor,
    },
}

/// Anchored overlay container with Esc and click-outside dismissal (UG7).
#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    anchor: Anchor,
    placement: PopoverPlacement,
    offset: Option<Point<Pixels>>,
    position_mode: AnchoredPositionMode,
    window_margin: Pixels,
    priority: usize,
    width: Option<Pixels>,
    min_width: Option<Pixels>,
    max_width: Option<Pixels>,
    max_height: Option<Pixels>,
    padding: Pixels,
    role: Role,
    aria_label: Option<SharedString>,
    key_context: Option<SharedString>,
    focus_handle: Option<FocusHandle>,
    on_close: Option<CloseHandler>,
    children: Vec<AnyElement>,
}

impl Popover {
    /// Creates an anchored popover with default `TopLeft` anchor and priority `1`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            anchor: Anchor::TopLeft,
            placement: PopoverPlacement::InPlace,
            offset: None,
            position_mode: AnchoredPositionMode::Window,
            window_margin: px(POPOVER_WINDOW_MARGIN),
            priority: POPOVER_DEFAULT_PRIORITY,
            width: None,
            min_width: None,
            max_width: None,
            max_height: None,
            padding: px(4.0),
            role: Role::Dialog,
            aria_label: None,
            key_context: None,
            focus_handle: None,
            on_close: None,
            children: Vec::new(),
        }
    }

    /// Sets which corner of the popover attaches to the anchor position or target corner.
    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Positions the popover at a point in window (or local, see [`Self::position_mode`]) coordinates.
    pub fn position(mut self, position: Point<Pixels>) -> Self {
        self.placement = PopoverPlacement::At(position);
        self
    }

    /// Attaches the popover next to `target` at `corner`, flipping sides if needed to fit the window.
    pub fn attach(mut self, target: Bounds<Pixels>, corner: Anchor) -> Self {
        self.placement = PopoverPlacement::Attach { target, corner };
        self
    }

    /// Applies an offset relative to the resolved anchor placement.
    pub fn offset(mut self, offset: Point<Pixels>) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn position_mode(mut self, mode: AnchoredPositionMode) -> Self {
        self.position_mode = mode;
        self
    }

    pub fn window_margin(mut self, margin: Pixels) -> Self {
        self.window_margin = margin;
        self
    }

    /// Sets the `deferred` layer priority (use higher priorities for nested popovers/submenus).
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn min_width(mut self, min_width: Pixels) -> Self {
        self.min_width = Some(min_width);
        self
    }

    pub fn max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = Some(max_width);
        self
    }

    pub fn max_height(mut self, max_height: Pixels) -> Self {
        self.max_height = Some(max_height);
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn key_context(mut self, context: impl Into<SharedString>) -> Self {
        self.key_context = Some(context.into());
        self
    }

    /// Associates a focus handle with the popover panel (so `Escape` and child key events route through it).
    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    /// Invoked when the user presses `Escape` or clicks outside the popover.
    pub fn on_close(
        mut self,
        handler: impl Fn(&PopoverCloseEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Popover {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Popover {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active = theme::theme(cx);
        let debug_id = self.id.to_string();

        let mut panel = div()
            .id(self.id)
            .debug_selector(move || debug_id)
            .role(self.role)
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .when_some(self.key_context, |this, key_context| {
                this.key_context(key_context.as_ref())
            })
            .when_some(self.focus_handle, |this, handle| {
                let handle = handle.tab_stop(false);
                this.track_focus(&handle).tab_stop(false)
            })
            .occlude()
            .flex()
            .flex_col()
            .p(self.padding)
            .when_some(self.width, |this, w| this.w(w))
            .when_some(self.min_width, |this, w| this.min_w(w))
            .when_some(self.max_width, |this, w| this.max_w(w))
            .when_some(self.max_height, |this, h| this.max_h(h))
            .rounded(px(POPOVER_RADIUS))
            .bg(theme::color(active.bg_card))
            .border_1()
            .border_color(theme::color(active.rule))
            .shadow(vec![BoxShadow {
                color: theme::color(active.shadow),
                offset: point(px(0.0), px(8.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            })
            .children(self.children);

        if let Some(on_close) = self.on_close {
            let on_close_out = on_close.clone();
            let on_close_esc = on_close;
            panel = panel
                .on_mouse_down_out(move |_, window, cx| {
                    on_close_out(
                        &PopoverCloseEvent {
                            reason: PopoverCloseReason::ClickOutside,
                        },
                        window,
                        cx,
                    );
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.key == "escape"
                        && !event.keystroke.modifiers.modified()
                        && !window.default_prevented()
                    {
                        window.prevent_default();
                        cx.stop_propagation();
                        on_close_esc(
                            &PopoverCloseEvent {
                                reason: PopoverCloseReason::Escape,
                            },
                            window,
                            cx,
                        );
                    }
                });
        }

        let mut overlay = anchored()
            .anchor(self.anchor)
            .position_mode(self.position_mode)
            .snap_to_window_with_margin(Edges::all(self.window_margin));

        overlay = match self.placement {
            PopoverPlacement::InPlace => overlay,
            PopoverPlacement::At(pos) => overlay.position(pos),
            PopoverPlacement::Attach { target, corner } => overlay.attach(target, corner),
        };
        if let Some(offset) = self.offset {
            overlay = overlay.offset(offset);
        }

        deferred(overlay.child(panel)).with_priority(self.priority)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ActiveTheme;
    use crate::widgets::button::TextButton;
    use gpui::{
        AppContext, Context, Entity, KeyDownEvent, Keystroke, Modifiers, MouseButton, Render,
        StyleRefinement, TestAppContext, VisualTestContext, size,
    };
    use std::cell::Cell;

    struct SiblingView {
        renders: u64,
        clicks_received: u32,
    }

    impl Render for SiblingView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            div()
                .id("underlying-sibling")
                .debug_selector(|| "underlying-sibling".to_owned())
                .size_full()
                .bg(theme::color(active.stage_paper))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.clicks_received += 1;
                        cx.notify();
                    }),
                )
        }
    }

    struct PopoverHostView {
        renders: u64,
        open: bool,
        submenu_open: bool,
        anchor_near_right_edge: bool,
        trigger_focus: FocusHandle,
        popover_focus: FocusHandle,
        item_focus: FocusHandle,
        submenu_focus: FocusHandle,
        trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
        item_clicks: u32,
        last_close_reason: Option<PopoverCloseReason>,
    }

    impl PopoverHostView {
        fn new(cx: &mut Context<Self>) -> Self {
            Self {
                renders: 0,
                open: false,
                submenu_open: false,
                anchor_near_right_edge: false,
                trigger_focus: cx.focus_handle(),
                popover_focus: cx.focus_handle(),
                item_focus: cx.focus_handle(),
                submenu_focus: cx.focus_handle(),
                trigger_bounds: Rc::new(Cell::new(Bounds::new(
                    point(px(16.0), px(8.0)),
                    size(px(80.0), px(30.0)),
                ))),
                item_clicks: 0,
                last_close_reason: None,
            }
        }
    }

    impl Render for PopoverHostView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            let trigger_bounds = if self.anchor_near_right_edge {
                Bounds::new(point(px(950.0), px(8.0)), size(px(80.0), px(30.0)))
            } else {
                self.trigger_bounds.get()
            };

            div()
                .id("popover-bar")
                .w_full()
                .h_full()
                .flex()
                .flex_row()
                .items_center()
                .px(px(16.0))
                .bg(theme::color(active.bg_panel))
                .child(
                    TextButton::new("trigger-btn", "Menu")
                        .focus_handle(&self.trigger_focus)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open = !this.open;
                            if this.open {
                                window.focus(&this.popover_focus, cx);
                            }
                            cx.notify();
                        })),
                )
                .when(self.open, |this| {
                    this.child(
                        Popover::new("test-popover")
                            .anchor(Anchor::TopLeft)
                            .attach(trigger_bounds, Anchor::BottomLeft)
                            .width(px(180.0))
                            .role(Role::Menu)
                            .aria_label("File menu")
                            .focus_handle(&self.popover_focus)
                            .on_close(cx.listener(|this, event: &PopoverCloseEvent, window, cx| {
                                this.open = false;
                                this.submenu_open = false;
                                this.last_close_reason = Some(event.reason);
                                window.focus(&this.trigger_focus, cx);
                                cx.notify();
                            }))
                            .child(
                                TextButton::new("popover-item-1", "Save")
                                    .full_width()
                                    .focus_handle(&self.item_focus)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.item_clicks += 1;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                TextButton::new("popover-item-2", "More")
                                    .full_width()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.submenu_open = true;
                                        window.focus(&this.submenu_focus, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                })
                .when(self.open && self.submenu_open, |this| {
                    this.child(
                        Popover::new("nested-popover")
                            .position(point(px(200.0), px(60.0)))
                            .width(px(140.0))
                            .priority(2)
                            .focus_handle(&self.submenu_focus)
                            .on_close(cx.listener(|this, event: &PopoverCloseEvent, window, cx| {
                                this.submenu_open = false;
                                this.last_close_reason = Some(event.reason);
                                window.focus(&this.popover_focus, cx);
                                cx.notify();
                            }))
                            .child(TextButton::new("nested-item", "Submenu action").full_width()),
                    )
                })
        }
    }

    struct RootView {
        renders: u64,
        host: Entity<PopoverHostView>,
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
                    self.host
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(46.0))),
                )
                .child(
                    self.sibling
                        .clone()
                        .cached(StyleRefinement::default().flex_1().w_full()),
                )
        }
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

    fn send_escape(cx: &mut VisualTestContext) {
        let keystroke = Keystroke::parse("escape").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
    }

    #[gpui::test]
    fn popover_anchors_occludes_and_closes_on_escape_and_outside_click(cx: &mut TestAppContext) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let host = cx.new(PopoverHostView::new);
            let sibling = cx.new(|_| SiblingView {
                renders: 0,
                clicks_received: 0,
            });
            RootView {
                renders: 0,
                host,
                sibling,
            }
        });

        let (host, sibling) = cx.update(|_, cx| {
            let r = root.read(cx);
            (r.host.clone(), r.sibling.clone())
        });
        let root_notifies = observe_count(cx, &root);
        let sibling_notifies = observe_count(cx, &sibling);

        // Open popover by clicking trigger button.
        let trigger_center = cx.debug_bounds("trigger-btn").unwrap().center();
        cx.simulate_click(trigger_center, Modifiers::none());

        let popover_bounds = cx
            .debug_bounds("test-popover")
            .expect("popover should open");
        // Positioned directly below the trigger (y = 8 + 30 = 38) and overlapping sibling area.
        assert!(popover_bounds.origin.y >= px(38.0));
        assert_eq!(popover_bounds.size.width, px(180.0));

        // Hovering an item inside the popover notifies only PopoverHostView and never re-renders
        // the cached sibling.
        let sibling_renders_before_hover = cx.update(|_, cx| sibling.read(cx).renders);
        let item_center = cx.debug_bounds("popover-item-1").unwrap().center();
        cx.simulate_mouse_move(item_center, None, Modifiers::none());
        assert_eq!(root_notifies.get(), 0);
        assert_eq!(sibling_notifies.get(), 0);
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            sibling_renders_before_hover
        );

        // Clicking an item inside keeps popover open and does NOT reach occluded sibling below.
        cx.simulate_click(item_center, Modifiers::none());
        assert!(cx.debug_bounds("test-popover").is_some());
        cx.update(|_, cx| {
            assert_eq!(host.read(cx).item_clicks, 1);
            assert_eq!(sibling.read(cx).clicks_received, 0);
        });

        // Open nested popover; first Esc closes only nested popover, second Esc closes outer popover
        // and restores trigger focus.
        let more_center = cx.debug_bounds("popover-item-2").unwrap().center();
        cx.simulate_click(more_center, Modifiers::none());
        assert!(cx.debug_bounds("nested-popover").is_some());
        assert!(cx.debug_bounds("test-popover").is_some());

        send_escape(cx);
        assert!(
            cx.debug_bounds("nested-popover").is_none(),
            "first Esc must close nested popover"
        );
        assert!(
            cx.debug_bounds("test-popover").is_some(),
            "first Esc must not close outer popover"
        );
        cx.update(|_, cx| {
            assert_eq!(
                host.read(cx).last_close_reason,
                Some(PopoverCloseReason::Escape)
            );
        });

        // Tab from popover panel reaches the first child item (panel itself is tab_stop(false)),
        // and pressing Esc while focused on the child bubbles up to close the popover.
        cx.update(|window, cx| {
            window.focus_next(cx);
            assert!(
                host.read(cx).item_focus.is_focused(window),
                "focus_next from popover container must enter first item"
            );
            window.focus_prev(cx);
            assert!(
                !host.read(cx).popover_focus.is_focused(window),
                "popover panel itself must not be a tab stop"
            );
            let item_focus = host.read(cx).item_focus.clone();
            window.focus(&item_focus, cx);
        });

        send_escape(cx);
        assert!(
            cx.debug_bounds("test-popover").is_none(),
            "second Esc must close outer popover"
        );
        cx.update(|window, cx| {
            assert!(
                host.read(cx).trigger_focus.is_focused(window),
                "Esc close restores trigger focus"
            );
        });

        // Re-open and click outside to close.
        cx.simulate_click(trigger_center, Modifiers::none());
        assert!(cx.debug_bounds("test-popover").is_some());
        cx.simulate_click(point(px(600.0), px(400.0)), Modifiers::none());
        assert!(
            cx.debug_bounds("test-popover").is_none(),
            "click outside must close popover"
        );
        cx.update(|_, cx| {
            assert_eq!(
                host.read(cx).last_close_reason,
                Some(PopoverCloseReason::ClickOutside)
            );
        });
    }

    #[gpui::test]
    fn popover_snaps_within_window_margin(cx: &mut TestAppContext) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let host = cx.new(|cx| {
                let mut h = PopoverHostView::new(cx);
                h.anchor_near_right_edge = true;
                h.open = true;
                h
            });
            let sibling = cx.new(|_| SiblingView {
                renders: 0,
                clicks_received: 0,
            });
            RootView {
                renders: 0,
                host,
                sibling,
            }
        });
        let _ = root;

        let window_size = cx.update(|window, _| window.viewport_size());
        let popover_bounds = cx
            .debug_bounds("test-popover")
            .expect("edge popover should render");
        assert!(
            popover_bounds.right() <= window_size.width - px(POPOVER_WINDOW_MARGIN) + px(0.5),
            "popover right edge ({:?}) overflowed window width ({:?})",
            popover_bounds.right(),
            window_size.width
        );
    }
}
