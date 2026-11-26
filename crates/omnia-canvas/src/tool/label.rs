//! T7.2: net-label placement and inline GPUI text editor (`D2`, `INV4`, `INV5`, `INV6`, `INV10`,
//! `INV11`, `B10`, `C4`, `K14`, `WD5`, `WD9`, `UD3`).
//!
//! - [`Tool::Label`](super::Tool::Label) targets an existing label (within K14 hit tolerance)
//!   or a wire/pin snap target (`Snap::Pin`, `Snap::WireEnd`, `Snap::Junction`, `Snap::WireBody`)
//!   at K14/WK4 `snap_radius`, never placing on empty `Snap::Grid` space.
//! - Hovering in `Tool::Label` runs [`LabelGesture::update_cursor`], which allocates nothing once
//!   `ToolMachine`'s scratch buffer has capacity (`B10`) and never notifies `SheetView` (`INV11`).
//! - Clicking a valid target opens an inline [`LabelEditor`] anchored at the target point with key
//!   context [`TEXT_FIELD_CONTEXT`] (`"TextField"`), keeping `Sheet` shortcuts inactive while typing
//!   (`UD3`), supporting GPUI `EntityInputHandler` (text insertion, UTF-16 ranges, IME marked text,
//!   caret navigation, clipboard copy/cut/paste, pointer selection), committing via one
//!   [`Command::AddLabel`] (`WD9` wire-body split) or one [`Command::SetText`], and cancelling
//!   cleanly on `Esc`, empty/whitespace text, unchanged existing text, or `OverlayView::end_gesture`
//!   (`INV5`, `C4`).

use crate::{
    overlay::gpui_color,
    tool::{DocStamp, PointerRadii},
};
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Font,
    FontWeight, GlobalElementId, Hsla, InputPreference, InspectorElementId, Keystroke, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Role,
    ShapedLine, SharedString, Style, Subscription, TextAlign, TextRun, UTF16Selection, Window, div,
    fill, point, prelude::*, px, size,
};
use omnia_doc::{
    Command, Doc, GRID_NM, ItemId, Label, LabelId, Pt, Rect, SHEET_LIMIT_NM, Snap,
    command::TextField, snap,
};
use omnia_theme::Theme;
use std::{borrow::Cow, ops::Range, sync::Arc};

/// Key context active while editing a net label inline (`ui-plan.md` UD3).
pub const TEXT_FIELD_CONTEXT: &str = "TextField";

/// Minimum and maximum logical widths of the inline label editor's inner text slot.
const MIN_TEXT_WIDTH_PX: f32 = 76.0;
const MAX_TEXT_WIDTH_PX: f32 = 208.0;
/// Horizontal padding inside the inline editor pill.
const PILL_PAD_X_PX: f32 = 6.0;
const PILL_PAD_Y_PX: f32 = 2.0;
/// K8: net label font size at 100% zoom, in logical pixels.
const NET_LABEL_FONT_PX: f32 = 13.0;

#[inline]
fn clamp_in_bounds(p: Pt) -> Pt {
    Pt {
        x: p.x.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
        y: p.y.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
    }
}

/// Returns `true` when `target` is a valid wire or pin target for net-label placement (WD5, WD9).
///
/// Net labels never attach to empty `Snap::Grid` space.
#[inline]
pub fn is_valid_label_snap(target: Snap) -> bool {
    matches!(
        target,
        Snap::Pin { .. } | Snap::WireEnd { .. } | Snap::Junction { .. } | Snap::WireBody { .. }
    )
}

/// Hover/click target resolved by `Tool::Label` under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelTarget {
    /// An existing net label within K14 hit tolerance: clicking edits its text (`SetText`).
    ExistingLabel { id: LabelId, at: Pt },
    /// A wire or pin attachment target (`Pin`, `WireEnd`, `Junction`, `WireBody`): clicking adds a label.
    Attach(Snap),
}

impl LabelTarget {
    /// World coordinate anchored by this target.
    #[inline]
    pub fn at(self) -> Pt {
        match self {
            Self::ExistingLabel { at, .. } => at,
            Self::Attach(snap) => snap.at(),
        }
    }
}

/// Target being created or edited by an active inline [`LabelEditor`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LabelEditTarget {
    /// Placing a new net label at `at` (snapped to wire/pin).
    New { at: Pt },
    /// Editing an existing net label's text (`Command::SetText`).
    Existing {
        id: LabelId,
        at: Pt,
        original: Arc<str>,
    },
}

impl LabelEditTarget {
    /// World coordinate anchored by the label being edited.
    #[inline]
    pub fn at(&self) -> Pt {
        match self {
            Self::New { at } | Self::Existing { at, .. } => *at,
        }
    }

    /// Initial text loaded into the inline editor when opened.
    #[inline]
    pub fn initial_text(&self) -> &str {
        match self {
            Self::New { .. } => "",
            Self::Existing { original, .. } => original,
        }
    }
}

/// Pure `Tool::Label` state kept in [`crate::tool::ToolState::Labeling`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelGesture {
    /// Current valid wire/pin or existing-label target under the cursor.
    pub target: Option<LabelTarget>,
    /// Active inline editing target, if open from `Tool::Label`.
    pub editing: Option<LabelEditTarget>,
}

impl LabelGesture {
    /// Builds a preview gesture directly for golden rendering or programmatic inspection.
    pub fn for_preview(target: Option<LabelTarget>) -> Self {
        Self {
            target,
            editing: None,
        }
    }

