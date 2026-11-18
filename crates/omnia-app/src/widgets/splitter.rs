//! Dock splitter widget with a 6 px hit target drawn as a centered 1 px rule (UK11, UD12, UG9).

use crate::theme;
use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseExitEvent, MouseMoveEvent, MouseUpEvent, Orientation, Pixels, Point, RenderOnce, Role,
    SharedString, Style, Window, div, fill, point, prelude::*, px, relative, size,
};
use std::{cell::Cell, rc::Rc};

/// Interactive hit-target width/height of a splitter (UK11).
pub const SPLITTER_HIT_SIZE: f32 = 6.0;
/// Drawn hairline thickness of a splitter (UK11).
pub const SPLITTER_LINE_SIZE: f32 = 1.0;

/// Orientation of a [`Splitter`] divider bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitterAxis {
    /// Vertical bar separating left and right panes; drags horizontally.
    Vertical,
    /// Horizontal bar separating top and bottom panes; drags vertically.
    Horizontal,
}

impl SplitterAxis {
    const fn orientation(self) -> Orientation {
        match self {
            Self::Vertical => Orientation::Vertical,
            Self::Horizontal => Orientation::Horizontal,
        }
    }

    const fn cursor_style(self) -> CursorStyle {
        match self {
            Self::Vertical => CursorStyle::ResizeLeftRight,
            Self::Horizontal => CursorStyle::ResizeUpDown,
        }
    }

    fn delta(self, from: Point<Pixels>, to: Point<Pixels>) -> Pixels {
        match self {
            Self::Vertical => to.x - from.x,
            Self::Horizontal => to.y - from.y,
        }
    }
}

/// Incremental pointer delta emitted while dragging a [`Splitter`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitterDragEvent {
    pub axis: SplitterAxis,
    /// Signed pixel delta along `axis` since the previous drag update.
    pub delta: Pixels,
    /// Current window-relative pointer position.
    pub position: Point<Pixels>,
}

type DragHandler = Rc<dyn Fn(&SplitterDragEvent, &mut Window, &mut App)>;
type DragEdgeHandler = Rc<dyn Fn(&(), &mut Window, &mut App)>;

/// Dock resize handle with a 6 px hitbox and a centered 1 px `rule` line (UK11).
#[derive(IntoElement)]
pub struct Splitter {
    id: ElementId,
    axis: SplitterAxis,
    aria_label: Option<SharedString>,
    on_drag_start: Option<DragEdgeHandler>,
    on_drag: Option<DragHandler>,
    on_drag_end: Option<DragEdgeHandler>,
}

impl Splitter {
    /// Creates a splitter bar with `axis` orientation.
    pub fn new(id: impl Into<ElementId>, axis: SplitterAxis) -> Self {
        Self {
            id: id.into(),
            axis,
            aria_label: None,
            on_drag_start: None,
            on_drag: None,
            on_drag_end: None,
        }
    }

    /// Creates a vertical splitter (`6 px` wide, full height).
    pub fn vertical(id: impl Into<ElementId>) -> Self {
        Self::new(id, SplitterAxis::Vertical)
    }

