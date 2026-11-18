//! Workspace toolbar (UK3, UD3, UD19, UINV5).
//!
//! The file, edit and zoom groups and the run controls are generated from the
//! single action registry ([`crate::actions::ACTIONS`]) through
//! [`crate::actions::toolbar_actions`]; no label, icon or shortcut is written
//! here for a registered action. The place strip shows the twelve mockup
//! category buttons, all disabled until the part picker lands (U4.2); the
//! "In use" picker is likewise present but disabled until U4.2 supplies its
//! list. The analysis picker, Run, Pause, Stop and the simulation-time pill
//! render the UD19 unavailable state while `CommandState::sim` is
//! [`SimState::Unavailable`].
//!
//! The toolbar is its own view, embedded with `.cached(..)` by its parent
//! (U1.7). It observes the per-window [`CommandState`] entity and notifies
//! itself only when a value it actually shows changes: the `enabled` /
//! `checked` result of every registry toolbar action, the analysis picker's
//! enabled state, and the simulation state behind the time pill. Flipping a
//! `CommandState` field the toolbar does not show (`tab_count`, the dock
//! toggles, `theme`, `canvas_edits.delete`, `canvas_edits.place`, ...) never
//! re-renders it.

use crate::actions::{
    ActionSpec, CommandState, SimState, ToolbarGroup, action_spec, toolbar_actions,
};
use crate::keymap;
use crate::theme;
use crate::widgets::{
    ButtonGroup, Icon, IconButton, Popover, PopoverCloseEvent, PopoverCloseReason, TextButton,
};
use gpui::{
    Context, Entity, FocusHandle, IntoElement, KeyDownEvent, MouseButton, OperatingSystem, Render,
    Role, Subscription, Window, div, prelude::*, px,
};
use std::time::Duration;

/// Toolbar height (UK3).
pub const TOOLBAR_HEIGHT: f32 = 50.0;
/// Horizontal gap between toolbar groups (UK3).
pub const TOOLBAR_GROUP_GAP: f32 = 8.0;
/// Space before the place strip, holding the divider (UK3).
pub const PLACE_STRIP_LEADING: f32 = 14.0;
/// Place-strip divider size (UK3: 1x22).
pub const DIVIDER_WIDTH: f32 = 1.0;
/// Place-strip divider height (UK3: 1x22).
pub const DIVIDER_HEIGHT: f32 = 22.0;
/// "In use" picker width (UK3).
pub const IN_USE_PICKER_WIDTH: f32 = 158.0;
/// Analysis picker width (UK3).
pub const ANALYSIS_PICKER_WIDTH: f32 = 118.0;
/// Run-controls group width (UK3).
pub const RUN_CONTROLS_WIDTH: f32 = 136.0;
/// Run button size inside the run-controls group (UK3: 60x28).
pub const RUN_BUTTON_WIDTH: f32 = 60.0;
/// Run button height inside the run-controls group (UK3: 60x28).
pub const RUN_BUTTON_HEIGHT: f32 = 28.0;
/// Simulation-time pill size (UK3: 74x24, radius 12).
pub const TIME_PILL_WIDTH: f32 = 74.0;
/// Simulation-time pill height (UK3: 74x24, radius 12).
pub const TIME_PILL_HEIGHT: f32 = 24.0;
/// Simulation-time pill corner radius (UK3: 74x24, radius 12).
pub const TIME_PILL_RADIUS: f32 = 12.0;
/// Picker and group height (UK3 groups are 34 high).
pub const PICKER_HEIGHT: f32 = 34.0;

/// Tooltip for every simulation control while simulation is unavailable (UD19).
pub const SIM_UNAVAILABLE_TOOLTIP: &str = "Simulation is not available in this build";
/// Text of the simulation-time pill while no simulation time exists (UD19).
pub const SIM_TIME_PLACEHOLDER: &str = "—";

/// One place-strip category button (mockup order, `design/build-omnia.mjs`).
///
/// The glyphs are the vendored Tabler icons nearest each category; the real
/// symbol glyphs and popovers arrive with the part picker (U4.2, T2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceCategory {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

/// The twelve place-strip categories in mockup order (UK3, U4.2).
pub const PLACE_CATEGORIES: [PlaceCategory; 12] = [
    PlaceCategory {
        id: "sources",
        label: "Sources",
        icon: "bolt",
    },
    PlaceCategory {
        id: "basic",
        label: "Basic",
        icon: "components",
    },
    PlaceCategory {
        id: "diodes",
        label: "Diodes",
        icon: "wave-square",
    },
    PlaceCategory {
        id: "transistors",
        label: "Transistors",
        icon: "cpu-2",
    },
    PlaceCategory {
        id: "analog",
        label: "Analog",
        icon: "wave-sine",
    },
    PlaceCategory {
        id: "logic",
        label: "Logic",
        icon: "binary",
    },
    PlaceCategory {
        id: "mcu",
        label: "Microcontrollers",
        icon: "cpu",
    },
    PlaceCategory {
        id: "indicators",
        label: "Indicators",
        icon: "gauge",
    },
    PlaceCategory {
        id: "power",
        label: "Power",
        icon: "bolt",
    },
    PlaceCategory {
        id: "misc",
        label: "Miscellaneous",
        icon: "layout-grid",
    },
    PlaceCategory {
        id: "electromech",
        label: "Electromechanical",
        icon: "settings",
    },
    PlaceCategory {
        id: "switches",
        label: "Switches",
        icon: "arrows-exchange",
    },
];