    /// Active editing anchor if editing, else hovered valid target anchor.
    #[inline]
    pub fn anchor_point(&self) -> Option<Pt> {
        self.editing
            .as_ref()
            .map(LabelEditTarget::at)
            .or_else(|| self.target.map(LabelTarget::at))
    }

    /// Updates hovered target under `raw_at`. Allocates nothing once `scratch` has capacity (B10).
    pub fn update_cursor(
        &mut self,
        doc: Option<&Doc>,
        raw_at: Pt,
        radii: PointerRadii,
        scratch: &mut Vec<ItemId>,
    ) -> bool {
        let next = doc.and_then(|doc| find_label_target(doc, raw_at, radii, scratch));
        let changed = self.target != next;
        self.target = next;
        changed
    }

    /// Resolves a left click in `Tool::Label`, returning an inline edit request when over an
    /// existing label or valid wire/pin target. Empty grid clicks open no editor (`INV5`).
    pub fn click(
        &mut self,
        doc: &Doc,
        doc_key: u64,
        raw_at: Pt,
        radii: PointerRadii,
        scratch: &mut Vec<ItemId>,
    ) -> (Option<(LabelEditTarget, DocStamp)>, bool) {
        let prev_target = self.target;
        let resolved = find_label_target(doc, raw_at, radii, scratch);
        self.target = resolved;
        let preview_changed = self.target != prev_target;
        let Some(target) = resolved else {
            return (None, preview_changed);
        };
        let edit = match target {
            LabelTarget::ExistingLabel { id, at } => {
                let Some(label) = doc.label(id) else {
                    return (None, preview_changed);
                };
                LabelEditTarget::Existing {
                    id,
                    at,
                    original: Arc::clone(&label.text),
                }
            }
            LabelTarget::Attach(snap) => LabelEditTarget::New { at: snap.at() },
        };
        self.editing = Some(edit.clone());
        let base = DocStamp {
            key: doc_key,
            revision: doc.revision(),
        };
        (Some((edit, base)), true)
    }

    /// Clears `editing` when inline editing finishes or cancels.
    pub fn clear_editing(&mut self) -> bool {
        self.editing.take().is_some()
    }
}

/// Finds the nearest existing label within `hit_tol` of `raw_at`, breaking ties by lowest `LabelId`.
///
/// Checked before wire/pin snapping so clicking a label sitting on a wire edits that label rather
/// than opening new-label placement (`omnia_doc::hit_test` breaks ties in favour of parts/wires).
/// Allocates nothing once `scratch` holds the query result (`B10`).
pub fn find_existing_label(
    doc: &Doc,
    raw_at: Pt,
    hit_tol: i32,
    scratch: &mut Vec<ItemId>,
) -> Option<(LabelId, Pt)> {
    let at = clamp_in_bounds(raw_at);
    let r = i64::from(hit_tol.max(0));
    let limit = i64::from(SHEET_LIMIT_NM);
    let coord = |v: i32, delta: i64| (i64::from(v) + delta).clamp(-limit, limit) as i32;
    let area = Rect {
        min: Pt {
            x: coord(at.x, -r),
            y: coord(at.y, -r),
        },
        max: Pt {
            x: coord(at.x, r),
            y: coord(at.y, r),
        },
    };
    let max_dist_sq = r * r;
    doc.query(area, scratch);
    scratch
        .iter()
        .filter_map(|&item| match item {
            ItemId::Label(id) => {
                let pos = doc.label(id)?.at;
                let dist_sq = at.distance_sq(pos);
                (dist_sq <= max_dist_sq).then_some((dist_sq, id.0, (id, pos)))
            }
            ItemId::Part(_) | ItemId::Wire(_) => None,
        })
        .min_by_key(|&(dist_sq, id_raw, _)| (dist_sq, id_raw))
        .map(|(_, _, found)| found)
}

/// Resolves the `Tool::Label` target at `raw_at`: existing label within `radii.hit`, else non-Grid
/// WD5 snap target within `radii.snap`. Allocates nothing once `scratch` has capacity (`B10`).
pub fn find_label_target(
    doc: &Doc,
    raw_at: Pt,
    radii: PointerRadii,
    scratch: &mut Vec<ItemId>,
) -> Option<LabelTarget> {
    if let Some((id, at)) = find_existing_label(doc, raw_at, radii.hit, scratch) {
        return Some(LabelTarget::ExistingLabel { id, at });
    }
    let snapped = snap(
        doc,
        clamp_in_bounds(raw_at),
        GRID_NM,
        radii.snap.max(0),
        scratch,
    );
    if !is_valid_label_snap(snapped) {
        return None;
    }
    if let Some((id, at)) = find_existing_label(doc, snapped.at(), 0, scratch) {
        return Some(LabelTarget::ExistingLabel { id, at });
    }
    Some(LabelTarget::Attach(snapped))
}

/// Converts committed inline editor text into one [`Command`], or `None` when cancelled (`INV5`).
///
/// Empty/whitespace-only input and unchanged existing-label text both return `None` so no
/// redundant history entry or document revision increment occurs.
pub fn label_commit_command(target: &LabelEditTarget, raw_text: &str) -> Option<Command> {
    let trimmed = raw_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    match target {
        LabelEditTarget::New { at } => Some(Command::AddLabel(Label {
            at: *at,
            text: Arc::from(trimmed),
        })),
        LabelEditTarget::Existing { id, original, .. } => {
            if raw_text == original.as_ref() || trimmed == original.as_ref() {
                None
            } else {
                Some(Command::SetText {
                    item: ItemId::Label(*id),
                    field: TextField::Label,
                    text: Arc::from(trimmed),
                })
            }
        }
    }
}

