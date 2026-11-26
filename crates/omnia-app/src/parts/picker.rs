//! Part picker: the place-strip category popovers and the "In use" picker (U4.2; UD2, UD4,
//! UK3, UINV5).
//!
//! [`PartPicker`] is its own view, embedded by the toolbar with `.cached(..)` (UD4). Hovering,
//! a place-flag flip and an "In use" change notify the picker only; GPUI re-runs the render of
//! its ancestors (the toolbar and the window root, `Window::mark_view_dirty`), which replay every
//! other cached child, so the sheet does not re-render for them (UINV2).
//!
//! Opening, closing and choosing move focus (Esc and Tab need it, UINV12), and at the pinned
//! GPUI revision `Window::focus` calls `refresh()`, which re-renders every cached view for one
//! draw, the sheet included. That is recorded in the U4.2 log entry for an owner decision; it is
//! not isolated here.
//!
//! - Each of the twelve place-strip buttons opens a popover listing that category's built-in
//!   symbols (T2.3's library). A category with no built-in symbol is a disabled button.
//! - The "In use · N parts" picker lists the symbols already in the active document with their
//!   counts ([`in_use_entries`]); it is disabled while the document has no parts.
//! - Choosing a symbol dispatches [`PlacePart`]. Its enabled state is
//!   [`place_part_enabled`], which reads `CanvasEdits::place`; that flag stays off until U5.2,
//!   so until then every symbol row is drawn disabled and choosing one changes nothing (UINV5).
//! - Place > Part in the menu bar (U1.5) mirrors both lists through [`part_menu`] and
//!   [`in_use_entries`], so the menu and the popovers cannot drift apart.
//!
//! Which category a built-in symbol belongs to, and the name shown for it, is chosen here
//! ([`SYMBOL_CATALOG`]); a library symbol missing from the table falls back to
//! "Miscellaneous" under its library name, and a test keeps the table complete.

use crate::actions::{CommandState, PlacePart, place_part_enabled};
use crate::chrome::toolbar::{
    IN_USE_PICKER_WIDTH, PICKER_HEIGHT, PLACE_CATEGORIES, PlaceCategory, TOOLBAR_GROUP_GAP,
};
use crate::theme;
use crate::widgets::button::{BUTTON_GROUP_GAP, BUTTON_GROUP_PADDING, ICON_BUTTON_SIZE};
use crate::widgets::{
    ButtonGroup, Icon, IconButton, Popover, PopoverCloseEvent, PopoverCloseReason, TextButton,
};
use gpui::{
    ClickEvent, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, MouseButton, Render, Role,
    Subscription, Window, div, point, prelude::*, px,
};
use omnia_doc::{Doc, SymbolId};
use omnia_symbols::{SymbolDef, SymbolLibrary};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

/// Width of the twelve-button place strip (UK3: 30 px buttons in a group with 2 px padding
/// and gaps).
pub const PLACE_STRIP_WIDTH: f32 = PLACE_CATEGORIES.len() as f32 * ICON_BUTTON_SIZE
    + (PLACE_CATEGORIES.len() - 1) as f32 * BUTTON_GROUP_GAP
    + 2.0 * BUTTON_GROUP_PADDING;
/// Width of the whole picker view: place strip, group gap, "In use" picker (UK3).
pub const PART_PICKER_WIDTH: f32 = PLACE_STRIP_WIDTH + TOOLBAR_GROUP_GAP + IN_USE_PICKER_WIDTH;
/// Minimum width of a category or "In use" popover (chosen).
pub const PICKER_POPOVER_MIN_WIDTH: f32 = 180.0;
/// Category id that catches library symbols missing from [`SYMBOL_CATALOG`].
pub const FALLBACK_CATEGORY: &str = "misc";

/// `(library name, category id, display name)` for every built-in symbol (chosen, U4.2).
///
/// Category ids are [`PLACE_CATEGORIES`] ids. Ground sits with the sources, as the place
/// strip's first category groups the parts a circuit is referenced and driven from.
pub const SYMBOL_CATALOG: &[(&str, &str, &str)] = &[
    ("Capacitor-IEEE-NonPolarized", "basic", "Capacitor"),
    ("Diode-COM-Standard", "diodes", "Diode"),
    ("Ground-COM-General", "sources", "Ground"),
    ("IC-COM-OpAmp", "analog", "Op-amp"),
    ("Resistor-IEEE-Potentiometer", "basic", "Potentiometer"),
    ("Resistor-IEEE-Standard", "basic", "Resistor"),
    ("Source-COM-AC", "sources", "Voltage source (AC)"),
    ("Source-COM-DC", "sources", "Voltage source (DC)"),
];

/// The category id and display name of `def` ([`SYMBOL_CATALOG`], else the fallback).
pub fn symbol_category(def: &SymbolDef) -> (&'static str, &'static str) {
    SYMBOL_CATALOG
        .iter()
        .find(|(name, _, _)| *name == def.name)
        .map_or((FALLBACK_CATEGORY, def.name), |(_, category, label)| {
            (*category, *label)
        })
}

/// One symbol row in a category popover or the Place > Part menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickerEntry {
    pub symbol: SymbolId,
    pub label: &'static str,
}

/// One row of the "In use" picker: a symbol and how many parts of it the document holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InUseEntry {
    pub symbol: SymbolId,
    pub label: &'static str,
    pub count: usize,
}