/// Which toolbar picker popover is open, if any (UD4: the toolbar owns it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarPicker {
    Analysis,
}

/// The values the toolbar actually shows, projected from [`CommandState`].
///
/// The toolbar re-renders exactly when this projection changes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ShownState {
    /// `(enabled, checked)` per registry toolbar action, in registry order.
    buttons: Vec<(bool, bool)>,
    /// Whether the analysis picker entry (`simulate.transient`) is enabled.
    analysis_enabled: bool,
    /// Whether simulation is unavailable, greying the analysis picker (UD19).
    /// The time pill shows the same placeholder in every other state too, and
    /// Run/Pause checked and enabled already ride in `buttons`, so sub-state
    /// flips that change nothing shown (Idle to Failed) must not render.
    sim_unavailable: bool,
}

impl ShownState {
    fn from_state(state: &CommandState) -> Self {
        let buttons = [
            ToolbarGroup::File,
            ToolbarGroup::Edit,
            ToolbarGroup::Zoom,
            ToolbarGroup::Sim,
        ]
        .iter()
        .flat_map(|group| toolbar_actions(*group))
        .map(|spec| ((spec.enabled)(state), (spec.checked)(state)))
        .collect();
        Self {
            buttons,
            analysis_enabled: action_spec("simulate.transient")
                .is_some_and(|spec| (spec.enabled)(state)),
            sim_unavailable: state.sim == SimState::Unavailable,
        }
    }
}

/// The workspace toolbar view (UK3). Embed with `.cached(..)` (UD4).
pub struct Toolbar {
    state: Entity<CommandState>,
    shown: ShownState,
    in_use_count: usize,
    open_picker: Option<ToolbarPicker>,
    /// Set when the popover closed on this very press (click-outside fires on
    /// mouse-down before the trigger's own handler); the trigger press that
    /// closed it must not reopen it. Cleared on the next executor tick.
    suppress_analysis_press: bool,
    analysis_focus: FocusHandle,
    analysis_popover_focus: FocusHandle,
    renders: u64,
    _state_subscription: Subscription,
}

impl Toolbar {
    /// Creates the toolbar observing `state`, the per-window command state.
    pub fn new(state: Entity<CommandState>, cx: &mut Context<Self>) -> Self {
        let shown = ShownState::from_state(state.read(cx));
        let subscription = cx.observe(&state, |this, entity, cx| {
            let next = ShownState::from_state(entity.read(cx));
            if next != this.shown {
                this.shown = next;
                cx.notify();
            }
        });
        Self {
            state,
            shown,
            in_use_count: 0,
            open_picker: None,
            suppress_analysis_press: false,
            analysis_focus: cx.focus_handle(),
            analysis_popover_focus: cx.focus_handle(),
            renders: 0,
            _state_subscription: subscription,
        }
    }

    /// The command-state entity this toolbar observes.
    pub fn command_state(&self) -> &Entity<CommandState> {
        &self.state
    }

    /// Number of parts shown by the "In use" picker (supplied by U4.2).
    pub fn in_use_count(&self) -> usize {
        self.in_use_count
    }

    /// Sets the "In use" part count; notifies only when it changes.
    pub fn set_in_use_count(&mut self, count: usize, cx: &mut Context<Self>) {
        if count != self.in_use_count {
            self.in_use_count = count;
            cx.notify();
        }
    }

    /// Which picker popover is open, if any.
    pub fn open_picker(&self) -> Option<ToolbarPicker> {
        self.open_picker
    }

    /// Toggles the analysis popover, focusing it on open (UD19, UD4).
    fn toggle_analysis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker = if self.open_picker == Some(ToolbarPicker::Analysis) {
            None
        } else {
            window.focus(&self.analysis_popover_focus, cx);
            Some(ToolbarPicker::Analysis)
        };
        cx.notify();
    }

    /// How many times this view has rendered (tests check the shown-value rule).
    pub fn render_count(&self) -> u64 {
        self.renders
    }
}

/// Label of the "In use" picker for `count` parts (UK3: "In use · N parts").
pub fn in_use_label(count: usize) -> String {
    if count == 1 {
        "In use · 1 part".to_owned()
    } else {
        format!("In use · {count} parts")
    }
}