/// Events emitted by [`LabelEditor`] back to [`crate::OverlayView`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LabelEditorEvent {
    Submit(Arc<str>),
    Cancel,
}

/// Single-line inline net-label editor implementing GPUI's `EntityInputHandler`.
///
/// Reuses `OverlayView`'s `FocusHandle` so opening and closing the editor keeps GPUI focus on the
/// same handle without triggering `Window::refresh()` or busting `CachedSheet` (`INV11`).
pub(crate) struct LabelEditor {
    focus: FocusHandle,
    theme: &'static Theme,
    active: bool,
    _keystrokes: Subscription,
    text: SharedString,
    placeholder: SharedString,
    anchor: usize,
    caret: usize,
    marked: Option<Range<usize>>,
    dragging: bool,
    cached: Option<CachedLine>,
    viewport: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
}

impl EventEmitter<LabelEditorEvent> for LabelEditor {}

impl LabelEditor {
    pub(crate) fn new(
        focus: FocusHandle,
        theme: &'static Theme,
        initial_text: &str,
        select_all: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let entity = cx.weak_entity();
        let focus_weak = focus.downgrade();
        let keystrokes = cx.intercept_keystrokes(move |event, window, cx| {
            // AltGr and platform character input must flow to the native text input handler.
            if event.input_preference == InputPreference::CharacterInput
                || !focus_weak.upgrade().is_some_and(|f| f.is_focused(window))
            {
                return;
            }
            let _ = entity.update(cx, |editor, cx| {
                if editor.active && editor.edit_keystroke(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            });
        });

        let clean = single_line(initial_text);
        let text: SharedString = clean.into_owned().into();
        let len = text.len();
        let (anchor, caret) = if select_all && len > 0 {
            (0, len)
        } else {
            (len, len)
        };

        Self {
            focus,
            theme,
            active: true,
            _keystrokes: keystrokes,
            text,
            placeholder: SharedString::new_static("Net"),
            anchor,
            caret,
            marked: None,
            dragging: false,
            cached: None,
            viewport: None,
            scroll_x: px(0.0),
        }
    }

    pub(crate) fn deactivate(&mut self) {
        self.active = false;
        self.dragging = false;
    }

    pub(crate) fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        if std::ptr::eq(self.theme, theme) {
            return;
        }
        self.theme = theme;
        self.cached = None;
        cx.notify();
    }

    pub(crate) fn shared_text(&self) -> SharedString {
        self.text.clone()
    }