/// One category of the Place > Part menu, in place-strip order (U1.5 reads this).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartMenuGroup {
    pub category: PlaceCategory,
    pub entries: Vec<PickerEntry>,
}

/// Orders rows by display name, then by lowest symbol id (determinism rule).
fn by_label_then_id(a: (&str, SymbolId), b: (&str, SymbolId)) -> std::cmp::Ordering {
    a.0.cmp(b.0).then(a.1.0.cmp(&b.1.0))
}

/// The built-in symbols of category `category_id`, by display name then id.
pub fn category_entries(library: &SymbolLibrary, category_id: &str) -> Vec<PickerEntry> {
    let mut entries: Vec<PickerEntry> = library
        .iter()
        .filter_map(|def| {
            let (category, label) = symbol_category(def);
            (category == category_id).then_some(PickerEntry {
                symbol: def.id,
                label,
            })
        })
        .collect();
    entries.sort_by(|a, b| by_label_then_id((a.label, a.symbol), (b.label, b.symbol)));
    entries
}

/// Every place-strip category with its symbols, in strip order; empty categories included so
/// the menu can show them disabled (UD2: Place > Part ▸ by category).
pub fn part_menu(library: &SymbolLibrary) -> Vec<PartMenuGroup> {
    PLACE_CATEGORIES
        .iter()
        .map(|category| PartMenuGroup {
            category: *category,
            entries: category_entries(library, category.id),
        })
        .collect()
}

/// The symbols already in `doc` with their part counts, by display name then id.
///
/// The counts sum to [`Doc::part_count`]. A part whose symbol is not in `library` is listed
/// as "Unknown symbol" rather than dropped, so the sum still holds.
pub fn in_use_entries(doc: &Doc, library: &SymbolLibrary) -> Vec<InUseEntry> {
    let mut counts: BTreeMap<SymbolId, usize> = BTreeMap::new();
    for (_, part) in doc.parts() {
        *counts.entry(part.symbol).or_default() += 1;
    }
    let mut entries: Vec<InUseEntry> = counts
        .into_iter()
        .map(|(symbol, count)| {
            let label = if symbol.index() < library.len() {
                symbol_category(library.get(symbol)).1
            } else {
                "Unknown symbol"
            };
            InUseEntry {
                symbol,
                label,
                count,
            }
        })
        .collect();
    entries.sort_by(|a, b| by_label_then_id((a.label, a.symbol), (b.label, b.symbol)));
    entries
}

/// Label of the "In use" picker for `count` parts (UK3: "In use · N parts").
pub fn in_use_label(count: usize) -> String {
    if count == 1 {
        "In use · 1 part".to_owned()
    } else {
        format!("In use · {count} parts")
    }
}

/// Which picker popover is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerPopover {
    /// The popover of `PLACE_CATEGORIES[index]`.
    Category(usize),
    /// The "In use" list.
    InUse,
}

/// The place strip and the "In use" picker (U4.2). Embed with `.cached(..)` at
/// [`PART_PICKER_WIDTH`] x [`PICKER_HEIGHT`] (UD4).
pub struct PartPicker {
    state: Entity<CommandState>,
    /// Symbols per category, in [`PLACE_CATEGORIES`] order; fixed for the library's lifetime.
    categories: Vec<Vec<PickerEntry>>,
    in_use: Vec<InUseEntry>,
    in_use_total: usize,
    /// The one `CommandState` value the picker shows: [`place_part_enabled`].
    place_enabled: bool,
    open: Option<PickerPopover>,
    /// The popover that closed on this very press: click-outside fires on mouse-down before
    /// the trigger's own handler, so that press must not reopen it. Cleared on the next tick.
    suppress_press: Option<PickerPopover>,
    category_focus: Vec<FocusHandle>,
    in_use_focus: FocusHandle,
    popover_focus: FocusHandle,
    /// Closes the open popover when focus leaves it (registered at first render, which has
    /// the window).
    focus_out: Option<Subscription>,
    renders: u64,
    _state_subscription: Subscription,
}

impl PartPicker {
    /// Creates the picker over `library`, observing the per-window `state`.
    pub fn new(
        state: Entity<CommandState>,
        library: Arc<SymbolLibrary>,
        cx: &mut Context<Self>,
    ) -> Self {
        let place_enabled = place_part_enabled(state.read(cx));
        let subscription = cx.observe(&state, |this, entity, cx| {
            let next = place_part_enabled(entity.read(cx));
            if next != this.place_enabled {
                this.place_enabled = next;
                // The flag is drawn only on the rows of an open popover; while all are
                // closed nothing visible changes, and the next open renders the new value.
                if this.open.is_some() {
                    cx.notify();
                }
            }
        });
        Self {
            state,
            categories: part_menu(&library)
                .into_iter()
                .map(|group| group.entries)
                .collect(),
            in_use: Vec::new(),
            in_use_total: 0,
            place_enabled,
            open: None,
            suppress_press: None,
            category_focus: PLACE_CATEGORIES.iter().map(|_| cx.focus_handle()).collect(),
            in_use_focus: cx.focus_handle(),
            popover_focus: cx.focus_handle(),
            focus_out: None,
            renders: 0,
            _state_subscription: subscription,
        }
    }

    /// Symbols of `PLACE_CATEGORIES[index]`, as its popover lists them.
    pub fn category(&self, index: usize) -> &[PickerEntry] {
        &self.categories[index]
    }