fn current_os() -> OperatingSystem {
    match std::env::consts::OS {
        "linux" => OperatingSystem::Linux,
        "macos" => OperatingSystem::Mac,
        _ => OperatingSystem::Windows,
    }
}

/// Shortcut hint for a registry action, from its last binding (UD2, UD3).
fn shortcut_for(spec: &ActionSpec) -> Option<String> {
    spec.bindings.last().map(|binding| {
        keymap::format_keybinding(
            &spec.key_binding(binding.keys, binding.context),
            current_os(),
        )
    })
}

/// One registry icon button in a file/edit/zoom group (UK3).
fn registry_icon_button(spec: &'static ActionSpec, state: &CommandState) -> IconButton {
    let enabled = (spec.enabled)(state);
    let button = IconButton::new(
        format!("toolbar-{}", spec.id),
        spec.icon.unwrap_or_default(),
    )
    .aria_label(spec.label)
    .disabled(!enabled);
    // A checked toggle (Full Screen) exposes its pressed state (UD20).
    let button = if (spec.checked)(state) {
        button.toggled(true)
    } else {
        button
    };
    let button = match shortcut_for(spec) {
        Some(shortcut) => button.tooltip_with_shortcut(spec.label, Some(shortcut)),
        None => button.tooltip(spec.label),
    };
    button.on_click(move |_, window, cx| {
        window.dispatch_action((spec.build)(), cx);
    })
}

/// A UK3 group of registry icon buttons (`toolbar_actions` order).
fn registry_group(
    id: &'static str,
    label: &'static str,
    group: ToolbarGroup,
    state: &CommandState,
) -> ButtonGroup {
    let mut element = ButtonGroup::new(id).aria_label(label);
    for spec in toolbar_actions(group) {
        element = element.child(registry_icon_button(spec, state));
    }
    element
}