    fn selection(&self) -> Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }

    fn set_selection(
        &mut self,
        anchor: usize,
        caret: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if (self.anchor, self.caret) != (anchor, caret) {
            self.anchor = anchor;
            self.caret = caret;
            window.notify_text_selection_changed(&self.focus);
            cx.notify();
        }
    }

    fn input_changed(&self, window: &Window, cx: &mut Context<Self>) {
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    fn edit_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = keystroke.modifiers;
        let command = modifiers.control && !modifiers.platform
            || cfg!(target_os = "macos") && modifiers.platform && !modifiers.control;
        let plain = !modifiers.control && !modifiers.platform;
        if modifiers.alt || modifiers.function || !(plain || command) {
            return false;
        }
        let key = keystroke.key.as_str();
        match (command, modifiers.shift, key) {
            (true, false, "a") => self.set_selection(0, self.text.len(), window, cx),
            (true, false, "c" | "x") => {
                let selection = self.selection();
                if !selection.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        self.text[selection.clone()].into(),
                    ));
                    if key == "x" {
                        self.commit(selection, "", window, cx);
                    }
                }
            }
            (true, false, "v") => {
                if let Some(item) = cx.read_from_clipboard() {
                    self.paste(item, window, cx);
                }
            }
            // Swallow Undo/Redo and modified delete/backspace while inline editing is open (`UD3`).
            (true, _, "z" | "y" | "backspace" | "delete") => {}
            (false, extend, "left" | "right" | "home" | "end") => {
                self.unmark_text(window, cx);
                let selection = self.selection();
                let offset = match key {
                    "home" => 0,
                    "end" => self.text.len(),
                    "left" if !extend && !selection.is_empty() => selection.start,
                    "right" if !extend && !selection.is_empty() => selection.end,
                    "left" => self.previous(),
                    _ => self.next(),
                };
                let anchor = if extend { self.anchor } else { offset };
                self.set_selection(anchor, offset, window, cx);
            }
            (false, _, "backspace" | "delete") => {
                let mut range = self.replacement_range(None);
                if range.is_empty() {
                    range = if key == "backspace" {
                        self.previous()..self.caret
                    } else {
                        self.caret..self.next()
                    };
                }
                self.commit(range, "", window, cx);
            }
            (false, false, "enter") => {
                self.unmark_text(window, cx);
                self.deactivate();
                cx.emit(LabelEditorEvent::Submit(Arc::from(self.text.as_ref())));
            }
            (false, false, "escape") => {
                self.deactivate();
                cx.emit(LabelEditorEvent::Cancel);
            }
            _ => return false,
        }
        true
    }

    fn previous(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(offset, _)| offset)
    }

    fn next(&self) -> usize {
        self.caret
            + self.text[self.caret..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        self.focus.focus(window, cx);
        window.request_virtual_keyboard();
        self.unmark_text(window, cx);
        self.dragging = true;
        let offset = self.index_at_x(event.position.x);
        if event.modifiers.shift {
            self.set_selection(self.anchor, offset, window, cx);
        } else if event.click_count == 2 {
            let range = word_at(&self.text, offset);
            self.set_selection(range.start, range.end, window, cx);
        } else {
            self.set_selection(offset, offset, window, cx);
        }
        cx.stop_propagation();
    }

    fn index_at_x(&self, x: Pixels) -> usize {
        if self.text.is_empty() {
            return 0;
        }
        match (&self.cached, self.viewport) {
            (Some(cache), Some(bounds)) => self.text.floor_char_boundary(
                cache
                    .line
                    .closest_index_for_x(x - bounds.left() + self.scroll_x),
            ),
            _ => self.caret,
        }
    }

    fn replacement_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|range| self.byte_range(range))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection())
    }

    fn byte_range(&self, range: Range<usize>) -> Range<usize> {
        let start = utf16_to_byte(&self.text, range.start);
        start..utf16_to_byte(&self.text, range.end).max(start)
    }

    fn to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.text[..range.start].encode_utf16().count()
            ..self.text[..range.end].encode_utf16().count()
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> bool {
        let changed = self.text[range.clone()] != *text;
        if changed {
            let mut value = String::with_capacity(self.text.len() - range.len() + text.len());
            value.push_str(&self.text[..range.start]);
            value.push_str(text);
            value.push_str(&self.text[range.end..]);
            self.text = value.into();
            self.cached = None;
        }
        self.caret = range.start + text.len();
        self.anchor = self.caret;
        changed
    }

    fn commit(&mut self, range: Range<usize>, text: &str, window: &Window, cx: &mut Context<Self>) {
        let was_marked = self.marked.take().is_some();
        let previous_selection = (self.anchor, self.caret);
        if self.replace(range, text) || was_marked {
            self.input_changed(window, cx);
        } else if previous_selection != (self.anchor, self.caret) {
            window.notify_text_selection_changed(&self.focus);
            cx.notify();
        }
    }

    fn ensure_line(&mut self, window: &Window) -> &ShapedLine {
        let style = window.text_style();
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let color = gpui_color(if self.text.is_empty() {
            self.theme.stage_ink_secondary
        } else {
            self.theme.stage_ink
        });
        let reuse = self.cached.as_ref().is_some_and(|cached| {
            cached.font == font && cached.font_size == font_size && cached.color == color
        });
        if !reuse {
            let display = if self.text.is_empty() {
                self.placeholder.clone()
            } else {
                self.text.clone()
            };
            let run = TextRun {
                len: display.len(),
                font: font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window
                .text_system()
                .shape_line(display, font_size, &[run], None);
            self.cached = Some(CachedLine {
                font,
                font_size,
                color,
                line,
            });
        }
        &self.cached.as_ref().expect("line cache was populated").line
    }

    fn layout_size(&mut self, window: &Window) -> (Pixels, Pixels) {
        let is_empty = self.text.is_empty();
        let line = self.ensure_line(window);
        let text_w = if is_empty {
            px(MIN_TEXT_WIDTH_PX)
        } else {
            (line.width() + px(4.0)).clamp(px(MIN_TEXT_WIDTH_PX), px(MAX_TEXT_WIDTH_PX))
        };
        (text_w, window.line_height())
    }

    fn prepaint_line(&mut self, bounds: Bounds<Pixels>, window: &Window) -> LinePaint {
        let _ = self.ensure_line(window);
        let line = &self.cached.as_ref().expect("line cache was populated").line;
        let focused = self.focus.is_focused(window);
        let caret_x = line.x_for_index(self.caret);
        let extent = if self.text.is_empty() {
            px(0.0)
        } else {
            line.width() + px(1.0)
        };
        let left = (caret_x + px(1.0) - bounds.size.width).max(px(0.0));
        self.scroll_x = self
            .scroll_x
            .max(left)
            .min(caret_x)
            .min((extent - bounds.size.width).max(px(0.0)));
        self.viewport = Some(bounds);
        let selection = self.selection();
        let rect = |range| range_bounds(bounds, line, range, self.scroll_x);
        let caret_x = bounds.left() + caret_x - self.scroll_x;
        let ink = gpui_color(self.theme.stage_ink);
        let halo = gpui_color(self.theme.stage_select_halo).opacity(0.32);
        LinePaint {
            line: line.clone(),
            origin: point(bounds.left() - self.scroll_x, bounds.top()),
            selection: (focused && !selection.is_empty()).then(|| fill(rect(selection), halo)),
            caret: (focused && self.anchor == self.caret).then(|| {
                fill(
                    Bounds::new(
                        point(caret_x, bounds.top()),
                        size(px(1.0), bounds.size.height),
                    ),
                    ink,
                )
            }),
            marked: self.marked.clone().filter(|_| focused).map(|range| {
                let area = rect(range);
                fill(
                    Bounds::new(
                        point(area.left(), area.bottom() - px(1.0)),
                        size(area.size.width, px(1.0)),
                    ),
                    ink,
                )
            }),
        }
    }
}

fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut consumed = 0;
    for (byte, ch) in text.char_indices() {
        consumed += ch.len_utf16();
        if consumed > offset {
            return byte;
        }
    }
    text.len()
}

fn single_line(text: &str) -> Cow<'_, str> {
    // Each substitute preserves the UTF-16 length of the replaced separator.
    const BREAKS: [char; 7] = [
        '\n', '\r', '\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}',
    ];
    if text.contains(BREAKS) {
        Cow::Owned(text.replace(BREAKS, " "))
    } else {
        Cow::Borrowed(text)
    }
}

