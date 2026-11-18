//! Shell button and button-group widgets (UK3, UK7, UK12, UD20, UINV1, UQ1).

use super::{
    icon::{DEFAULT_ICON_SIZE, Icon},
    tooltip::{TOOLTIP_DELAY, Tooltip},
};
use crate::theme;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FocusHandle, FontWeight, IntoElement, ParentElement,
    Pixels, RenderOnce, Role, SharedString, Toggled, Window, div, prelude::*, px,
};
use std::rc::Rc;

/// Toolbar button group height (UK3).
pub const BUTTON_GROUP_HEIGHT: f32 = 34.0;
/// Inner padding around buttons inside a [`ButtonGroup`] (UK3).
pub const BUTTON_GROUP_PADDING: f32 = 2.0;
/// Horizontal gap between buttons inside a [`ButtonGroup`] (UK3).
pub const BUTTON_GROUP_GAP: f32 = 2.0;
/// Corner radius of a [`ButtonGroup`] container (UK3).
pub const BUTTON_GROUP_RADIUS: f32 = 9.0;

/// Default width and height of an [`IconButton`] (UK3).
pub const ICON_BUTTON_SIZE: f32 = 30.0;
/// Corner radius of an [`IconButton`] (UK3).
pub const ICON_BUTTON_RADIUS: f32 = 7.0;

/// Default height of a [`TextButton`] (UK3 / mockup buttons).
pub const TEXT_BUTTON_HEIGHT: f32 = 30.0;
/// Corner radius of a [`TextButton`] (mockup buttons).
pub const TEXT_BUTTON_RADIUS: f32 = 8.0;
/// Default leading icon size inside a [`TextButton`].
pub const TEXT_BUTTON_ICON_SIZE: f32 = 14.0;

/// Focus-ring stroke width (UD20).
pub const FOCUS_RING_WIDTH: f32 = 2.0;

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Visual emphasis for a [`TextButton`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    #[default]
    Plain,
}

/// Container grouping related icon or compact controls (UK3).
#[derive(IntoElement)]
pub struct ButtonGroup {
    id: ElementId,
    height: Pixels,
    padding: Pixels,
    gap: Pixels,
    radius: Pixels,
    role: Role,
    label: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl ButtonGroup {
    /// Creates a horizontal button group with UK3 dimensions (`34 px` high, `2 px` padding/gap).
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            height: px(BUTTON_GROUP_HEIGHT),
            padding: px(BUTTON_GROUP_PADDING),
            gap: px(BUTTON_GROUP_GAP),
            radius: px(BUTTON_GROUP_RADIUS),
            role: Role::Group,
            label: None,
            children: Vec::new(),
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        self.gap = gap;
        self
    }

    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn role(mut self, role: Role) -> Self {
        self.role = role;
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

impl ParentElement for ButtonGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ButtonGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active = theme::theme(cx);
        let debug_id = self.id.to_string();
        div()
            .id(self.id)
            .debug_selector(move || debug_id)
            .role(self.role)
            .when_some(self.label, |this, label| this.aria_label(label))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .h(self.height)
            .p(self.padding)
            .gap(self.gap)
            .rounded(self.radius)
            .bg(theme::color(active.bg_inset))
            .children(self.children)
    }
}

/// Square icon button with active, hover, disabled, focus-ring, tooltip and role support (UK3, UD20).
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon_name: SharedString,
    icon_size: Pixels,
    size: Pixels,
    radius: Pixels,
    active: bool,
    toggled: Option<bool>,
    disabled: bool,
    role: Role,
    aria_label: Option<SharedString>,
    shortcut: Option<SharedString>,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    focus_handle: Option<FocusHandle>,
    tab_stop: bool,
    on_click: Option<ClickHandler>,
}