impl Render for Toolbar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let active = theme::theme(cx);
        let state = *self.state.read(cx);
        let sim_unavailable = state.sim == SimState::Unavailable;
        let analysis_spec = action_spec("simulate.transient");
        let analysis_enabled = analysis_spec.is_some_and(|spec| (spec.enabled)(&state));
        let in_use = in_use_label(self.in_use_count);

        // Place strip: twelve category buttons, disabled until U4.2.
        let mut place_strip = ButtonGroup::new("toolbar-place-strip").aria_label("Place parts");
        for category in PLACE_CATEGORIES {
            place_strip = place_strip.child(
                IconButton::new(format!("toolbar-place-{}", category.id), category.icon)
                    .aria_label(category.label)
                    .tooltip(category.label)
                    .disabled(true),
            );
        }

        // Run controls from the Sim registry group: Run is the primary text
        // button, Pause and Stop are icon buttons (UK3).
        let sim_specs = toolbar_actions(ToolbarGroup::Sim);
        let run_controls = {
            let mut group = div()
                .id("toolbar-run-controls")
                .debug_selector(|| "toolbar-run-controls".to_owned())
                .role(Role::Group)
                .aria_label("Simulation controls")
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .w(px(RUN_CONTROLS_WIDTH))
                .h(px(PICKER_HEIGHT))
                .px(px(6.0))
                .gap(px(2.0))
                .rounded(px(9.0))
                .bg(theme::color(active.bg_inset));
            for spec in sim_specs {
                let enabled = (spec.enabled)(&state);
                if spec.id == "simulate.run" {
                    let button = TextButton::new(format!("toolbar-{}", spec.id), spec.label)
                        .primary()
                        .icon(spec.icon.unwrap_or_default())
                        .width(px(RUN_BUTTON_WIDTH))
                        .height(px(RUN_BUTTON_HEIGHT))
                        .aria_label(spec.label)
                        .active((spec.checked)(&state))
                        .disabled(!enabled);
                    let button = if sim_unavailable {
                        button.tooltip(SIM_UNAVAILABLE_TOOLTIP)
                    } else {
                        button.tooltip(spec.label)
                    };
                    group = group.child(button.on_click(move |_, window, cx| {
                        window.dispatch_action((spec.build)(), cx);
                    }));
                } else {
                    let button = IconButton::new(
                        format!("toolbar-{}", spec.id),
                        spec.icon.unwrap_or_default(),
                    )
                    .aria_label(spec.label)
                    .disabled(!enabled);
                    // Pause exposes its pressed state while paused (UD20).
                    let button = if (spec.checked)(&state) {
                        button.toggled(true)
                    } else {
                        button
                    };
                    let button = if sim_unavailable {
                        button.tooltip(SIM_UNAVAILABLE_TOOLTIP)
                    } else {
                        button.tooltip(spec.label)
                    };
                    group = group.child(button.on_click(move |_, window, cx| {
                        window.dispatch_action((spec.build)(), cx);
                    }));
                }
            }
            group
        };

        let analysis_open = self.open_picker == Some(ToolbarPicker::Analysis);

        div()
            .id("toolbar")
            .debug_selector(|| "toolbar".to_owned())
            .role(Role::Toolbar)
            .aria_label("Toolbar")
            .w_full()
            .h(px(TOOLBAR_HEIGHT))
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .px(px(10.0))
            .gap(px(0.0))
            .bg(theme::color(active.bg_panel))
            .child(registry_group(
                "toolbar-group-file",
                "File",
                ToolbarGroup::File,
                &state,
            ))
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            .child(registry_group(
                "toolbar-group-edit",
                "Edit",
                ToolbarGroup::Edit,
                &state,
            ))
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            .child(registry_group(
                "toolbar-group-zoom",
                "View",
                ToolbarGroup::Zoom,
                &state,
            ))
            // 14 px before the place strip, with the 1x22 divider centred (UK3).
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(PLACE_STRIP_LEADING))
                    .h(px(PICKER_HEIGHT))
                    .child(
                        div()
                            .id("toolbar-divider")
                            .debug_selector(|| "toolbar-divider".to_owned())
                            .w(px(DIVIDER_WIDTH))
                            .h(px(DIVIDER_HEIGHT))
                            .bg(theme::color(active.rule)),
                    ),
            )
            .child(place_strip)
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            // "In use · N parts" picker: present, disabled until U4.2.
            .child(
                div()
                    .id("toolbar-in-use")
                    .debug_selector(|| "toolbar-in-use".to_owned())
                    .role(Role::Button)
                    .aria_label(in_use.clone())
                    .aria_description("unavailable")
                    .tab_stop(false)
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .w(px(IN_USE_PICKER_WIDTH))
                    .h(px(PICKER_HEIGHT))
                    .px(px(12.0))
                    .rounded(px(9.0))
                    .bg(theme::color(active.bg_inset))
                    .border_1()
                    .border_color(theme::color(active.rule))
                    .cursor_default()
                    .child(
                        div()
                            .font_family("Inter")
                            .text_size(px(12.0))
                            .text_color(theme::color(active.text_muted))
                            .child(in_use),
                    )
                    .child(
                        Icon::new("chevron-down")
                            .size(px(16.0))
                            .color(active.text_muted),
                    ),
            )
            // Right-aligned simulation cluster (UK3).
            .child(div().flex_1().min_w_0())
            .child(
                div()
                    .relative()
                    .flex_none()
                    .child(
                        div()
                            .id("toolbar-analysis")
                            .debug_selector(|| "toolbar-analysis".to_owned())
                            .role(Role::Button)
                            .aria_label("Analysis: Transient")
                            .flex()
                            .flex_row()
                            .flex_none()
                            .items_center()
                            .justify_between()
                            .w(px(ANALYSIS_PICKER_WIDTH))
                            .h(px(PICKER_HEIGHT))
                            .px(px(12.0))
                            .rounded(px(9.0))
                            .bg(theme::color(active.bg_inset))
                            .border_1()
                            .border_color(theme::color(active.rule))
                            .track_focus(&self.analysis_focus)
                            .tab_stop(true)
                            .cursor_pointer()
                            .focus_visible(|style| {
                                style.border_color(theme::color(active.focus_ring))
                            })
                            // Toggle on mouse-down, not click: the open popover's
                            // click-outside close fires on mouse-down, so a click
                            // toggle would see the closed state and reopen it.
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    cx.stop_propagation();
                                    if this.suppress_analysis_press {
                                        this.suppress_analysis_press = false;
                                        return;
                                    }
                                    this.toggle_analysis(window, cx);
                                }),
                            )
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                let key = event.keystroke.key.as_str();
                                if (key == "enter" || key == "space")
                                    && !event.keystroke.modifiers.modified()
                                {
                                    this.toggle_analysis(window, cx);
                                }
                            }))
                            .child(
                                div()
                                    .font_family("Inter")
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_size(px(12.0))
                                    .text_color(theme::color(if sim_unavailable {
                                        active.text_muted
                                    } else {
                                        active.text_primary
                                    }))
                                    .child("Transient"),
                            )
                            .child(
                                Icon::new("chevron-down")
                                    .size(px(16.0))
                                    .color(active.text_secondary),
                            ),
                    )
                    .when(analysis_open, |this| {
                        this.child(
                            Popover::new("toolbar-analysis-popover")
                                .aria_label("Analysis")
                                .role(Role::Menu)
                                .width(px(ANALYSIS_PICKER_WIDTH))
                                .focus_handle(&self.analysis_popover_focus)
                                .offset(gpui::point(px(0.0), px(4.0)))
                                .on_close(cx.listener(
                                    |this, event: &PopoverCloseEvent, window, cx| {
                                        if event.reason == PopoverCloseReason::ClickOutside {
                                            this.suppress_analysis_press = true;
                                            cx.spawn(async move |this, cx| {
                                                cx.background_executor()
                                                    .timer(Duration::ZERO)
                                                    .await;
                                                let _ = this.update(cx, |this, _| {
                                                    this.suppress_analysis_press = false;
                                                });
                                            })
                                            .detach();
                                        }
                                        this.open_picker = None;
                                        window.focus(&this.analysis_focus, cx);
                                        cx.notify();
                                    },
                                ))
                                .child(
                                    TextButton::new("toolbar-analysis-transient", "Transient")
                                        .full_width()
                                        .disabled(!analysis_enabled)
                                        .aria_label("Transient"),
                                ),
                        )
                    }),
            )
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            .child(run_controls)
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            // Simulation-time pill: "—" with no dot while unavailable (UD19).
            .child(
                div()
                    .id("toolbar-time-pill")
                    .debug_selector(|| "toolbar-time-pill".to_owned())
                    .role(Role::Status)
                    .aria_label(format!("Simulation time {SIM_TIME_PLACEHOLDER}"))
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(TIME_PILL_WIDTH))
                    .h(px(TIME_PILL_HEIGHT))
                    .rounded(px(TIME_PILL_RADIUS))
                    .border_1()
                    .border_color(theme::color(active.rule))
                    .child(
                        div()
                            .font_family("Source Code Pro")
                            .text_size(px(10.5))
                            .text_color(theme::color(active.text_secondary))
                            .child(SIM_TIME_PLACEHOLDER),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{CanvasEdits, ScreenKind};
    use crate::theme::ActiveTheme;
    use crate::theme::ThemeChoice;
    use gpui::{
        AppContext, Context, Entity, Modifiers, Render, StyleRefinement, TestAppContext,
        VisualTestContext, size,
    };
    use std::cell::Cell;
    use std::rc::Rc;

    fn ready_state() -> CommandState {
        CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 1,
            modified: true,
            can_undo: true,
            can_redo: true,
            has_selection: true,
            has_clip: true,
            canvas_edits: CanvasEdits {
                undo: true,
                delete: true,
                clipboard: true,
                place: false,
            },
            sim: SimState::Unavailable,
            ..CommandState::default()
        }
    }

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

    struct RootView {
        toolbar: Entity<Toolbar>,
        sibling: Entity<SiblingView>,
        saves: Rc<Cell<u32>>,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let active = theme::theme(cx);
            let saves = self.saves.clone();
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(theme::color(active.bg_backdrop))
                .on_action(move |_: &crate::actions::Save, _, _| {
                    saves.set(saves.get() + 1);
                })
                .child(
                    self.toolbar
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(TOOLBAR_HEIGHT))),
                )
                .child(
                    self.sibling
                        .clone()
                        .cached(StyleRefinement::default().flex_1().w_full()),
                )
        }
    }

    struct Harness {
        root: Entity<RootView>,
        toolbar: Entity<Toolbar>,
        state: Entity<CommandState>,
        sibling: Entity<SiblingView>,
        saves: Rc<Cell<u32>>,
    }

    fn harness(
        cx: &mut TestAppContext,
        initial: CommandState,
    ) -> (Harness, &mut VisualTestContext) {
        cx.update(ActiveTheme::install);
        let saves = Rc::new(Cell::new(0));
        let saves_in_view = saves.clone();
        let (root, cx) = cx.add_window_view(|_, cx| {
            let state = cx.new(|_| initial);
            let toolbar = cx.new(|cx| Toolbar::new(state.clone(), cx));
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                toolbar: toolbar.clone(),
                sibling: sibling.clone(),
                saves: saves_in_view,
            }
        });
        let (toolbar, sibling, state) = cx.update(|_, cx| {
            let view = root.read(cx);
            let toolbar = view.toolbar.clone();
            let sibling = view.sibling.clone();
            let state = toolbar.read(cx).command_state().clone();
            (toolbar, sibling, state)
        });
        (
            Harness {
                root,
                toolbar,
                state,
                sibling,
                saves,
            },
            cx,
        )
    }

    fn toolbar_renders(cx: &mut VisualTestContext, toolbar: &Entity<Toolbar>) -> u64 {
        cx.update(|_, cx| toolbar.read(cx).render_count())
    }

    fn set_state(
        cx: &mut VisualTestContext,
        state: &Entity<CommandState>,
        change: impl FnOnce(&mut CommandState),
    ) {
        cx.update(|_, cx| {
            state.update(cx, |current, cx| {
                change(current);
                cx.notify();
            });
        });
        cx.run_until_parked();
    }

    #[test]
    fn shown_state_projection_covers_every_command_state_field() {
        // Fields the toolbar shows must move the projection; fields it does
        // not show must not (U1.6 done-when, UD3, UD19).
        let base = ready_state();
        let shown = |change: &dyn Fn(&mut CommandState)| {
            let mut next = base;
            change(&mut next);
            ShownState::from_state(&next) != ShownState::from_state(&base)
        };
        // Shown: each of these flips a toolbar enabled/checked value.
        assert!(shown(&|s| s.screen = ScreenKind::Home), "screen");
        assert!(shown(&|s| s.has_document = false), "has_document");
        assert!(shown(&|s| s.loading = true), "loading");
        assert!(shown(&|s| s.can_undo = false), "can_undo");
        assert!(shown(&|s| s.can_redo = false), "can_redo");
        assert!(shown(&|s| s.has_selection = false), "has_selection");
        assert!(shown(&|s| s.has_clip = false), "has_clip");
        assert!(shown(&|s| s.full_screen = true), "full_screen");
        assert!(shown(&|s| s.sim = SimState::Idle), "sim");
        assert!(shown(&|s| s.canvas_edits.undo = false), "canvas_edits.undo");
        assert!(
            shown(&|s| s.canvas_edits.clipboard = false),
            "canvas_edits.clipboard"
        );
        // Save is on through `modified` in the baseline; turning it off shows.
        assert!(shown(&|s| s.modified = false), "modified");
        // Not shown on the toolbar.
        assert!(!shown(&|s| s.tab_count = 4), "tab_count must not render");
        assert!(!shown(&|s| s.toolbox = true), "toolbox must not render");
        assert!(
            !shown(&|s| s.spreadsheet = true),
            "spreadsheet must not render"
        );
        assert!(!shown(&|s| s.results = true), "results must not render");
        assert!(
            !shown(&|s| s.theme = ThemeChoice::Gray),
            "theme must not render"
        );
        assert!(
            !shown(&|s| s.canvas_edits.delete = false),
            "canvas_edits.delete is not a toolbar value"
        );
        assert!(
            !shown(&|s| s.canvas_edits.place = true),
            "place strip stays disabled until U4.2, so place must not render"
        );
        // `untitled` alone shows only when it is the reason Save is enabled.
        let mut untitled_only = base;
        untitled_only.modified = false;
        let mut untitled_on = untitled_only;
        untitled_on.untitled = true;
        assert_ne!(
            ShownState::from_state(&untitled_only),
            ShownState::from_state(&untitled_on),
            "untitled enables Save when unmodified"
        );
        assert_eq!(in_use_label(0), "In use · 0 parts");
        assert_eq!(in_use_label(1), "In use · 1 part");
        assert_eq!(in_use_label(11), "In use · 11 parts");
        assert_eq!(PLACE_CATEGORIES.len(), 12);
    }

    #[gpui::test]
    fn toolbar_renders_exactly_when_a_shown_value_changes(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, ready_state());
        cx.simulate_resize(size(px(1440.0), px(900.0)));
        cx.run_until_parked();

        // (description, change, expected render delta)
        type Step = (&'static str, Box<dyn Fn(&mut CommandState)>, u64);
        let steps: Vec<Step> = vec![
            ("tab_count", Box::new(|s| s.tab_count = 4), 0),
            ("toolbox", Box::new(|s| s.toolbox = true), 0),
            ("spreadsheet", Box::new(|s| s.spreadsheet = true), 0),
            ("results", Box::new(|s| s.results = true), 0),
            ("theme", Box::new(|s| s.theme = ThemeChoice::Gray), 0),
            (
                "canvas_edits.delete",
                Box::new(|s| s.canvas_edits.delete = false),
                0,
            ),
            (
                "canvas_edits.place",
                Box::new(|s| s.canvas_edits.place = true),
                0,
            ),
            ("notify without change", Box::new(|_| {}), 0),
            ("full_screen on", Box::new(|s| s.full_screen = true), 1),
            ("full_screen off", Box::new(|s| s.full_screen = false), 1),
            ("can_undo off", Box::new(|s| s.can_undo = false), 1),
            ("can_undo on", Box::new(|s| s.can_undo = true), 1),
            ("can_redo off", Box::new(|s| s.can_redo = false), 1),
            ("can_redo on", Box::new(|s| s.can_redo = true), 1),
            (
                "has_selection off",
                Box::new(|s| s.has_selection = false),
                1,
            ),
            ("has_selection on", Box::new(|s| s.has_selection = true), 1),
            ("has_clip off", Box::new(|s| s.has_clip = false), 1),
            ("has_clip on", Box::new(|s| s.has_clip = true), 1),
            ("modified off", Box::new(|s| s.modified = false), 1),
            ("untitled on", Box::new(|s| s.untitled = true), 1),
            ("untitled off", Box::new(|s| s.untitled = false), 1),
            ("modified on", Box::new(|s| s.modified = true), 1),
            ("loading on", Box::new(|s| s.loading = true), 1),
            ("loading off", Box::new(|s| s.loading = false), 1),
            ("has_document off", Box::new(|s| s.has_document = false), 1),
            ("has_document on", Box::new(|s| s.has_document = true), 1),
            ("screen home", Box::new(|s| s.screen = ScreenKind::Home), 1),
            (
                "screen workspace",
                Box::new(|s| s.screen = ScreenKind::Workspace),
                1,
            ),
            (
                "canvas_edits.undo off",
                Box::new(|s| s.canvas_edits.undo = false),
                1,
            ),
            (
                "canvas_edits.undo on",
                Box::new(|s| s.canvas_edits.undo = true),
                1,
            ),
            (
                "canvas_edits.clipboard off",
                Box::new(|s| s.canvas_edits.clipboard = false),
                1,
            ),
            (
                "canvas_edits.clipboard on",
                Box::new(|s| s.canvas_edits.clipboard = true),
                1,
            ),
            ("sim idle", Box::new(|s| s.sim = SimState::Idle), 1),
            ("sim running", Box::new(|s| s.sim = SimState::Running), 1),
            ("sim paused", Box::new(|s| s.sim = SimState::Paused), 1),
            ("sim failed", Box::new(|s| s.sim = SimState::Failed), 1),
            // Nothing shown changes between Failed and Idle (UD3).
            (
                "sim failed to idle",
                Box::new(|s| s.sim = SimState::Idle),
                0,
            ),
            (
                "sim idle to failed",
                Box::new(|s| s.sim = SimState::Failed),
                0,
            ),
            (
                "sim unavailable",
                Box::new(|s| s.sim = SimState::Unavailable),
                1,
            ),
        ];

        let sibling_before = cx.update(|_, cx| h.sibling.read(cx).renders);
        for (name, change, expected) in steps {
            let before = toolbar_renders(cx, &h.toolbar);
            set_state(cx, &h.state, change);
            let after = toolbar_renders(cx, &h.toolbar);
            assert_eq!(
                after - before,
                expected,
                "flipping {name} should render the toolbar exactly {expected} time(s)"
            );
        }
        assert_eq!(
            cx.update(|_, cx| h.sibling.read(cx).renders),
            sibling_before,
            "toolbar state flips must not re-render the cached sibling"
        );
        let _ = h.root;
    }

    #[gpui::test]
    fn uk3_toolbar_debug_bounds_match_plan(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, ready_state());
        cx.simulate_resize(size(px(1440.0), px(900.0)));
        cx.run_until_parked();
        let _ = &h;

        let toolbar = cx.debug_bounds("toolbar").expect("toolbar bounds");
        assert_eq!(toolbar.size.width, px(1440.0));
        assert_eq!(toolbar.size.height, px(TOOLBAR_HEIGHT));

        for (selector, width) in [
            ("toolbar-group-file", 130.0),
            ("toolbar-group-edit", 162.0),
            ("toolbar-group-zoom", 162.0),
        ] {
            let bounds = cx.debug_bounds(selector).expect(selector);
            assert_eq!(bounds.size.height, px(PICKER_HEIGHT), "{selector} height");
            assert_eq!(bounds.size.width, px(width), "{selector} width");
        }

        let save = cx.debug_bounds("toolbar-file.save").expect("save bounds");
        assert_eq!(save.size.width, px(30.0));
        assert_eq!(save.size.height, px(30.0));

        let divider = cx.debug_bounds("toolbar-divider").expect("divider bounds");
        assert_eq!(divider.size.width, px(DIVIDER_WIDTH));
        assert_eq!(divider.size.height, px(DIVIDER_HEIGHT));

        let place = cx.debug_bounds("toolbar-place-strip").expect("place strip");
        assert_eq!(place.size.height, px(PICKER_HEIGHT));
        for selector in [
            "toolbar-place-sources",
            "toolbar-place-basic",
            "toolbar-place-diodes",
            "toolbar-place-transistors",
            "toolbar-place-analog",
            "toolbar-place-logic",
            "toolbar-place-mcu",
            "toolbar-place-indicators",
            "toolbar-place-power",
            "toolbar-place-misc",
            "toolbar-place-electromech",
            "toolbar-place-switches",
        ] {
            let bounds = cx.debug_bounds(selector).expect(selector);
            assert_eq!(bounds.size.width, px(30.0), "{selector} width");
            assert_eq!(bounds.size.height, px(30.0), "{selector} height");
        }

        let in_use = cx.debug_bounds("toolbar-in-use").expect("in-use bounds");
        assert_eq!(in_use.size.width, px(IN_USE_PICKER_WIDTH));
        assert_eq!(in_use.size.height, px(PICKER_HEIGHT));

        let analysis = cx
            .debug_bounds("toolbar-analysis")
            .expect("analysis bounds");
        assert_eq!(analysis.size.width, px(ANALYSIS_PICKER_WIDTH));
        assert_eq!(analysis.size.height, px(PICKER_HEIGHT));

        let run_controls = cx
            .debug_bounds("toolbar-run-controls")
            .expect("run controls bounds");
        assert_eq!(run_controls.size.width, px(RUN_CONTROLS_WIDTH));
        assert_eq!(run_controls.size.height, px(PICKER_HEIGHT));

        let run = cx
            .debug_bounds("toolbar-simulate.run")
            .expect("run button bounds");
        assert_eq!(run.size.width, px(RUN_BUTTON_WIDTH));
        assert_eq!(run.size.height, px(RUN_BUTTON_HEIGHT));

        let pill = cx
            .debug_bounds("toolbar-time-pill")
            .expect("time pill bounds");
        assert_eq!(pill.size.width, px(TIME_PILL_WIDTH));
        assert_eq!(pill.size.height, px(TIME_PILL_HEIGHT));

        // UK3 order and gaps, left to right.
        let file = cx.debug_bounds("toolbar-group-file").unwrap();
        let edit = cx.debug_bounds("toolbar-group-edit").unwrap();
        let zoom = cx.debug_bounds("toolbar-group-zoom").unwrap();
        assert_eq!(
            edit.origin.x - (file.origin.x + file.size.width),
            px(TOOLBAR_GROUP_GAP)
        );
        assert_eq!(
            zoom.origin.x - (edit.origin.x + edit.size.width),
            px(TOOLBAR_GROUP_GAP)
        );
        assert_eq!(
            place.origin.x - (zoom.origin.x + zoom.size.width),
            px(PLACE_STRIP_LEADING)
        );
        assert_eq!(
            in_use.origin.x - (place.origin.x + place.size.width),
            px(TOOLBAR_GROUP_GAP)
        );
        // Right-aligned cluster ends 10 px from the window edge.
        assert_eq!(
            (pill.origin.x + pill.size.width) + px(10.0),
            toolbar.origin.x + toolbar.size.width
        );
        assert!(analysis.origin.x > in_use.origin.x + in_use.size.width);
    }

    #[gpui::test]
    fn toolbar_clicks_dispatch_registry_actions_and_disabled_clicks_do_nothing(
        cx: &mut TestAppContext,
    ) {
        let (h, cx) = harness(cx, ready_state());
        cx.simulate_resize(size(px(1440.0), px(900.0)));
        cx.run_until_parked();

        // Enabled Save (modified document) dispatches the registry action.
        let save_center = cx.debug_bounds("toolbar-file.save").unwrap().center();
        cx.simulate_click(save_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(h.saves.get(), 1, "clicking Save should dispatch Save once");

        // Disabled Save (unmodified, titled) changes nothing (UINV5).
        set_state(cx, &h.state, |s| {
            s.modified = false;
            s.untitled = false;
        });
        cx.simulate_click(save_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(h.saves.get(), 1, "disabled Save must not dispatch");

        // Disabled place-strip and run controls dispatch nothing.
        let place_center = cx.debug_bounds("toolbar-place-analog").unwrap().center();
        cx.simulate_click(place_center, Modifiers::none());
        let run_center = cx.debug_bounds("toolbar-simulate.run").unwrap().center();
        cx.simulate_click(run_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(h.saves.get(), 1);

        // Analysis picker opens its UD19 list and Escape closes it.
        let analysis_center = cx.debug_bounds("toolbar-analysis").unwrap().center();
        cx.simulate_click(analysis_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| h.toolbar.read(cx).open_picker()),
            Some(ToolbarPicker::Analysis)
        );
        let popover = cx
            .debug_bounds("toolbar-analysis-popover")
            .expect("analysis popover should be visible");
        let analysis = cx.debug_bounds("toolbar-analysis").unwrap();
        assert_eq!(popover.origin.x, analysis.origin.x, "popover under picker");
        assert!(
            popover.origin.y >= analysis.origin.y + analysis.size.height,
            "popover below picker: popover y {} vs picker bottom {}",
            popover.origin.y,
            analysis.origin.y + analysis.size.height
        );
        // Clicking the trigger again closes it (no reopen from click-outside).
        cx.simulate_click(analysis_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| h.toolbar.read(cx).open_picker()),
            None,
            "second trigger click should close the analysis popover"
        );
        assert!(cx.debug_bounds("toolbar-analysis-popover").is_none());
        // Reopen for the Escape check below.
        cx.simulate_click(analysis_center, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| h.toolbar.read(cx).open_picker()),
            Some(ToolbarPicker::Analysis)
        );
        let keystroke = gpui::Keystroke::parse("escape").unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke,
            is_held: false,
            prefer_character_input: false,
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| h.toolbar.read(cx).open_picker()),
            None,
            "Escape should close the analysis popover"
        );

        // In-use count is the toolbar's own shown value.
        let before = toolbar_renders(cx, &h.toolbar);
        cx.update(|_, cx| {
            h.toolbar.update(cx, |toolbar, cx| {
                toolbar.set_in_use_count(11, cx);
            });
        });
        cx.run_until_parked();
        assert_eq!(toolbar_renders(cx, &h.toolbar) - before, 1);
        assert_eq!(cx.update(|_, cx| h.toolbar.read(cx).in_use_count()), 11);
    }
}