    /// The "In use" rows, as its popover lists them.
    pub fn in_use(&self) -> &[InUseEntry] {
        &self.in_use
    }

    /// The part total shown on the "In use" picker.
    pub fn in_use_total(&self) -> usize {
        self.in_use_total
    }

    /// Recomputes the "In use" list from the active document; notifies only on a change.
    pub fn follow_document(&mut self, doc: &Doc, library: &SymbolLibrary, cx: &mut Context<Self>) {
        let entries = in_use_entries(doc, library);
        let total = doc.part_count();
        if entries != self.in_use || total != self.in_use_total {
            self.in_use = entries;
            self.in_use_total = total;
            if self.in_use_total == 0 && self.open == Some(PickerPopover::InUse) {
                self.open = None;
            }
            cx.notify();
        }
    }

    /// Which popover is open, if any.
    pub fn open_popover(&self) -> Option<PickerPopover> {
        self.open
    }

    /// Focus handle of a popover's trigger (tests and focus order, U4.5).
    pub fn trigger_focus(&self, popover: PickerPopover) -> &FocusHandle {
        match popover {
            PickerPopover::Category(index) => &self.category_focus[index],
            PickerPopover::InUse => &self.in_use_focus,
        }
    }

    /// How many times this view has rendered.
    pub fn render_count(&self) -> u64 {
        self.renders
    }

    fn can_open(&self, popover: PickerPopover) -> bool {
        match popover {
            PickerPopover::Category(index) => !self.categories[index].is_empty(),
            PickerPopover::InUse => self.in_use_total > 0,
        }
    }

    /// Opens `popover` (closing any other) or closes it if it is the open one.
    fn toggle(&mut self, popover: PickerPopover, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_open(popover) {
            return;
        }
        if self.open == Some(popover) {
            self.open = None;
            window.focus(self.trigger_focus(popover), cx);
        } else {
            self.open = Some(popover);
            window.focus(&self.popover_focus, cx);
        }
        cx.notify();
    }

    /// A mouse press on a trigger: toggles, unless this press already closed it.
    fn press(&mut self, popover: PickerPopover, window: &mut Window, cx: &mut Context<Self>) {
        if self.suppress_press.take() == Some(popover) {
            // This press closed the popover through click-outside, which leaves focus alone;
            // the trigger stops propagation, so put focus on it here (keyboard reopens it).
            window.focus(self.trigger_focus(popover), cx);
            return;
        }
        self.toggle(popover, window, cx);
    }

    fn close(
        &mut self,
        popover: PickerPopover,
        reason: PopoverCloseReason,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open != Some(popover) {
            return;
        }
        self.open = None;
        match reason {
            PopoverCloseReason::Escape => window.focus(self.trigger_focus(popover), cx),
            PopoverCloseReason::ClickOutside => {
                self.suppress_press = Some(popover);
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::ZERO).await;
                    let _ = this.update(cx, |this, _| this.suppress_press = None);
                })
                .detach();
            }
        }
        cx.notify();
    }

    /// Chooses `symbol`: dispatches [`PlacePart`] when placing is enabled, else does nothing.
    fn choose(&mut self, symbol: SymbolId, window: &mut Window, cx: &mut Context<Self>) {
        // Re-check `enabled` first (UINV5): a disabled row never reaches here, but the flag
        // may have flipped since the frame was drawn.
        if !place_part_enabled(self.state.read(cx)) {
            return;
        }
        window.dispatch_action(Box::new(PlacePart { symbol }), cx);
        if let Some(open) = self.open.take() {
            window.focus(self.trigger_focus(open), cx);
        }
        cx.notify();
    }

    fn symbol_row(
        &self,
        id: String,
        label: String,
        aria: String,
        symbol: SymbolId,
        cx: &mut Context<Self>,
    ) -> TextButton {
        TextButton::new(id, label)
            .full_width()
            .role(Role::MenuItem)
            .aria_label(aria)
            .disabled(!self.place_enabled)
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.choose(symbol, window, cx);
            }))
    }

    fn popover(
        &self,
        popover: PickerPopover,
        id: String,
        title: &'static str,
        cx: &mut Context<Self>,
    ) -> Popover {
        let active = theme::theme(cx);
        Popover::new(id)
            .aria_label(title)
            .role(Role::Menu)
            .min_width(px(PICKER_POPOVER_MIN_WIDTH))
            .focus_handle(&self.popover_focus)
            .offset(point(px(0.0), px(4.0)))
            .on_close(
                cx.listener(move |this, event: &PopoverCloseEvent, window, cx| {
                    this.close(popover, event.reason, window, cx);
                }),
            )
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .font_family("Inter")
                    .text_size(px(11.0))
                    .text_color(theme::color(active.text_secondary))
                    .child(title),
            )
    }

    fn category_trigger(
        &self,
        index: usize,
        category: PlaceCategory,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let popover = PickerPopover::Category(index);
        let entries = &self.categories[index];
        let empty = entries.is_empty();
        let open_here = self.open == Some(popover);
        let tooltip = if empty {
            format!("{} (no parts yet)", category.label)
        } else {
            category.label.to_owned()
        };
        let trigger = IconButton::new(format!("toolbar-place-{}", category.id), category.icon)
            .aria_label(category.label)
            .tooltip(tooltip)
            .active(open_here)
            .disabled(empty)
            .focus_handle(&self.category_focus[index])
            // Mouse presses toggle on mouse-down below; this handles Enter and Space.
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if matches!(event, ClickEvent::Keyboard(_)) {
                    this.toggle(popover, window, cx);
                }
            }));
        let mut wrapper = div().relative().flex_none().child(trigger);
        if !empty {
            wrapper = wrapper.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.press(popover, window, cx);
                }),
            );
        }
        if open_here {
            let mut panel = self.popover(
                popover,
                format!("toolbar-place-{}-popover", category.id),
                category.label,
                cx,
            );
            for entry in self.categories[index].clone() {
                panel = panel.child(self.symbol_row(
                    format!("picker-place-{}", entry.symbol.0),
                    entry.label.to_owned(),
                    entry.label.to_owned(),
                    entry.symbol,
                    cx,
                ));
            }
            wrapper = wrapper.child(panel);
        }
        wrapper
    }

    fn in_use_trigger(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = theme::theme(cx);
        let popover = PickerPopover::InUse;
        let enabled = self.in_use_total > 0;
        let label = in_use_label(self.in_use_total);
        let text_color = if enabled {
            active.text_primary
        } else {
            active.text_muted
        };
        let trigger = div()
            .id("toolbar-in-use")
            .debug_selector(|| "toolbar-in-use".to_owned())
            .role(Role::Button)
            .aria_label(label.clone())
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
            .child(
                div()
                    .font_family("Inter")
                    .text_size(px(12.0))
                    .text_color(theme::color(text_color))
                    .child(label),
            )
            .child(Icon::new("chevron-down").size(px(16.0)).color(if enabled {
                active.text_secondary
            } else {
                active.text_muted
            }));
        let trigger = if enabled {
            trigger
                .track_focus(&self.in_use_focus)
                .tab_stop(true)
                .cursor_pointer()
                .focus_visible(|style| style.border_color(theme::color(active.focus_ring)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.press(popover, window, cx);
                    }),
                )
                .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if (key == "enter" || key == "space") && !event.keystroke.modifiers.modified() {
                        this.toggle(popover, window, cx);
                    }
                }))
        } else {
            trigger
                .tab_stop(false)
                .cursor_default()
                .aria_description("unavailable")
        };
        let mut wrapper = div().relative().flex_none().child(trigger);
        if self.open == Some(popover) {
            let mut panel =
                self.popover(popover, "toolbar-in-use-popover".to_owned(), "In use", cx);
            for entry in self.in_use.clone() {
                panel = panel.child(self.symbol_row(
                    format!("picker-in-use-{}", entry.symbol.0),
                    format!("{} · {}", entry.label, entry.count),
                    format!("{}, {} in use", entry.label, entry.count),
                    entry.symbol,
                    cx,
                ));
            }
            wrapper = wrapper.child(panel);
        }
        wrapper
    }
}