impl IconButton {
    /// Creates a `30 x 30 px` icon button using Tabler icon `icon_name`.
    pub fn new(id: impl Into<ElementId>, icon_name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon_name: icon_name.into(),
            icon_size: px(DEFAULT_ICON_SIZE),
            size: px(ICON_BUTTON_SIZE),
            radius: px(ICON_BUTTON_RADIUS),
            active: false,
            toggled: None,
            disabled: false,
            role: Role::Button,
            aria_label: None,
            shortcut: None,
            tooltip: None,
            focus_handle: None,
            tab_stop: true,
            on_click: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = size;
        self
    }

    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    /// Marks the button active (`bg/raised` fill and `text/primary` icon).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Marks the button as a toggle button with explicit pressed state for assistive technology.
    pub fn toggled(mut self, toggled: bool) -> Self {
        self.active = toggled;
        self.toggled = Some(toggled);
        self
    }

    /// Disables pointer activation and keyboard focus; sets `aria_description("unavailable")` (UD20).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn tooltip(mut self, label: impl Into<SharedString>) -> Self {
        self.tooltip = Some((label.into(), None));
        self
    }

    pub fn tooltip_with_shortcut(
        mut self,
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
    ) -> Self {
        let shortcut = shortcut.map(Into::into);
        if let Some(ref key) = shortcut {
            self.shortcut = Some(key.clone());
        }
        self.tooltip = Some((label.into(), shortcut));
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active_theme = theme::theme(cx);
        let debug_id = self.id.to_string();
        let icon_color = if self.disabled {
            active_theme.text_muted
        } else if self.active {
            active_theme.text_primary
        } else {
            active_theme.text_secondary
        };

        let mut base = div()
            .id(self.id)
            .debug_selector(move || debug_id)
            .role(self.role)
            .when_some(self.aria_label, |this, label| this.aria_label(label))
            .when_some(self.shortcut, |this, shortcut| {
                this.aria_keyshortcuts(shortcut)
            })
            .when_some(self.toggled, |this, toggled| {
                this.aria_toggled(if toggled {
                    Toggled::True
                } else {
                    Toggled::False
                })
            })
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(self.size)
            .rounded(self.radius)
            .border_2()
            .when(self.active && !self.disabled, |this| {
                this.bg(theme::color(active_theme.bg_raised))
            });

        if self.disabled {
            if let Some(ref handle) = self.focus_handle {
                let _ = handle.clone().tab_stop(false);
            }
            base = base
                .tab_stop(false)
                .cursor_default()
                .aria_description("unavailable");
        } else {
            base = match self.focus_handle {
                Some(ref handle) => {
                    let handle = handle.clone().tab_stop(self.tab_stop);
                    base.track_focus(&handle)
                }
                None => base.focusable().tab_stop(self.tab_stop),
            };
            base = base
                .cursor_pointer()
                .hover(|style| style.bg(theme::color(active_theme.bg_raised)))
                .focus_visible(|style| style.border_color(theme::color(active_theme.focus_ring)))
                .when_some(self.on_click, |this, handler| {
                    this.on_click(move |event, window, cx| handler(event, window, cx))
                });
        }

        if let Some((label, shortcut)) = self.tooltip {
            base = base
                .tooltip_show_delay(TOOLTIP_DELAY)
                .tooltip(Tooltip::builder(label, shortcut));
        }

        base.child(
            Icon::new(self.icon_name)
                .size(self.icon_size)
                .color(icon_color),
        )
    }
}

/// Text button with [`ButtonVariant::Primary`] and [`ButtonVariant::Plain`] variants (UK3, UK7, UD20).
#[derive(IntoElement)]
pub struct TextButton {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    icon_name: Option<SharedString>,
    icon_size: Pixels,
    shortcut: Option<SharedString>,
    height: Pixels,
    padding_x: Pixels,
    width: Option<Pixels>,
    min_width: Option<Pixels>,
    full_width: bool,
    radius: Pixels,
    active: bool,
    disabled: bool,
    role: Role,
    aria_label: Option<SharedString>,
    tooltip: Option<(SharedString, Option<SharedString>)>,
    focus_handle: Option<FocusHandle>,
    tab_stop: bool,
    on_click: Option<ClickHandler>,
}