fn word_at(text: &str, offset: usize) -> Range<usize> {
    fn word(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }

    let offset = if text[offset..].starts_with(word) {
        offset
    } else if let Some((previous, ch)) = text[..offset].char_indices().next_back()
        && word(ch)
    {
        previous
    } else {
        return offset..offset + text[offset..].chars().next().map_or(0, char::len_utf8);
    };
    let start = text[..offset]
        .char_indices()
        .rev()
        .find_map(|(index, ch)| (!word(ch)).then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    let end = text[offset..]
        .find(|ch| !word(ch))
        .map_or(text.len(), |end| offset + end);
    start..end
}

impl EntityInputHandler for LabelEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.byte_range(range);
        *actual_range = Some(self.to_utf16(range.clone()));
        Some(self.text[range].into())
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.text.encode_utf16().count())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(self.selection()),
            reversed: self.caret < self.anchor,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.byte_range(range);
        self.set_selection(range.start, range.end, window, cx);
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|range| self.to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.marked.take().is_some() {
            self.input_changed(window, cx);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active {
            return;
        }
        self.commit(
            self.replacement_range(range),
            &single_line(text),
            window,
            cx,
        );
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active {
            return;
        }
        let range = self.replacement_range(range);
        let text = single_line(text);
        self.replace(range.clone(), &text);
        self.marked = (!text.is_empty()).then_some(range.start..self.caret);
        if let Some(selection) = selection {
            self.anchor = range.start + utf16_to_byte(&text, selection.start);
            self.caret = (range.start + utf16_to_byte(&text, selection.end)).max(self.anchor);
        }
        self.input_changed(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let viewport = self.viewport?;
        let line = &self.cached.as_ref()?.line;
        Some(range_bounds(
            viewport,
            line,
            self.byte_range(range),
            self.scroll_x,
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.viewport?.localize(&position)?;
        self.cached.as_ref()?;
        Some(
            self.text[..self.index_at_x(position.x)]
                .encode_utf16()
                .count(),
        )
    }
}

impl Render for LabelEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(("label-editor", cx.entity_id()))
            .occlude()
            .role(Role::TextInput)
            .aria_label("Net label")
            .aria_value(self.text.clone())
            .font_family("Inter")
            .font_weight(FontWeight::MEDIUM)
            .text_size(px(NET_LABEL_FONT_PX))
            .px(px(PILL_PAD_X_PX))
            .py(px(PILL_PAD_Y_PX))
            .rounded(px(4.0))
            .border_1()
            .border_color(gpui_color(self.theme.focus_ring))
            .bg(gpui_color(self.theme.stage_paper))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(LabelTextElement(cx.entity()))
    }
}

struct CachedLine {
    font: Font,
    font_size: Pixels,
    color: Hsla,
    line: ShapedLine,
}

struct LabelTextElement(Entity<LabelEditor>);

struct LinePaint {
    line: ShapedLine,
    origin: Point<Pixels>,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
    marked: Option<PaintQuad>,
}

fn range_bounds(
    bounds: Bounds<Pixels>,
    line: &ShapedLine,
    range: Range<usize>,
    scroll: Pixels,
) -> Bounds<Pixels> {
    let left = line.x_for_index(range.start);
    let right = line.x_for_index(range.end);
    Bounds::from_corners(
        point(bounds.left() + left.min(right) - scroll, bounds.top()),
        point(bounds.left() + left.max(right) - scroll, bounds.bottom()),
    )
}