impl Render for PartPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if self.focus_out.is_none() {
            // Focus leaving an open popover (Tab past its last enabled row, or to anywhere
            // else) closes it, so Esc can never be left without a popover to reach.
            let handle = self.popover_focus.clone();
            self.focus_out = Some(cx.on_focus_out(&handle, window, |this, _, window, cx| {
                if this.open.take().is_some() {
                    // Focus listeners run inside a draw, where a notify does not schedule
                    // another frame; notify once the draw is over.
                    cx.defer_in(window, |_, _, cx| cx.notify());
                }
            }));
        }
        let mut strip = ButtonGroup::new("toolbar-place-strip").aria_label("Place parts");
        for (index, category) in PLACE_CATEGORIES.iter().enumerate() {
            strip = strip.child(self.category_trigger(index, *category, cx));
        }
        div()
            .id("part-picker")
            .debug_selector(|| "part-picker".to_owned())
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .w(px(PART_PICKER_WIDTH))
            .h(px(PICKER_HEIGHT))
            .child(strip)
            .child(div().w(px(TOOLBAR_GROUP_GAP)).flex_none())
            .child(self.in_use_trigger(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{CanvasEdits, ScreenKind};
    use crate::theme::ActiveTheme;
    use gpui::{
        AppContext, KeyUpEvent, Keystroke, Modifiers, StyleRefinement, TestAppContext,
        VisualTestContext, size,
    };
    use omnia_doc::{Orient, Pt};
    use std::cell::RefCell;
    use std::rc::Rc;

    const GRID: i32 = 1_270_000;

    fn ready_state(place: bool) -> CommandState {
        CommandState {
            screen: ScreenKind::Workspace,
            has_document: true,
            tab_count: 1,
            canvas_edits: CanvasEdits {
                place,
                ..CanvasEdits::default()
            },
            ..CommandState::default()
        }
    }

    fn place(doc: &mut Doc, library: &SymbolLibrary, symbol: SymbolId, slot: i32) {
        doc.apply(library.place_command(
            symbol,
            Pt {
                x: slot * 20 * GRID,
                y: 0,
            },
            Orient::IDENTITY,
            format!("X{slot}").into(),
            "v".into(),
        ))
        .expect("place part");
    }

    #[test]
    fn catalog_names_every_builtin_symbol_once_in_a_strip_category() {
        let library = SymbolLibrary::builtin();
        for def in library.iter() {
            let rows = SYMBOL_CATALOG
                .iter()
                .filter(|(name, _, _)| *name == def.name)
                .count();
            assert_eq!(rows, 1, "{} needs exactly one catalog row", def.name);
        }
        for (name, category, _) in SYMBOL_CATALOG {
            assert!(
                library.by_name(name).is_some(),
                "{name} is not in the library"
            );
            assert!(
                PLACE_CATEGORIES.iter().any(|c| c.id == *category),
                "{name}: unknown category {category}"
            );
        }
        let menu = part_menu(&library);
        assert_eq!(menu.len(), PLACE_CATEGORIES.len());
        let listed: usize = menu.iter().map(|group| group.entries.len()).sum();
        assert_eq!(listed, library.len(), "every symbol listed exactly once");
        let basic = &menu[1];
        assert_eq!(basic.category.id, "basic");
        let labels: Vec<_> = basic.entries.iter().map(|e| e.label).collect();
        assert_eq!(labels, ["Capacitor", "Potentiometer", "Resistor"]);
        let empty: Vec<_> = menu
            .iter()
            .filter(|group| group.entries.is_empty())
            .map(|group| group.category.id)
            .collect();
        assert_eq!(
            empty,
            [
                "transistors",
                "logic",
                "mcu",
                "indicators",
                "power",
                "misc",
                "electromech",
                "switches"
            ]
        );
    }

    #[test]
    fn in_use_counts_match_the_document_in_any_insertion_order() {
        let library = SymbolLibrary::builtin();
        let resistor = library.by_name("Resistor-IEEE-Standard").unwrap();
        let capacitor = library.by_name("Capacitor-IEEE-NonPolarized").unwrap();
        let ground = library.by_name("Ground-COM-General").unwrap();
        let order_a = [resistor, capacitor, resistor, ground, resistor, capacitor];
        let mut order_b = order_a;
        order_b.reverse();

        let build = |order: &[SymbolId]| {
            let mut doc = Doc::new();
            for (slot, symbol) in order.iter().enumerate() {
                place(&mut doc, &library, *symbol, slot as i32);
            }
            doc
        };
        let doc_a = build(&order_a);
        let doc_b = build(&order_b);
        let entries = in_use_entries(&doc_a, &library);
        assert_eq!(
            entries,
            in_use_entries(&doc_b, &library),
            "order independent"
        );
        assert_eq!(
            entries,
            [
                InUseEntry {
                    symbol: capacitor,
                    label: "Capacitor",
                    count: 2
                },
                InUseEntry {
                    symbol: ground,
                    label: "Ground",
                    count: 1
                },
                InUseEntry {
                    symbol: resistor,
                    label: "Resistor",
                    count: 3
                },
            ]
        );
        let sum: usize = entries.iter().map(|e| e.count).sum();
        assert_eq!(sum, doc_a.part_count());
        assert!(in_use_entries(&Doc::new(), &library).is_empty());
        assert_eq!(in_use_label(0), "In use · 0 parts");
        assert_eq!(in_use_label(1), "In use · 1 part");
        assert_eq!(in_use_label(11), "In use · 11 parts");
    }

    struct SiblingView {
        renders: u64,
    }

    impl Render for SiblingView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            div()
                .size_full()
                .bg(theme::color(theme::theme(cx).stage_paper))
        }
    }

    struct RootView {
        picker: Entity<PartPicker>,
        sibling: Entity<SiblingView>,
        placed: Rc<RefCell<Vec<SymbolId>>>,
        renders: u64,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.renders += 1;
            let placed = self.placed.clone();
            div()
                .size_full()
                .flex()
                .flex_col()
                .bg(theme::color(theme::theme(cx).bg_backdrop))
                .on_action(move |action: &PlacePart, _, _| {
                    placed.borrow_mut().push(action.symbol);
                })
                .child(
                    self.picker.clone().cached(
                        StyleRefinement::default()
                            .flex_none()
                            .w(px(PART_PICKER_WIDTH))
                            .h(px(PICKER_HEIGHT)),
                    ),
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
        picker: Entity<PartPicker>,
        state: Entity<CommandState>,
        sibling: Entity<SiblingView>,
        placed: Rc<RefCell<Vec<SymbolId>>>,
        library: Arc<SymbolLibrary>,
    }

    fn harness(cx: &mut TestAppContext, place: bool) -> (Harness, &mut VisualTestContext) {
        cx.update(ActiveTheme::install);
        let library = SymbolLibrary::builtin();
        let placed = Rc::new(RefCell::new(Vec::new()));
        let in_view = placed.clone();
        let lib = library.clone();
        let (root, cx) = cx.add_window_view(move |_, cx| {
            let state = cx.new(|_| ready_state(place));
            let picker = cx.new(|cx| PartPicker::new(state, lib, cx));
            let sibling = cx.new(|_| SiblingView { renders: 0 });
            RootView {
                picker,
                sibling,
                placed: in_view,
                renders: 0,
            }
        });
        cx.simulate_resize(size(px(1440.0), px(900.0)));
        cx.run_until_parked();
        let (picker, sibling) = cx.update(|_, cx| {
            let view = root.read(cx);
            (view.picker.clone(), view.sibling.clone())
        });
        let state = cx.update(|_, cx| picker.read(cx).state.clone());
        (
            Harness {
                root,
                picker,
                state,
                sibling,
                placed,
                library,
            },
            cx,
        )
    }

    fn open(cx: &mut VisualTestContext, h: &Harness) -> Option<PickerPopover> {
        cx.update(|_, cx| h.picker.read(cx).open_popover())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let center = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is not drawn"))
            .center();
        cx.simulate_click(center, Modifiers::none());
        cx.run_until_parked();
    }

    fn press_key(cx: &mut VisualTestContext, key: &'static str) {
        let keystroke = Keystroke::parse(key).unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(KeyUpEvent { keystroke });
        cx.run_until_parked();
    }

    fn focus_trigger(cx: &mut VisualTestContext, h: &Harness, popover: PickerPopover) {
        let handle = cx.update(|_, cx| h.picker.read(cx).trigger_focus(popover).clone());
        cx.update(|window, cx| window.focus(&handle, cx));
        cx.run_until_parked();
    }

    fn trigger_focused(cx: &mut VisualTestContext, h: &Harness, popover: PickerPopover) -> bool {
        let handle = cx.update(|_, cx| h.picker.read(cx).trigger_focus(popover).clone());
        cx.update(|window, _| handle.is_focused(window))
    }

    fn renders(cx: &mut VisualTestContext, h: &Harness) -> (u64, u64, u64) {
        cx.update(|_, cx| {
            (
                h.picker.read(cx).render_count(),
                h.root.read(cx).renders,
                h.sibling.read(cx).renders,
            )
        })
    }

    /// Debug selectors of the trigger and popover of each strip category.
    const SELECTORS: [(&str, &str); 12] = [
        ("toolbar-place-sources", "toolbar-place-sources-popover"),
        ("toolbar-place-basic", "toolbar-place-basic-popover"),
        ("toolbar-place-diodes", "toolbar-place-diodes-popover"),
        (
            "toolbar-place-transistors",
            "toolbar-place-transistors-popover",
        ),
        ("toolbar-place-analog", "toolbar-place-analog-popover"),
        ("toolbar-place-logic", "toolbar-place-logic-popover"),
        ("toolbar-place-mcu", "toolbar-place-mcu-popover"),
        (
            "toolbar-place-indicators",
            "toolbar-place-indicators-popover",
        ),
        ("toolbar-place-power", "toolbar-place-power-popover"),
        ("toolbar-place-misc", "toolbar-place-misc-popover"),
        (
            "toolbar-place-electromech",
            "toolbar-place-electromech-popover",
        ),
        ("toolbar-place-switches", "toolbar-place-switches-popover"),
    ];

    /// Debug selector of a symbol row in a category popover.
    fn row_selector(symbol: SymbolId) -> &'static str {
        const ROWS: [&str; 8] = [
            "picker-place-0",
            "picker-place-1",
            "picker-place-2",
            "picker-place-3",
            "picker-place-4",
            "picker-place-5",
            "picker-place-6",
            "picker-place-7",
        ];
        ROWS[symbol.index()]
    }

    #[gpui::test]
    fn each_category_popover_opens_by_mouse_and_lists_its_symbols(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, false);
        let menu = part_menu(&h.library);
        for (index, (trigger, popover)) in SELECTORS.iter().enumerate() {
            let entries = &menu[index].entries;
            click(cx, trigger);
            if entries.is_empty() {
                // An empty category is a disabled button (U4.2).
                assert_eq!(open(cx, &h), None, "{trigger} is empty and must not open");
                assert!(cx.debug_bounds(popover).is_none());
                continue;
            }
            assert_eq!(
                open(cx, &h),
                Some(PickerPopover::Category(index)),
                "{trigger}"
            );
            let panel = cx.debug_bounds(popover).expect(popover);
            let button = cx.debug_bounds(trigger).unwrap();
            assert_eq!(
                panel.origin.x, button.origin.x,
                "{popover} sits under its button"
            );
            assert!(panel.origin.y >= button.origin.y + button.size.height);
            // The popover lists exactly the Place > Part entries, in that order.
            let mut last_y = panel.origin.y;
            for entry in entries {
                let row = cx
                    .debug_bounds(row_selector(entry.symbol))
                    .expect("row drawn");
                assert!(row.origin.y > last_y, "rows in menu order");
                last_y = row.origin.y;
            }
            for symbol in h.library.iter().map(|def| def.id) {
                if !entries.iter().any(|e| e.symbol == symbol) {
                    assert!(cx.debug_bounds(row_selector(symbol)).is_none());
                }
            }
            // A second press on the same button closes it without reopening.
            click(cx, trigger);
            assert_eq!(open(cx, &h), None, "{trigger} second press closes");
            assert!(cx.debug_bounds(popover).is_none());
        }

        // Opening one category while another is open switches to it.
        click(cx, "toolbar-place-basic");
        click(cx, "toolbar-place-analog");
        assert_eq!(open(cx, &h), Some(PickerPopover::Category(4)));
        // Clicking elsewhere closes it; the next press on its button opens it again.
        cx.simulate_click(point(px(700.0), px(600.0)), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(open(cx, &h), None);
        click(cx, "toolbar-place-analog");
        assert_eq!(open(cx, &h), Some(PickerPopover::Category(4)));

        // Hover traffic over an open popover and the strip re-renders nothing but the
        // picker's own subtree: the cached sheet stand-in is replayed (UD4, UINV2).
        // Clicks are different: GPUI focuses a focusable element on mouse-down and
        // `Window::focus` calls `refresh()`, which re-renders every cached view. That is
        // a GPUI fact logged for the owner (U4.2 log entry), not something the picker
        // can avoid while popovers take focus for Esc and Tab (UINV12).
        let sibling_before = renders(cx, &h).2;
        let rows: Vec<_> = menu[4]
            .entries
            .iter()
            .map(|entry| {
                cx.debug_bounds(row_selector(entry.symbol))
                    .unwrap()
                    .center()
            })
            .collect();
        let strip: Vec<_> = SELECTORS
            .iter()
            .map(|(trigger, _)| cx.debug_bounds(trigger).unwrap().center())
            .collect();
        for position in rows.iter().chain(strip.iter()).chain(rows.iter()) {
            cx.simulate_mouse_move(*position, None, Modifiers::none());
            cx.run_until_parked();
        }
        assert_eq!(open(cx, &h), Some(PickerPopover::Category(4)));
        assert_eq!(
            renders(cx, &h).2,
            sibling_before,
            "hover never re-renders the sheet stand-in"
        );
    }

    #[gpui::test]
    fn each_popover_opens_and_closes_by_keyboard(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, false);
        let menu = part_menu(&h.library);
        for (index, group) in menu.iter().enumerate() {
            if group.entries.is_empty() {
                continue;
            }
            let popover = PickerPopover::Category(index);
            focus_trigger(cx, &h, popover);
            press_key(cx, "enter");
            assert_eq!(
                open(cx, &h),
                Some(popover),
                "Enter opens {}",
                group.category.id
            );
            assert!(cx.debug_bounds(SELECTORS[index].1).is_some());
            press_key(cx, "escape");
            assert_eq!(open(cx, &h), None, "Esc closes {}", group.category.id);
            assert!(
                trigger_focused(cx, &h, popover),
                "Esc returns focus to the button"
            );
            press_key(cx, "space");
            assert_eq!(
                open(cx, &h),
                Some(popover),
                "Space opens {}",
                group.category.id
            );
            press_key(cx, "escape");
        }

        // The "In use" picker, once the document has parts.
        let mut doc = Doc::new();
        let resistor = h.library.by_name("Resistor-IEEE-Standard").unwrap();
        place(&mut doc, &h.library, resistor, 0);
        let library = h.library.clone();
        cx.update(|_, cx| {
            h.picker
                .update(cx, |picker, cx| picker.follow_document(&doc, &library, cx))
        });
        cx.run_until_parked();
        focus_trigger(cx, &h, PickerPopover::InUse);
        press_key(cx, "enter");
        assert_eq!(open(cx, &h), Some(PickerPopover::InUse));
        assert!(cx.debug_bounds("toolbar-in-use-popover").is_some());
        press_key(cx, "escape");
        assert_eq!(open(cx, &h), None);
        assert!(trigger_focused(cx, &h, PickerPopover::InUse));
    }

    #[gpui::test]
    fn in_use_picker_lists_the_document_with_counts(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, false);
        // Empty document: "0 parts", disabled, does not open.
        click(cx, "toolbar-in-use");
        assert_eq!(open(cx, &h), None);

        let resistor = h.library.by_name("Resistor-IEEE-Standard").unwrap();
        let opamp = h.library.by_name("IC-COM-OpAmp").unwrap();
        let mut doc = Doc::new();
        for (slot, symbol) in [resistor, opamp, resistor, resistor]
            .into_iter()
            .enumerate()
        {
            place(&mut doc, &h.library, symbol, slot as i32);
        }
        let library = h.library.clone();
        let (picker_before, _, sibling_before) = renders(cx, &h);
        cx.update(|_, cx| {
            h.picker
                .update(cx, |picker, cx| picker.follow_document(&doc, &library, cx))
        });
        cx.run_until_parked();
        let (picker_after, _, sibling_after) = renders(cx, &h);
        assert_eq!(
            picker_after - picker_before,
            1,
            "a count change renders the picker once"
        );
        assert_eq!(
            sibling_after, sibling_before,
            "sheet stand-in never re-renders"
        );
        // Following the same document again changes nothing and renders nothing.
        cx.update(|_, cx| {
            h.picker
                .update(cx, |picker, cx| picker.follow_document(&doc, &library, cx))
        });
        cx.run_until_parked();
        assert_eq!(renders(cx, &h).0, picker_after);

        let (total, entries) = cx.update(|_, cx| {
            let picker = h.picker.read(cx);
            (picker.in_use_total(), picker.in_use().to_vec())
        });
        assert_eq!(total, doc.part_count());
        assert_eq!(entries, in_use_entries(&doc, &h.library));

        click(cx, "toolbar-in-use");
        assert_eq!(open(cx, &h), Some(PickerPopover::InUse));
        let opamp_row = cx.debug_bounds("picker-in-use-3").expect("op-amp row");
        let resistor_row = cx.debug_bounds("picker-in-use-5").expect("resistor row");
        assert!(
            opamp_row.origin.y < resistor_row.origin.y,
            "by name: Op-amp, Resistor"
        );
        assert!(
            cx.debug_bounds("picker-in-use-0").is_none(),
            "no capacitor placed"
        );
        click(cx, "toolbar-in-use");
        assert_eq!(open(cx, &h), None);
    }

    #[gpui::test]
    fn choosing_with_place_off_changes_nothing(cx: &mut TestAppContext) {
        // UINV5: `PlacePart` is enabled by `CanvasEdits::place`, off until U5.2.
        let (h, cx) = harness(cx, false);
        click(cx, "toolbar-place-basic");
        let resistor = h.library.by_name("Resistor-IEEE-Standard").unwrap();
        click(cx, row_selector(resistor));
        assert!(h.placed.borrow().is_empty(), "no PlacePart dispatched");
        assert_eq!(
            open(cx, &h),
            Some(PickerPopover::Category(1)),
            "popover unchanged"
        );
        // Even a direct call re-checks `enabled` first.
        cx.update(|window, cx| {
            h.picker
                .update(cx, |picker, cx| picker.choose(resistor, window, cx))
        });
        cx.run_until_parked();
        assert!(h.placed.borrow().is_empty());
    }

    #[gpui::test]
    fn choosing_with_place_on_dispatches_place_part_and_closes(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, true);
        let opamp = h.library.by_name("IC-COM-OpAmp").unwrap();
        click(cx, "toolbar-place-analog");
        click(cx, row_selector(opamp));
        assert_eq!(*h.placed.borrow(), [opamp]);
        assert_eq!(open(cx, &h), None, "choosing closes the popover");

        // By keyboard: open, Tab to the first row, Enter.
        let ground = h.library.by_name("Ground-COM-General").unwrap();
        focus_trigger(cx, &h, PickerPopover::Category(0));
        press_key(cx, "enter");
        assert_eq!(open(cx, &h), Some(PickerPopover::Category(0)));
        cx.update(|window, cx| window.focus_next(cx));
        press_key(cx, "enter");
        assert_eq!(*h.placed.borrow(), [opamp, ground]);
        assert_eq!(open(cx, &h), None);
        assert!(trigger_focused(cx, &h, PickerPopover::Category(0)));
    }

    #[gpui::test]
    fn picker_renders_only_when_the_place_flag_flips(cx: &mut TestAppContext) {
        let (h, cx) = harness(cx, false);
        type Change = fn(&mut CommandState);
        // Closed: the place flag is not drawn anywhere, so nothing renders.
        // Open: it is drawn on every row, so each flip renders once.
        let steps: [(&str, Option<&'static str>, Change, u64); 9] = [
            ("can_undo", None, |s| s.can_undo = true, 0),
            ("place on, closed", None, |s| s.canvas_edits.place = true, 0),
            (
                "place off, closed",
                None,
                |s| s.canvas_edits.place = false,
                0,
            ),
            (
                "full_screen, open",
                Some("toolbar-place-basic"),
                |s| s.full_screen = true,
                0,
            ),
            (
                "canvas_edits.delete, open",
                None,
                |s| s.canvas_edits.delete = true,
                0,
            ),
            ("place on, open", None, |s| s.canvas_edits.place = true, 1),
            (
                "loading, open (place disabled)",
                None,
                |s| s.loading = true,
                1,
            ),
            ("loading off, open", None, |s| s.loading = false, 1),
            (
                "place on again, open",
                None,
                |s| s.canvas_edits.place = true,
                0,
            ),
        ];
        for (name, open_first, change, expected) in steps {
            if let Some(trigger) = open_first {
                click(cx, trigger);
                assert_eq!(open(cx, &h), Some(PickerPopover::Category(1)));
            }
            let before = renders(cx, &h).0;
            cx.update(|_, cx| {
                h.state.update(cx, |state, cx| {
                    change(state);
                    cx.notify();
                })
            });
            cx.run_until_parked();
            assert_eq!(renders(cx, &h).0 - before, expected, "{name}");
        }
    }

    #[gpui::test]
    fn closing_by_a_second_press_leaves_focus_on_the_button(cx: &mut TestAppContext) {
        // Review of #96: the press that closes a popover through click-outside must leave
        // focus on its button, so Enter reopens it.
        let (h, cx) = harness(cx, false);
        let mut doc = Doc::new();
        let resistor = h.library.by_name("Resistor-IEEE-Standard").unwrap();
        place(&mut doc, &h.library, resistor, 0);
        let library = h.library.clone();
        cx.update(|_, cx| {
            h.picker
                .update(cx, |picker, cx| picker.follow_document(&doc, &library, cx))
        });
        cx.run_until_parked();
        for (trigger, popover) in [
            ("toolbar-in-use", PickerPopover::InUse),
            ("toolbar-place-basic", PickerPopover::Category(1)),
        ] {
            click(cx, trigger);
            assert_eq!(open(cx, &h), Some(popover), "{trigger} opens");
            click(cx, trigger);
            assert_eq!(open(cx, &h), None, "{trigger} second press closes");
            assert!(trigger_focused(cx, &h, popover), "{trigger} keeps focus");
            press_key(cx, "enter");
            assert_eq!(open(cx, &h), Some(popover), "{trigger}: Enter reopens");
            press_key(cx, "escape");
            assert_eq!(open(cx, &h), None);
        }
    }

    #[gpui::test]
    fn focus_leaving_an_open_popover_closes_it(cx: &mut TestAppContext) {
        // Review of #96: with placing off every row is disabled, so Tab leaves the panel;
        // the popover must not stay open where Esc can no longer reach it.
        let (h, cx) = harness(cx, false);
        // GPUI dispatches focus listeners only for an active window.
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        focus_trigger(cx, &h, PickerPopover::Category(1));
        press_key(cx, "enter");
        assert_eq!(open(cx, &h), Some(PickerPopover::Category(1)));
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        assert_eq!(open(cx, &h), None, "focus left, popover closed");
        assert!(cx.debug_bounds("toolbar-place-basic-popover").is_none());
    }
}