    /// Creates a horizontal splitter (`6 px` tall, full width).
    pub fn horizontal(id: impl Into<ElementId>) -> Self {
        Self::new(id, SplitterAxis::Horizontal)
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn on_drag_start(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_drag_start = Some(Rc::new(handler));
        self
    }

    pub fn on_drag(
        mut self,
        handler: impl Fn(&SplitterDragEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_drag = Some(Rc::new(handler));
        self
    }

    pub fn on_drag_end(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_drag_end = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Splitter {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let debug_id = self.id.to_string();
        let mut container = div()
            .id(self.id)
            .debug_selector(move || debug_id)
            .role(Role::Splitter)
            .aria_orientation(self.axis.orientation())
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .flex_none();

        container = match self.axis {
            SplitterAxis::Vertical => container
                .w(px(SPLITTER_HIT_SIZE))
                .h_full()
                .cursor_col_resize(),
            SplitterAxis::Horizontal => container
                .h(px(SPLITTER_HIT_SIZE))
                .w_full()
                .cursor_row_resize(),
        };

        container.child(SplitterTrack {
            axis: self.axis,
            on_drag_start: self.on_drag_start,
            on_drag: self.on_drag,
            on_drag_end: self.on_drag_end,
        })
    }
}

#[derive(Default)]
struct SplitterState {
    hovered: Rc<Cell<bool>>,
    dragging: Rc<Cell<Option<Point<Pixels>>>>,
}

struct SplitterTrack {
    axis: SplitterAxis,
    on_drag_start: Option<DragEdgeHandler>,
    on_drag: Option<DragHandler>,
    on_drag_end: Option<DragEdgeHandler>,
}

impl SplitterTrack {
    fn line_bounds(axis: SplitterAxis, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        let thickness = px(SPLITTER_LINE_SIZE);
        match axis {
            SplitterAxis::Vertical => Bounds {
                origin: point(
                    bounds.origin.x + (bounds.size.width - thickness) * 0.5,
                    bounds.origin.y,
                ),
                size: size(thickness, bounds.size.height),
            },
            SplitterAxis::Horizontal => Bounds {
                origin: point(
                    bounds.origin.x,
                    bounds.origin.y + (bounds.size.height - thickness) * 0.5,
                ),
                size: size(bounds.size.width, thickness),
            },
        }
    }
}

impl Element for SplitterTrack {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name("splitter-track".into()))
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let id = id.expect("SplitterTrack must have an ElementId");
        let active = theme::theme(cx);
        let axis = self.axis;
        let on_drag_start = self.on_drag_start.clone();
        let on_drag = self.on_drag.clone();
        let on_drag_end = self.on_drag_end.clone();

        window.with_element_state::<SplitterState, _>(id, |state, window| {
            let state = state.unwrap_or_default();
            let is_dragging = state.dragging.get().is_some();
            let is_hovered = state.hovered.get();
            let line_color = if is_dragging {
                active.focus_ring
            } else if is_hovered {
                active.text_secondary
            } else {
                active.rule
            };

            window.paint_quad(fill(
                Self::line_bounds(axis, bounds),
                theme::color(line_color),
            ));

            if is_dragging {
                window.set_window_cursor_style(axis.cursor_style());
            }

            let current_view = window.current_view();

            // Mouse down inside the 6 px hitbox starts the drag.
            {
                let hitbox = hitbox.clone();
                let dragging = state.dragging.clone();
                let on_drag_start = on_drag_start.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                        && !window.default_prevented()
                    {
                        dragging.set(Some(event.position));
                        window.prevent_default();
                        cx.stop_propagation();
                        if let Some(ref handler) = on_drag_start {
                            handler(&(), window, cx);
                        }
                        cx.notify(current_view);
                    }
                });
            }

            // Capture-phase move tracks hover transitions (notifying only this view) and
            // continuous drag deltas even after the pointer leaves the 6 px bar.
            {
                let hitbox = hitbox.clone();
                let hovered = state.hovered.clone();
                let dragging = state.dragging.clone();
                let on_drag = on_drag.clone();
                let on_drag_end = on_drag_end.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }

                    let now_hovered = hitbox.is_hovered(window);
                    if now_hovered != hovered.get() {
                        hovered.set(now_hovered);
                        cx.notify(current_view);
                    }

                    if let Some(last) = dragging.get() {
                        if event.pressed_button == Some(MouseButton::Left) {
                            let delta = axis.delta(last, event.position);
                            dragging.set(Some(event.position));
                            if delta != px(0.0)
                                && let Some(ref handler) = on_drag
                            {
                                handler(
                                    &SplitterDragEvent {
                                        axis,
                                        delta,
                                        position: event.position,
                                    },
                                    window,
                                    cx,
                                );
                            }
                        } else {
                            dragging.set(None);
                            if let Some(ref handler) = on_drag_end {
                                handler(&(), window, cx);
                            }
                            cx.notify(current_view);
                        }
                    }
                });
            }

            // Capture-phase mouse up ends any active drag regardless of pointer position.
            {
                let dragging = state.dragging.clone();
                let on_drag_end = on_drag_end.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && dragging.get().is_some()
                    {
                        dragging.set(None);
                        if let Some(ref handler) = on_drag_end {
                            handler(&(), window, cx);
                        }
                        cx.notify(current_view);
                    }
                });
            }

            // Clear hover if the pointer leaves the window directly.
            {
                let hovered = state.hovered.clone();
                window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && hovered.replace(false) {
                        cx.notify(current_view);
                    }
                });
            }

            ((), state)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ActiveTheme;
    use gpui::{
        AppContext, Context, Entity, Modifiers, MouseDownEvent, MouseUpEvent, Render,
        StyleRefinement, TestAppContext, VisualTestContext,
    };

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

    struct SplitterHostView {
        renders: u64,
        vertical_offset: Pixels,
        horizontal_offset: Pixels,
        drag_starts: u32,
        drag_ends: u32,
    }

    impl Render for SplitterHostView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            div()
                .size_full()
                .flex()
                .flex_row()
                .bg(theme::color(active.bg_panel))
                .child(div().w(px(200.0)).h_full())
                .child(
                    Splitter::vertical("v-splitter")
                        .aria_label("Toolbox splitter")
                        .on_drag_start(cx.listener(|this, _, _, _| {
                            this.drag_starts += 1;
                        }))
                        .on_drag(cx.listener(|this, event: &SplitterDragEvent, _, cx| {
                            this.vertical_offset += event.delta;
                            cx.notify();
                        }))
                        .on_drag_end(cx.listener(|this, _, _, _| {
                            this.drag_ends += 1;
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(div().flex_1().w_full())
                        .child(
                            Splitter::horizontal("h-splitter")
                                .aria_label("Bottom dock splitter")
                                .on_drag(cx.listener(|this, event: &SplitterDragEvent, _, cx| {
                                    this.horizontal_offset += event.delta;
                                    cx.notify();
                                })),
                        )
                        .child(div().h(px(120.0)).w_full()),
                )
        }
    }

    struct RootView {
        renders: u64,
        splitters: Entity<SplitterHostView>,
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
                    self.splitters
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(400.0))),
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

    #[test]
    fn line_bounds_centers_1px_rule_inside_6px_hit_area() {
        let v_bounds = Bounds::new(point(px(200.0), px(0.0)), size(px(6.0), px(400.0)));
        let v_line = SplitterTrack::line_bounds(SplitterAxis::Vertical, v_bounds);
        assert_eq!(v_line.origin, point(px(202.5), px(0.0)));
        assert_eq!(v_line.size, size(px(1.0), px(400.0)));

        let h_bounds = Bounds::new(point(px(0.0), px(300.0)), size(px(500.0), px(6.0)));
        let h_line = SplitterTrack::line_bounds(SplitterAxis::Horizontal, h_bounds);
        assert_eq!(h_line.origin, point(px(0.0), px(302.5)));
        assert_eq!(h_line.size, size(px(500.0), px(1.0)));
    }

    #[gpui::test]
    fn splitter_hit_area_hover_isolation_and_drag(cx: &mut TestAppContext) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let splitters = cx.new(|_| SplitterHostView {
                renders: 0,
                vertical_offset: px(0.0),
                horizontal_offset: px(0.0),
                drag_starts: 0,
                drag_ends: 0,
            });
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                renders: 0,
                splitters,
                sibling,
            }
        });

        let (splitters, sibling) = cx.update(|_, cx| {
            let r = root.read(cx);
            (r.splitters.clone(), r.sibling.clone())
        });

        let v_bounds = cx.debug_bounds("v-splitter").expect("v-splitter bounds");
        let h_bounds = cx.debug_bounds("h-splitter").expect("h-splitter bounds");
        assert_eq!(v_bounds.size.width, px(SPLITTER_HIT_SIZE));
        assert_eq!(v_bounds.size.height, px(400.0));
        assert_eq!(h_bounds.size.height, px(SPLITTER_HIT_SIZE));
        assert!(h_bounds.size.width > px(100.0));

        let root_notifies = observe_count(cx, &root);
        let sibling_notifies = observe_count(cx, &sibling);
        let splitters_notifies = observe_count(cx, &splitters);
        let initial_sibling_renders = cx.update(|_, cx| sibling.read(cx).renders);
        let initial_splitters_renders = cx.update(|_, cx| splitters.read(cx).renders);

        // Hover vertical splitter -> notifies only SplitterHostView once; moving inside adds 0;
        // leaving notifies SplitterHostView once.
        let v_center = v_bounds.center();
        cx.simulate_mouse_move(v_center, None, Modifiers::none());
        cx.simulate_mouse_move(v_center + point(px(0.0), px(20.0)), None, Modifiers::none());
        cx.simulate_mouse_move(point(px(10.0), px(10.0)), None, Modifiers::none());

        assert_eq!(root_notifies.get(), 0, "splitter hover notified root");
        assert_eq!(
            sibling_notifies.get(),
            0,
            "splitter hover notified sibling view"
        );
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            initial_sibling_renders
        );
        assert_eq!(splitters_notifies.get(), 2);
        assert_eq!(
            cx.update(|_, cx| splitters.read(cx).renders),
            initial_splitters_renders + 2
        );

        // Drag vertical splitter even after pointer leaves the 6 px bar.
        cx.simulate_mouse_move(v_center, None, Modifiers::none());
        cx.simulate_event(MouseDownEvent {
            position: v_center,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_mouse_move(
            v_center + point(px(45.0), px(10.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            v_center + point(px(80.0), px(25.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_event(MouseUpEvent {
            position: v_center + point(px(80.0), px(25.0)),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        // Moving after MouseUp must NOT drag anymore.
        cx.simulate_mouse_move(
            v_center + point(px(120.0), px(25.0)),
            None,
            Modifiers::none(),
        );

        cx.update(|_, cx| {
            let s = splitters.read(cx);
            assert_eq!(s.drag_starts, 1);
            assert_eq!(s.drag_ends, 1);
            assert_eq!(s.vertical_offset, px(80.0));
        });

        // Drag horizontal splitter along Y axis.
        let h_center = h_bounds.center();
        cx.simulate_mouse_move(h_center, None, Modifiers::none());
        cx.simulate_event(MouseDownEvent {
            position: h_center,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_mouse_move(
            h_center + point(px(30.0), px(-55.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_event(MouseUpEvent {
            position: h_center + point(px(30.0), px(-55.0)),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });

        cx.update(|_, cx| {
            assert_eq!(splitters.read(cx).horizontal_offset, px(-55.0));
        });
    }
}