impl Element for LabelTextElement {
    type RequestLayoutState = ();
    type PrepaintState = LinePaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let (width, height) = self.0.update(cx, |editor, _| editor.layout_size(window));
        let mut style = Style::default();
        style.size.width = width.into();
        style.size.height = height.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> LinePaint {
        self.0
            .update(cx, |editor, _| editor.prepaint_line(bounds, window))
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        paint: &mut LinePaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, is_active, entity, weak) = {
            let editor = self.0.read(cx);
            (
                editor.focus.clone(),
                editor.active,
                self.0.clone(),
                self.0.downgrade(),
            )
        };
        if is_active {
            window.handle_input(&focus, ElementInputHandler::new(bounds, entity), cx);
        }
        let dragging_editor = weak.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture && focus.is_focused(window) {
                let _ = dragging_editor.update(cx, |editor, cx| {
                    if editor.active
                        && editor.dragging
                        && event.pressed_button == Some(MouseButton::Left)
                    {
                        let offset = editor.index_at_x(event.position.x);
                        editor.set_selection(editor.anchor, offset, window, cx);
                        cx.stop_propagation();
                    }
                });
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = weak.update(cx, |editor, _| editor.dragging = false);
            }
        });
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = paint.selection.take() {
                window.paint_quad(selection);
            }
            paint
                .line
                .paint(
                    paint.origin,
                    bounds.size.height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                )
                .expect("label editor line paints");
            if let Some(marked) = paint.marked.take() {
                window.paint_quad(marked);
            }
            if let Some(caret) = paint.caret.take() {
                window.paint_quad(caret);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolMachine, ToolPhase, ToolRequest, ToolState};
    use omnia_doc::{Orient, Part, PartId, Rect, SymbolId, Wire, WireId};
    use omnia_symbols::SymbolLibrary;

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    fn label_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let sym: SymbolId = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(sym);
        let pins: Vec<Pt> = def.pins.iter().map(|p| p.at).collect();
        Doc::from_items(
            [(
                Part {
                    symbol: sym,
                    at: pt(-6, 0),
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "1k".into(),
                },
                pins,
                def.bounds,
            )],
            [
                Wire {
                    a: pt(-4, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(0, -4),
                    b: pt(0, 4),
                },
                Wire {
                    a: pt(-4, 0),
                    b: pt(-4, 4),
                },
            ],
            [Label {
                at: pt(0, 0),
                text: "CLK".into(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn t7_2_find_label_target_covers_all_targets_rejects_grid_and_prioritizes_existing_label() {
        let doc = label_doc();
        let mut scratch = Vec::with_capacity(16);
        let radii = PointerRadii {
            hit: G / 4,
            snap: G / 2,
        };

        // Existing label on top of junction/wires takes priority inside hit tolerance.
        assert_eq!(
            find_label_target(&doc, pt(0, 0), radii, &mut scratch),
            Some(LabelTarget::ExistingLabel {
                id: LabelId(0),
                at: pt(0, 0),
            })
        );

        // Wire body snap within snap radius.
        assert_eq!(
            find_label_target(&doc, Pt { x: 2 * G, y: G / 5 }, radii, &mut scratch,),
            Some(LabelTarget::Attach(Snap::WireBody {
                wire: WireId(0),
                at: pt(2, 0),
            }))
        );

        // Pin snap at R1's left pin (-8, 0).
        assert_eq!(
            find_label_target(&doc, pt(-8, 0), radii, &mut scratch),
            Some(LabelTarget::Attach(Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: pt(-8, 0),
            }))
        );

        // Wire end snap at (4, 0).
        assert_eq!(
            find_label_target(&doc, pt(4, 0), radii, &mut scratch),
            Some(LabelTarget::Attach(Snap::WireEnd {
                wire: WireId(0),
                at: pt(4, 0),
            }))
        );

        // Empty grid space rejects placement (no Snap::Grid labels).
        assert_eq!(
            find_label_target(&doc, pt(10, 10), radii, &mut scratch),
            None
        );

        // Between hit radius (G/4) and snap radius (G/2) of an existing label sitting on a wire
        // end (`(4, 0)`), snapping onto that point still edits the existing label rather than
        // stacking a duplicate. Meanwhile an unlabeled pin 7 px away still accepts a new label.
        let lib = SymbolLibrary::builtin();
        let end_doc = Doc::from_items(
            doc.parts().map(|(_, p)| {
                let def = lib.get(p.symbol);
                (
                    p.clone(),
                    def.pins.iter().map(|pin| pin.at).collect(),
                    def.bounds,
                )
            }),
            [Wire {
                a: pt(-4, 0),
                b: pt(4, 0),
            }],
            [Label {
                at: pt(4, 0),
                text: "END".into(),
            }],
        )
        .unwrap();
        // Cursor at (4*G + 7*G/20, 0): outside hit (0.25*G = 5px at 100%) and inside snap (0.5*G = 10px).
        let near_end = Pt {
            x: 4 * G + G * 7 / 20,
            y: 0,
        };
        assert_eq!(
            find_label_target(&end_doc, near_end, radii, &mut scratch),
            Some(LabelTarget::ExistingLabel {
                id: LabelId(0),
                at: pt(4, 0),
            })
        );
        // Unlabeled pin at (-8, 0) with another label 7/20*G away from a different point won't steal (-8, 0).
        let near_pin = Pt {
            x: -8 * G + G * 7 / 20,
            y: 0,
        };
        assert_eq!(
            find_label_target(&end_doc, near_pin, radii, &mut scratch),
            Some(LabelTarget::Attach(Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: pt(-8, 0),
            }))
        );
    }

    #[test]
    fn t7_2_label_commit_command_enforces_inv5_noops() {
        let new_target = LabelEditTarget::New { at: pt(2, 0) };
        assert_eq!(label_commit_command(&new_target, ""), None);
        assert_eq!(label_commit_command(&new_target, "   \t "), None);
        assert_eq!(
            label_commit_command(&new_target, "  VCC "),
            Some(Command::AddLabel(Label {
                at: pt(2, 0),
                text: "VCC".into(),
            }))
        );

        let existing = LabelEditTarget::Existing {
            id: LabelId(0),
            at: pt(0, 0),
            original: "CLK".into(),
        };
        assert_eq!(label_commit_command(&existing, "CLK"), None);
        assert_eq!(label_commit_command(&existing, "  CLK  "), None);
        assert_eq!(label_commit_command(&existing, "   "), None);
        assert_eq!(
            label_commit_command(&existing, "MOSI"),
            Some(Command::SetText {
                item: ItemId::Label(LabelId(0)),
                field: TextField::Label,
                text: "MOSI".into(),
            })
        );

        let spaced_existing = LabelEditTarget::Existing {
            id: LabelId(1),
            at: pt(4, 0),
            original: " VCC ".into(),
        };
        assert_eq!(label_commit_command(&spaced_existing, " VCC "), None);
        assert_eq!(
            label_commit_command(&spaced_existing, "VCC"),
            Some(Command::SetText {
                item: ItemId::Label(LabelId(1)),
                field: TextField::Label,
                text: "VCC".into(),
            })
        );
    }

    #[test]
    fn t7_2_tool_machine_label_targeting_is_b10_zero_alloc_when_warm() {
        let doc = label_doc();
        let radii = PointerRadii {
            hit: G / 4,
            snap: G / 2,
        };
        let mut m = ToolMachine::new();
        m.set_tool(Tool::Label);
        assert_eq!(m.phase(), ToolPhase::Labeling);

        // Warm scratch capacity over dense query points.
        for x in -8..=8 {
            let _ = m.pointer_move_with_radii(&doc, pt(x, 0), radii, false);
        }

        let prev = crate::sheet::replace_alloc_counter(Some(0));
        for step in 0..32 {
            let p = Pt {
                x: ((step % 17) - 8) * G,
                y: ((step % 5) - 2) * (G / 4),
            };
            let _ = m.pointer_move_with_radii(&doc, p, radii, false);
        }
        let allocs = crate::sheet::replace_alloc_counter(prev).unwrap_or(0);
        assert_eq!(
            allocs, 0,
            "B10: warm Tool::Label pointer move must not allocate"
        );

        // Clicking empty grid in Tool::Label produces no edit request.
        let empty = m.pointer_down_with_radii(&doc, 1, pt(10, 10), radii, &[], false);
        assert!(empty.handled && empty.request.is_none());

        // Clicking wire body produces OpenLabelEditor request, and Esc twice leaves Tool::Label.
        let open = m.pointer_down_with_radii(&doc, 1, pt(2, 0), radii, &[], false);
        assert!(open.handled);
        assert!(matches!(
            open.request,
            Some(ToolRequest::OpenLabelEditor {
                target: LabelEditTarget::New { at },
                ..
            }) if at == pt(2, 0)
        ));
        assert!(matches!(
            m.state(),
            ToolState::Labeling(LabelGesture {
                editing: Some(_),
                ..
            })
        ));

        let esc1 = m.cancel();
        assert!(esc1.handled && esc1.preview_changed);
        assert_eq!(m.tool(), Tool::Label);
        let esc2 = m.cancel();
        assert!(esc2.handled && esc2.preview_changed);
        assert_eq!(m.tool(), Tool::Select);
    }

    #[test]
    fn t7_2_existing_label_tie_breaks_by_lowest_id() {
        let doc = Doc::from_items(
            [],
            [],
            [
                Label {
                    at: pt(0, 0),
                    text: "A".into(),
                },
                Label {
                    at: pt(0, 0),
                    text: "B".into(),
                },
            ],
        )
        .unwrap();
        let mut scratch = Vec::new();
        assert_eq!(
            find_existing_label(&doc, pt(0, 0), G / 4, &mut scratch),
            Some((LabelId(0), pt(0, 0)))
        );
    }

    #[test]
    fn t7_2_empty_sheet_has_zero_targets() {
        let doc = Doc::new();
        let mut scratch = Vec::new();
        let radii = PointerRadii {
            hit: G / 4,
            snap: G / 2,
        };
        assert_eq!(find_label_target(&doc, Pt::ZERO, radii, &mut scratch), None);
        assert!(Rect::from_points(Pt::ZERO, Pt::ZERO).is_ok());
    }

    #[gpui::test]
    fn t7_2_label_editor_entity_input_handler_ime_and_pointer_selection(
        cx: &mut gpui::TestAppContext,
    ) {
        use std::{cell::RefCell, rc::Rc};

        struct EditorHost {
            focus: FocusHandle,
            editor: Entity<LabelEditor>,
            events: Rc<RefCell<Vec<LabelEditorEvent>>>,
        }

        impl Render for EditorHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .key_context(TEXT_FIELD_CONTEXT)
                    .track_focus(&self.focus)
                    .p(px(20.0))
                    .child(self.editor.clone())
            }
        }

        let events: Rc<RefCell<Vec<LabelEditorEvent>>> = Rc::new(RefCell::new(Vec::new()));
        let events_init = events.clone();
        let (host, cx) = cx.add_window_view(move |window, cx| {
            let focus = cx.focus_handle();
            let f_clone = focus.clone();
            let editor =
                cx.new(|cx| LabelEditor::new(f_clone, &omnia_theme::GRAPHITE, "a😀b", false, cx));
            let events_sub = events_init.clone();
            cx.subscribe(&editor, move |_, _, ev: &LabelEditorEvent, _| {
                events_sub.borrow_mut().push(ev.clone());
            })
            .detach();
            window.activate_window();
            window.focus(&focus, cx);
            EditorHost {
                focus,
                editor,
                events: events_init,
            }
        });
        cx.run_until_parked();

        // Exercise IME marked text, UTF-16 surrogate ranges, bounds queries, and unmark_text.
        cx.update(|window, cx| {
            let editor = host.read(cx).editor.clone();
            editor.update(cx, |ed, cx| {
                assert_eq!(ed.text_length_utf16(window, cx), Some(4));
                let mut actual = None;
                assert_eq!(
                    ed.text_for_range(1..3, &mut actual, window, cx).as_deref(),
                    Some("😀")
                );
                assert_eq!(actual, Some(1..3));

                ed.replace_and_mark_text_in_range(Some(1..3), "🐱\n猫", Some(2..3), window, cx);
                assert_eq!(ed.shared_text(), "a🐱 猫b");
                assert_eq!(ed.marked_text_range(window, cx), Some(1..5));
                let sel = ed.selected_text_range(false, window, cx).unwrap();
                assert_eq!(sel.range, 3..4);
            });
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            assert!(host.read(cx).events.borrow().is_empty());
            let editor = host.read(cx).editor.clone();
            editor.update(cx, |ed, cx| {
                let bounds = ed.viewport.expect("drawn editor has viewport");
                let r_bounds = ed.bounds_for_range(0..2, bounds, window, cx);
                assert!(r_bounds.is_some());
                assert_eq!(
                    ed.selection_bounds_for_range(0..2, bounds, window, cx)
                        .len(),
                    1
                );

                ed.unmark_text(window, cx);
                assert_eq!(ed.marked_text_range(window, cx), None);
                ed.set_selected_text_range(0..6, window, cx);
                ed.replace_text_in_range(None, "CLK_BUS", window, cx);
                assert_eq!(ed.shared_text(), "CLK_BUS");
            });
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            let editor = host.read(cx).editor.clone();
            editor.update(cx, |ed, cx| {
                let bounds = ed.viewport.expect("drawn editor has viewport");
                let mid = ed
                    .bounds_for_range(0..3, bounds, window, cx)
                    .unwrap()
                    .center();
                assert!(ed.character_index_for_point(mid, window, cx).is_some());
            });
        });

        // Double-click word selection and drag past right edge + Enter submit.
        let pos = cx.update(|_, cx| {
            let ed = host.read(cx).editor.read(cx);
            let vp = ed.viewport.unwrap();
            let line = &ed.cached.as_ref().unwrap().line;
            point(vp.left() + line.x_for_index(1), vp.center().y)
        });
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: pos,
            modifiers: gpui::Modifiers::none(),
            click_count: 2,
            first_mouse: false,
        });
        cx.update(|window, cx| {
            let ed_entity = host.read(cx).editor.clone();
            let sel = ed_entity
                .update(cx, |ed, cx| ed.selected_text_range(false, window, cx))
                .unwrap();
            assert_eq!(sel.range, 0..7);
        });
        cx.simulate_mouse_up(pos, MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            events.borrow().as_slice(),
            &[LabelEditorEvent::Submit("CLK_BUS".into())]
        );
    }

    fn assert_inv10(doc: &Doc) {
        let oracle = omnia_doc::Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &w)| (id, w)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        assert_eq!(doc.net_count(), oracle.net_count());
        let mut pts: Vec<Pt> = doc
            .wires()
            .flat_map(|(_, w)| [w.a, w.b])
            .chain(
                doc.parts()
                    .flat_map(|(id, _)| doc.pins(id).unwrap().iter().copied()),
            )
            .chain(doc.labels().map(|(_, l)| l.at))
            .collect();
        pts.sort_unstable_by_key(|p| (p.x, p.y));
        pts.dedup();
        for &a in &pts {
            assert_eq!(doc.net_at(a).is_some(), oracle.net_at(a).is_some());
            for &b in &pts {
                assert_eq!(
                    doc.net_at(a) == doc.net_at(b),
                    oracle.net_at(a) == oracle.net_at(b)
                );
            }
        }
        doc.assert_normal_form();
    }

    #[test]
    fn w3_6_wd9_wire_body_split_on_lattice_and_half_grid_and_tc_label_04() {
        // Lattice wire body + half-grid wire body (from pin offset by half grid).
        let half = G / 2;
        let mut doc = Doc::from_items(
            [],
            [
                Wire {
                    a: pt(0, 0),
                    b: pt(6, 0),
                },
                Wire {
                    a: Pt { x: half, y: 2 * G },
                    b: Pt { x: half, y: 6 * G },
                },
            ],
            [],
        )
        .unwrap();
        let radii = PointerRadii {
            hit: G / 4,
            snap: G / 2,
        };
        let mut scratch = Vec::with_capacity(16);
        let mut gesture = LabelGesture::default();

        // 1. Click lattice wire body near (3G, 0): snaps to (3G, 0) and splits the wire into 2 segments.
        let (edit1, _) = gesture
            .click(
                &doc,
                1,
                Pt {
                    x: 3 * G + G / 6,
                    y: G / 8,
                },
                radii,
                &mut scratch,
            )
            .0
            .unwrap();
        assert_eq!(edit1, LabelEditTarget::New { at: pt(3, 0) });
        let cmd1 = label_commit_command(&edit1, " NET_L ").unwrap();
        let pre1 = doc.fork();
        doc.apply(cmd1).unwrap();
        assert_inv10(&doc);
        let segs_y0: Vec<Wire> = doc
            .wires()
            .filter(|(_, w)| w.a.y == 0)
            .map(|(_, &w)| w)
            .collect();
        assert_eq!(segs_y0.len(), 2);
        assert!(segs_y0.contains(&Wire {
            a: pt(0, 0),
            b: pt(3, 0)
        }));
        assert!(segs_y0.contains(&Wire {
            a: pt(3, 0),
            b: pt(6, 0)
        }));

        // 2. Click half-grid wire body near (half, 4G): WD5 keeps half-grid X and snaps Y to grid 4G,
        //    then Command::AddLabel splits the half-grid wire cleanly.
        let (edit2, _) = gesture
            .click(
                &doc,
                1,
                Pt {
                    x: half + G / 8,
                    y: 4 * G + G / 6,
                },
                radii,
                &mut scratch,
            )
            .0
            .unwrap();
        assert_eq!(
            edit2,
            LabelEditTarget::New {
                at: Pt { x: half, y: 4 * G },
            }
        );
        doc.apply(label_commit_command(&edit2, "HALF").unwrap())
            .unwrap();
        assert_inv10(&doc);
        assert_eq!(doc.wires().count(), 4);

        // Undo back to pre1 restores exact unsplit wire (INV4).
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert!(doc.content_eq(&pre1));
        assert_inv10(&doc);
        doc.redo().unwrap();
        assert_inv10(&doc);

        // 3. TC-label-04: deleting the right half (3, 0)->(6, 0) keeps NET_L on the left half
        //    and removes its name/net from (6, 0).
        let right_wire = doc
            .wires()
            .find(|(_, w)| {
                **w == Wire {
                    a: pt(3, 0),
                    b: pt(6, 0),
                }
            })
            .map(|(id, _)| ItemId::Wire(id))
            .unwrap();
        let left_net_before = doc.net_at(pt(0, 0));
        assert_eq!(left_net_before, doc.net_at(pt(6, 0)));
        doc.apply(Command::Delete {
            items: vec![right_wire],
            with_wires: false,
        })
        .unwrap();
        assert_inv10(&doc);
        assert_eq!(doc.net_at(pt(0, 0)), doc.net_at(pt(3, 0)));
        assert_ne!(doc.net_at(pt(0, 0)), doc.net_at(pt(6, 0)));
    }
}