impl TextButton {
    /// Creates a [`ButtonVariant::Plain`] text button (`30 px` high, `8 px` radius).
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::Plain,
            icon_name: None,
            icon_size: px(TEXT_BUTTON_ICON_SIZE),
            shortcut: None,
            height: px(TEXT_BUTTON_HEIGHT),
            padding_x: px(12.0),
            width: None,
            min_width: None,
            full_width: false,
            radius: px(TEXT_BUTTON_RADIUS),
            active: false,
            disabled: false,
            role: Role::Button,
            aria_label: None,
            tooltip: None,
            focus_handle: None,
            tab_stop: true,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(mut self) -> Self {
        self.variant = ButtonVariant::Primary;
        self
    }

    pub fn plain(mut self) -> Self {
        self.variant = ButtonVariant::Plain;
        self
    }

    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon_name = Some(name.into());
        self
    }

    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = size;
        self
    }

    /// Displays a monospace shortcut hint at the trailing edge of the button.
    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn padding_x(mut self, padding_x: Pixels) -> Self {
        self.padding_x = padding_x;
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

    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }

    pub fn radius(mut self, radius: Pixels) -> Self {
        self.radius = radius;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

    pub fn tooltip(mut self, label: impl Into<SharedString>) -> Self {
        self.tooltip = Some((label.into(), None));
        self
    }

    pub fn tooltip_with_shortcut(
        mut self,
        label: impl Into<SharedString>,
        shortcut: Option<impl Into<SharedString>>,
    ) -> Self {
        self.tooltip = Some((label.into(), shortcut.map(Into::into)));
        self
    }

    pub fn focus_handle(mut self, handle: &FocusHandle) -> Self {
        self.focus_handle = Some(handle.clone());
        self
    }

    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for TextButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active_theme = theme::theme(cx);
        let debug_id = self.id.to_string();
        let (bg, fg, shortcut_fg) = if self.disabled {
            (
                active_theme.bg_inset,
                active_theme.text_muted,
                active_theme.text_muted,
            )
        } else {
            match self.variant {
                ButtonVariant::Primary => (
                    active_theme.button_primary,
                    active_theme.button_on_primary,
                    active_theme.button_on_primary,
                ),
                ButtonVariant::Plain => (
                    active_theme.bg_raised,
                    active_theme.text_primary,
                    active_theme.text_secondary,
                ),
            }
        };

        let aria_label = self.aria_label.unwrap_or_else(|| self.label.clone());
        let has_shortcut = self.shortcut.is_some();

        let mut base = div()
            .id(self.id)
            .debug_selector(move || debug_id)
            .role(self.role)
            .aria_label(aria_label)
            .when_some(self.shortcut.clone(), |this, shortcut| {
                this.aria_keyshortcuts(shortcut)
            })
            .when(self.active && !self.disabled, |this| {
                this.aria_toggled(Toggled::True)
            })
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .when(!has_shortcut, |this| this.justify_center())
            .when(has_shortcut, |this| this.justify_between())
            .gap(px(8.0))
            .h(self.height)
            .px(self.padding_x)
            .when_some(self.width, |this, width| this.w(width))
            .when_some(self.min_width, |this, min_width| this.min_w(min_width))
            .when(self.full_width, |this| this.w_full())
            .rounded(self.radius)
            .border_2()
            .bg(theme::color(bg));

        if self.disabled {
            if let Some(ref handle) = self.focus_handle {
                let _ = handle.clone().tab_stop(false);
            }
            base = base
                .tab_stop(false)
                .cursor_default()
                .aria_description("unavailable");
        } else {
            base = match self.focus_handle {
                Some(ref handle) => {
                    let handle = handle.clone().tab_stop(self.tab_stop);
                    base.track_focus(&handle)
                }
                None => base.focusable().tab_stop(self.tab_stop),
            };
            base = base
                .cursor_pointer()
                .hover(|style| match self.variant {
                    ButtonVariant::Primary => style.opacity(0.9),
                    ButtonVariant::Plain => style.opacity(0.86),
                })
                .focus_visible(|style| style.border_color(theme::color(active_theme.focus_ring)))
                .when_some(self.on_click, |this, handler| {
                    this.on_click(move |event, window, cx| handler(event, window, cx))
                });
        }

        if let Some((label, shortcut)) = self.tooltip {
            base = base
                .tooltip_show_delay(TOOLTIP_DELAY)
                .tooltip(Tooltip::builder(label, shortcut));
        }

        base.when_some(self.icon_name, |this, icon_name| {
            this.child(Icon::new(icon_name).size(self.icon_size).color(fg))
        })
        .child(
            div()
                .font_family("Inter")
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(12.0))
                .text_color(theme::color(fg))
                .child(self.label),
        )
        .when_some(self.shortcut, |this, shortcut| {
            this.child(
                div()
                    .font_family("Source Code Pro")
                    .text_size(px(10.5))
                    .text_color(theme::color(shortcut_fg))
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
        AppContext, Context, Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, Render,
        StyleRefinement, TestAppContext, VisualTestContext, point,
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

    struct ButtonHostView {
        renders: u64,
        first_focus: FocusHandle,
        second_focus: FocusHandle,
        disabled_focus: FocusHandle,
        primary_focus: FocusHandle,
        plain_focus: FocusHandle,
        active_tool: bool,
        first_clicks: Rc<Cell<u32>>,
        second_clicks: Rc<Cell<u32>>,
        disabled_clicks: Rc<Cell<u32>>,
        primary_clicks: Rc<Cell<u32>>,
        plain_clicks: Rc<Cell<u32>>,
    }

    impl ButtonHostView {
        fn new(cx: &mut Context<Self>) -> Self {
            Self {
                renders: 0,
                first_focus: cx.focus_handle(),
                second_focus: cx.focus_handle(),
                disabled_focus: cx.focus_handle(),
                primary_focus: cx.focus_handle(),
                plain_focus: cx.focus_handle(),
                active_tool: true,
                first_clicks: Rc::new(Cell::new(0)),
                second_clicks: Rc::new(Cell::new(0)),
                disabled_clicks: Rc::new(Cell::new(0)),
                primary_clicks: Rc::new(Cell::new(0)),
                plain_clicks: Rc::new(Cell::new(0)),
            }
        }
    }

    impl Render for ButtonHostView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let active = theme::theme(cx);
            let first_clicks = self.first_clicks.clone();
            let second_clicks = self.second_clicks.clone();
            let disabled_clicks = self.disabled_clicks.clone();
            let primary_clicks = self.primary_clicks.clone();
            let plain_clicks = self.plain_clicks.clone();

            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .p(px(8.0))
                .bg(theme::color(active.bg_panel))
                .child(
                    ButtonGroup::new("group-file")
                        .aria_label("File tools")
                        .child(
                            IconButton::new("btn-new", "file-plus")
                                .aria_label("New")
                                .tooltip_with_shortcut("New", Some("Ctrl+N"))
                                .active(self.active_tool)
                                .focus_handle(&self.first_focus)
                                .on_click(move |_, _, _| {
                                    first_clicks.set(first_clicks.get() + 1);
                                }),
                        )
                        .child(
                            IconButton::new("btn-print", "printer")
                                .aria_label("Print")
                                .tooltip("Print is unavailable")
                                .disabled(true)
                                .focus_handle(&self.disabled_focus)
                                .on_click(move |_, _, _| {
                                    disabled_clicks.set(disabled_clicks.get() + 1);
                                }),
                        )
                        .child(
                            IconButton::new("btn-open", "folder-open")
                                .aria_label("Open")
                                .toggled(false)
                                .focus_handle(&self.second_focus)
                                .on_click(move |_, _, _| {
                                    second_clicks.set(second_clicks.get() + 1);
                                }),
                        ),
                )
                .child(
                    TextButton::new("btn-run", "Run")
                        .primary()
                        .icon("player-play")
                        .focus_handle(&self.primary_focus)
                        .on_click(move |_, _, _| {
                            primary_clicks.set(primary_clicks.get() + 1);
                        }),
                )
                .child(
                    TextButton::new("btn-toolbox", "Toolbox")
                        .plain()
                        .shortcut("Ctrl+1")
                        .focus_handle(&self.plain_focus)
                        .on_click(move |_, _, _| {
                            plain_clicks.set(plain_clicks.get() + 1);
                        }),
                )
                .child({
                    let disabled_clicks = self.disabled_clicks.clone();
                    TextButton::new("btn-disabled-text", "Unavailable")
                        .primary()
                        .disabled(true)
                        .on_click(move |_, _, _| {
                            disabled_clicks.set(disabled_clicks.get() + 1);
                        })
                })
        }
    }

    struct RootView {
        renders: u64,
        buttons: Entity<ButtonHostView>,
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
                    self.buttons
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(50.0))),
                )
                .child(
                    self.sibling
                        .clone()
                        .cached(StyleRefinement::default().flex_1().w_full()),
                )
        }
    }

    use std::cell::Cell;

    fn observe_count<T: 'static>(cx: &mut VisualTestContext, entity: &Entity<T>) -> Rc<Cell<u32>> {
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        cx.update(|_, cx| {
            cx.observe(entity, move |_, _| counter.set(counter.get() + 1))
                .detach()
        });
        count
    }

    fn press_key(cx: &mut VisualTestContext, key: &'static str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    #[gpui::test]
    fn button_group_sizes_match_uk3_and_hover_notifies_only_widget_view(cx: &mut TestAppContext) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let buttons = cx.new(ButtonHostView::new);
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                renders: 0,
                buttons,
                sibling,
            }
        });

        let (buttons, sibling) = cx.update(|_, cx| {
            let r = root.read(cx);
            (r.buttons.clone(), r.sibling.clone())
        });

        let group_bounds = cx.debug_bounds("group-file").expect("group bounds");
        assert_eq!(group_bounds.size.height, px(BUTTON_GROUP_HEIGHT));
        // 2 px left + 3 * 30 px buttons + 2 * 2 px gaps + 2 px right = 98 px
        assert_eq!(group_bounds.size.width, px(98.0));

        let new_bounds = cx.debug_bounds("btn-new").expect("btn-new bounds");
        assert_eq!(new_bounds.size.width, px(ICON_BUTTON_SIZE));
        assert_eq!(new_bounds.size.height, px(ICON_BUTTON_SIZE));

        let run_bounds = cx.debug_bounds("btn-run").expect("btn-run bounds");
        assert_eq!(run_bounds.size.height, px(TEXT_BUTTON_HEIGHT));

        let root_notifies = observe_count(cx, &root);
        let sibling_notifies = observe_count(cx, &sibling);
        let buttons_notifies = observe_count(cx, &buttons);
        let initial_sibling_renders = cx.update(|_, cx| sibling.read(cx).renders);
        let initial_buttons_renders = cx.update(|_, cx| buttons.read(cx).renders);

        // Hover enters open button -> notifies only ButtonHostView.
        let open_center = cx.debug_bounds("btn-open").unwrap().center();
        cx.simulate_mouse_move(open_center, None, Modifiers::none());
        // Hover moves within same button -> no extra notify.
        cx.simulate_mouse_move(
            open_center + point(px(1.0), px(1.0)),
            None,
            Modifiers::none(),
        );
        // Hover enters plain text button -> exits open + enters plain.
        let plain_center = cx.debug_bounds("btn-toolbox").unwrap().center();
        cx.simulate_mouse_move(plain_center, None, Modifiers::none());
        // Exit to neutral background first, then hover over disabled buttons -> 0 notifies.
        cx.simulate_mouse_move(point(px(600.0), px(25.0)), None, Modifiers::none());
        let notifies_before_disabled = buttons_notifies.get();
        let renders_before_disabled = cx.update(|_, cx| buttons.read(cx).renders);

        let disabled_center = cx.debug_bounds("btn-print").unwrap().center();
        cx.simulate_mouse_move(disabled_center, None, Modifiers::none());
        let disabled_text_center = cx.debug_bounds("btn-disabled-text").unwrap().center();
        cx.simulate_mouse_move(disabled_text_center, None, Modifiers::none());

        assert_eq!(
            buttons_notifies.get(),
            notifies_before_disabled,
            "hovering disabled buttons should not notify"
        );
        assert_eq!(
            cx.update(|_, cx| buttons.read(cx).renders),
            renders_before_disabled,
            "hovering disabled buttons should not re-render"
        );

        assert_eq!(root_notifies.get(), 0, "hover notified root");
        assert_eq!(sibling_notifies.get(), 0, "hover notified sibling");
        assert_eq!(
            cx.update(|_, cx| sibling.read(cx).renders),
            initial_sibling_renders,
            "hover re-rendered cached sibling"
        );
        assert!(
            buttons_notifies.get() > 0,
            "hover should notify widget view"
        );
        assert!(
            cx.update(|_, cx| buttons.read(cx).renders) > initial_buttons_renders,
            "hover should re-render widget view"
        );

        // Hovering an IconButton with tooltip_with_shortcut shows its tooltip after 500 ms.
        cx.simulate_mouse_move(new_bounds.center(), None, Modifiers::none());
        cx.background_executor.advance_clock(TOOLTIP_DELAY);
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("omnia-tooltip").is_some(),
            "IconButton tooltip should appear after 500 ms"
        );
        assert_eq!(root_notifies.get(), 0, "tooltip notified root");
        assert_eq!(sibling_notifies.get(), 0, "tooltip notified sibling");
    }

    #[gpui::test]
    fn buttons_support_click_keyboard_activation_focus_order_and_disabled(cx: &mut TestAppContext) {
        cx.update(ActiveTheme::install);
        let (root, cx) = cx.add_window_view(|_, cx| {
            let buttons = cx.new(ButtonHostView::new);
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                renders: 0,
                buttons,
                sibling,
            }
        });

        let buttons = cx.update(|_, cx| root.read(cx).buttons.clone());
        let (first_focus, second_focus, disabled_focus, primary_focus, plain_focus) =
            cx.update(|_, cx| {
                let b = buttons.read(cx);
                (
                    b.first_focus.clone(),
                    b.second_focus.clone(),
                    b.disabled_focus.clone(),
                    b.primary_focus.clone(),
                    b.plain_focus.clone(),
                )
            });

        // Mouse click works on enabled buttons and is ignored on disabled buttons.
        let new_center = cx.debug_bounds("btn-new").unwrap().center();
        let print_center = cx.debug_bounds("btn-print").unwrap().center();
        let disabled_text_center = cx.debug_bounds("btn-disabled-text").unwrap().center();
        cx.simulate_click(new_center, Modifiers::none());
        cx.simulate_click(print_center, Modifiers::none());
        cx.simulate_click(disabled_text_center, Modifiers::none());
        cx.update(|_, cx| {
            let b = buttons.read(cx);
            assert_eq!(b.first_clicks.get(), 1);
            assert_eq!(b.disabled_clicks.get(), 0);
        });

        // Focus first button, advance Tab order: must skip disabled button and reach second button.
        cx.update(|window, cx| {
            window.focus(&first_focus, cx);
        });
        cx.update(|window, cx| {
            window.focus_next(cx);
            assert!(
                !disabled_focus.is_focused(window),
                "disabled button must not be a tab stop"
            );
            assert!(
                second_focus.is_focused(window),
                "tab must advance from btn-new over disabled btn-print to btn-open"
            );
        });
        press_key(cx, "enter");
        press_key(cx, "space");
        cx.update(|window, cx| {
            assert!(
                window.last_input_was_keyboard(),
                "keyboard activation enters keyboard modality for focus_visible"
            );
            assert_eq!(buttons.read(cx).second_clicks.get(), 2);
        });

        // Tab to primary and plain text buttons and activate them with Enter / Space.
        cx.update(|window, cx| {
            window.focus_next(cx);
            assert!(primary_focus.is_focused(window));
        });
        press_key(cx, "enter");
        cx.update(|window, cx| {
            window.focus_next(cx);
            assert!(plain_focus.is_focused(window));
        });
        press_key(cx, "space");

        cx.update(|_, cx| {
            let b = buttons.read(cx);
            assert_eq!(b.primary_clicks.get(), 1);
            assert_eq!(b.plain_clicks.get(), 1);
        });
    }
}
