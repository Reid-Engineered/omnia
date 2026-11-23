//! Single-line input for Home search, part filters, and Preferences (UD16).
//! The GPUI input-handler and element wiring is adapted from its input example.

use crate::{actions::KeyContext, theme, widgets::button::FOCUS_RING_WIDTH};
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, DispatchPhase, Element,
    ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, Font, GlobalElementId, Hsla, InputPreference, InspectorElementId, Keystroke,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    Role, ShapedLine, SharedString, Style, Subscription, TextAlign, TextRun, UTF16Selection,
    Window, div, fill, point, prelude::*, px, relative, size,
};
use std::{borrow::Cow, ops::Range};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// A user edit was committed. Read the current value with TextField::text.
    Changed,
    Submit,
    Cancel,
}

/// A tab stop with its own editing keys and no document-facing actions.
pub struct TextField {
    focus: FocusHandle,
    _keystrokes: Subscription,
    text: SharedString,
    placeholder: SharedString,
    label: SharedString,
    anchor: usize,
    caret: usize,
    marked: Option<Range<usize>>,
    dragging: bool,
    cached: Option<CachedLine>,
    viewport: Option<Bounds<Pixels>>,
    scroll_x: Pixels,
}

impl TextField {
    pub fn new(
        label: impl Into<SharedString>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle().tab_stop(true);
        let field = cx.weak_entity();
        let keystrokes = cx.intercept_keystrokes({
            let focus = focus.downgrade();
            move |event, window, cx| {
                // AltGr and other character input explicitly preferred by the platform
                // must continue to the native text handler.
                if event.input_preference == InputPreference::CharacterInput
                    || !focus
                        .upgrade()
                        .is_some_and(|focus| focus.is_focused(window))
                {
                    return;
                }
                let _ = field.update(cx, |field, cx| {
                    if field.edit_keystroke(&event.keystroke, window, cx) {
                        cx.stop_propagation();
                    }
                });
            }
        });
        let placeholder = placeholder.into();
        let placeholder = match single_line(&placeholder) {
            Cow::Borrowed(_) => placeholder,
            Cow::Owned(clean) => clean.into(),
        };
        Self {
            focus,
            _keystrokes: keystrokes,
            text: "".into(),
            placeholder,
            label: label.into(),
            anchor: 0,
            caret: 0,
            marked: None,
            dragging: false,
            cached: None,
            viewport: None,
            scroll_x: px(0.),
        }
    }

    /// The current field contents, including any in-progress IME composition.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Load a value and place the caret at the end without emitting Changed.
    pub fn set_text(&mut self, text: &str, window: &Window, cx: &mut Context<Self>) {
        let text = single_line(text);
        self.replace(0..self.text.len(), &text);
        self.marked = None;
        self.input_changed(window, cx);
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
            (true, _, "z" | "y") => {} // Keep document undo/redo out of the field.
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
            (false, false, "backspace" | "delete") => {
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
                cx.emit(TextFieldEvent::Submit);
            }
            (false, false, "escape") => cx.emit(TextFieldEvent::Cancel),
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
            cx.emit(TextFieldEvent::Changed);
        } else if previous_selection != (self.anchor, self.caret) {
            window.notify_text_selection_changed(&self.focus);
            cx.notify();
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
    // Each substitute has the same UTF-16 length as the original separator.
    // Native selections inside marked text therefore keep their offsets.
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

impl EventEmitter<TextFieldEvent> for TextField {}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TextField {
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
            cx.emit(TextFieldEvent::Changed);
        }
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = theme::theme(cx);
        let (label, value, child) = text_field_paint(|| {
            (
                self.label.clone(),
                self.text.clone(),
                TextElement(cx.entity()),
            )
        });
        div()
            .id(("text-field", cx.entity_id()))
            .key_context(KeyContext::TextField.as_str())
            .track_focus(&self.focus)
            .role(Role::TextInput)
            .aria_label(label)
            .aria_value(value)
            .w_full()
            .min_w_0()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(6.))
            .border(px(FOCUS_RING_WIDTH))
            .border_color(theme::color(active.rule))
            .focus(|style| style.border_color(theme::color(active.focus_ring)))
            .bg(theme::color(active.bg_inset))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(child)
    }
}

struct CachedLine {
    font: Font,
    font_size: Pixels,
    color: Hsla,
    line: ShapedLine,
}

struct TextElement(Entity<TextField>);

struct LinePaint {
    line: ShapedLine,
    origin: Point<Pixels>,
    selection: Option<PaintQuad>,
    caret: Option<PaintQuad>,
    marked: Option<PaintQuad>,
}

impl TextField {
    fn prepaint_line(&mut self, bounds: Bounds<Pixels>, window: &Window, cx: &App) -> LinePaint {
        let active = theme::theme(cx);
        let style = window.text_style();
        let (font, font_size, color, reuse) = text_field_paint(|| {
            let font = style.font();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let color = theme::color(if self.text.is_empty() {
                active.text_muted
            } else {
                active.text_primary
            });
            let reuse = self.cached.as_ref().is_some_and(|cached| {
                cached.font == font && cached.font_size == font_size && cached.color == color
            });
            (font, font_size, color, reuse)
        });
        if !reuse {
            let (display, run) = text_field_paint(|| {
                let display = if self.text.is_empty() {
                    &self.placeholder
                } else {
                    &self.text
                };
                let run = TextRun {
                    len: display.len(),
                    font: font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                (display.clone(), run)
            });
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
        text_field_paint(|| {
            let line = &self.cached.as_ref().expect("line cache was populated").line;
            let focused = self.focus.is_focused(window);
            let caret_x = line.x_for_index(self.caret);
            let extent = if self.text.is_empty() {
                px(0.)
            } else {
                line.width() + px(1.)
            };
            let left = (caret_x + px(1.) - bounds.size.width).max(px(0.));
            self.scroll_x = self
                .scroll_x
                .max(left)
                .min(caret_x)
                .min((extent - bounds.size.width).max(px(0.)));
            self.viewport = Some(bounds);
            let selection = self.selection();
            let rect = |range| range_bounds(bounds, line, range, self.scroll_x);
            let caret_x = bounds.left() + caret_x - self.scroll_x;
            LinePaint {
                line: line.clone(),
                origin: point(bounds.left() - self.scroll_x, bounds.top()),
                selection: (focused && !selection.is_empty()).then(|| {
                    fill(
                        rect(selection),
                        theme::color(active.stage_select_halo).opacity(0.25),
                    )
                }),
                caret: (focused && self.anchor == self.caret).then(|| {
                    fill(
                        Bounds::new(
                            point(caret_x, bounds.top()),
                            size(px(1.), bounds.size.height),
                        ),
                        theme::color(active.text_primary),
                    )
                }),
                marked: self.marked.clone().filter(|_| focused).map(|range| {
                    let area = rect(range);
                    fill(
                        Bounds::new(
                            point(area.left(), area.bottom() - px(1.)),
                            size(area.size.width, px(1.)),
                        ),
                        theme::color(active.text_primary),
                    )
                }),
            }
        })
    }
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

impl Element for TextElement {
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
        let style = text_field_paint(|| {
            let mut style = Style::default();
            style.size.width = relative(1.).into();
            style.size.height = window.line_height().into();
            style
        });
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
            .update(cx, |field, cx| field.prepaint_line(bounds, window, cx))
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
        let (focus, input, field, mask) = text_field_paint(|| {
            (
                self.0.read(cx).focus.clone(),
                ElementInputHandler::new(bounds, self.0.clone()),
                self.0.downgrade(),
                ContentMask { bounds },
            )
        });
        window.handle_input(&focus, input, cx);
        // Continue selection outside the field, and release even over a different view.
        let dragging_field = field.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Capture && focus.is_focused(window) {
                let _ = dragging_field.update(cx, |field, cx| {
                    if field.dragging && event.pressed_button == Some(MouseButton::Left) {
                        let offset = field.index_at_x(event.position.x);
                        field.set_selection(field.anchor, offset, window, cx);
                        cx.stop_propagation();
                    }
                });
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = field.update(cx, |field, _| field.dragging = false);
            }
        });
        window.with_content_mask(Some(mask), |window| {
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
                .expect("text field line paints");
            if let Some(marked) = paint.marked.take() {
                window.paint_quad(marked);
            }
            if let Some(caret) = paint.caret.take() {
                window.paint_quad(caret);
            }
        });
    }
}

// Like the sheet's paint scope, the test measures our preparation and data
// handling separately from GPUI's layout, scene, shaping, and input registration.
#[inline]
fn text_field_paint<T>(paint: impl FnOnce() -> T) -> T {
    #[cfg(test)]
    let _scope = tests::PaintScope::enter();
    paint()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        actions::{Copy, Cut, Delete, Paste, Redo, Rotate, SelectAll, Undo},
        keymap,
    };
    use gpui::{AppContext, Modifiers, TestApp, TestAppContext};
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
    };

    struct Host {
        field: Entity<TextField>,
        sheet: FocusHandle,
        rotate: usize,
        document_edits: usize,
        events: Vec<TextFieldEvent>,
    }

    impl Host {
        fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
            let field = cx.new(|cx| TextField::new("Threshold", "Enter a value", cx));
            cx.subscribe(&field, |host, _, event, _| host.events.push(*event))
                .detach();
            Self {
                field,
                sheet: cx.focus_handle(),
                rotate: 0,
                document_edits: 0,
                events: Vec::new(),
            }
        }
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().key_context("Shell").child(
                div()
                    .key_context("Workspace")
                    .on_action(cx.listener(|host, _: &Undo, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &Redo, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &Copy, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &Cut, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &Paste, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &Delete, _, _| host.document_edits += 1))
                    .on_action(cx.listener(|host, _: &SelectAll, _, _| host.document_edits += 1))
                    .child(
                        div()
                            .key_context("Sheet")
                            .track_focus(&self.sheet)
                            .on_action(cx.listener(|host, _: &Rotate, _, _| host.rotate += 1)),
                    )
                    .child(div().key_context("Dialog").child(self.field.clone())),
            )
        }
    }

    #[gpui::test]
    fn utc_type_r_threshold_field_leaves_document_shortcuts_alone(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let bindings = keymap::default_bindings();
            assert!(keymap::conflicts(&bindings).is_empty()); // UINV4
            cx.bind_keys(bindings);
        });
        let (host, cx) = cx.add_window_view(Host::new);
        cx.update(|window, cx| window.focus(&host.read(cx).sheet.clone(), cx));
        cx.simulate_keystrokes("r ctrl-z");
        cx.update(|window, cx| {
            let host = host.read(cx);
            assert_eq!((host.rotate, host.document_edits), (1, 1));
            window.focus(&host.field.read(cx).focus_handle(cx), cx);
        });

        cx.simulate_keystrokes("r shift-r ctrl-z ctrl-shift-z ctrl-y");
        cx.update(|_, cx| {
            let host = host.read(cx);
            assert_eq!(host.field.read(cx).text(), "rR");
            assert_eq!((host.rotate, host.document_edits), (1, 1));
            assert_eq!(
                host.events,
                [TextFieldEvent::Changed, TextFieldEvent::Changed]
            );
        });
    }

    #[gpui::test]
    fn navigation_selection_clipboard_and_deletion(cx: &mut TestAppContext) {
        cx.update(|cx| cx.bind_keys(keymap::default_bindings()));
        let (host, cx) = cx.add_window_view(Host::new);
        cx.update(|window, cx| window.focus(&host.read(cx).field.read(cx).focus_handle(cx), cx));
        cx.simulate_keystrokes("a b c d home right shift-right shift-right ctrl-c");
        cx.update(|_, cx| {
            let field = host.read(cx).field.read(cx);
            assert_eq!(field.text(), "abcd");
            assert_eq!(field.selection(), 1..3);
            assert_eq!(
                cx.read_from_clipboard().unwrap().text().as_deref(),
                Some("bc")
            );
        });

        cx.simulate_keystrokes("ctrl-x end ctrl-v backspace home delete");
        cx.update(|_, cx| {
            assert_eq!(host.read(cx).field.read(cx).text(), "db");
            cx.write_to_clipboard(ClipboardItem::new_string("e\r\nf\u{2028}g".into()));
        });
        cx.simulate_keystrokes("ctrl-a ctrl-v end shift-left shift-left left delete enter escape");
        cx.update(|_, cx| {
            let host = host.read(cx);
            assert_eq!(host.field.read(cx).text(), "e  fg");
            assert_eq!(host.document_edits, 0);
            assert!(
                host.events
                    .ends_with(&[TextFieldEvent::Submit, TextFieldEvent::Cancel])
            );
        });
    }

    #[gpui::test]
    fn click_double_click_and_drag_past_the_field(cx: &mut TestAppContext) {
        let (field, cx) = cx.add_window_view(|window, cx| {
            let mut field = TextField::new("Threshold", "", cx);
            field.set_text("alpha beta", window, cx);
            field
        });
        let (alpha, past_end) = cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            let field = field.read(cx);
            let area = field.viewport.unwrap();
            let line = &field.cached.as_ref().unwrap().line;
            (
                point(area.left() + line.x_for_index(2), area.center().y),
                point(area.right() + px(20.), area.center().y),
            )
        });
        cx.simulate_click(alpha, Modifiers::default());
        cx.update(|window, cx| {
            let field = field.read(cx);
            assert!(field.focus.is_focused(window));
            assert_eq!(field.selection(), 2..2);
        });
        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: alpha,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.update(|_, cx| assert_eq!(field.read(cx).selection(), 0..5));
        cx.simulate_mouse_move(past_end, MouseButton::Left, Modifiers::default());
        cx.update(|_, cx| assert_eq!(field.read(cx).selection(), 0..10));
        cx.simulate_mouse_up(past_end, MouseButton::Left, Modifiers::default());
        cx.update(|_, cx| assert!(!field.read(cx).dragging));
    }

    #[gpui::test]
    fn ime_replaces_the_marked_range_and_reports_utf16_offsets(cx: &mut TestAppContext) {
        let (host, cx) = cx.add_window_view(Host::new);
        cx.update(|window, cx| {
            let field = host.read(cx).field.clone();
            field.update(cx, |field, cx| {
                field.set_text("a😀b", window, cx);
                field.replace_and_mark_text_in_range(Some(1..3), "🐱\n猫", Some(2..3), window, cx);
                assert_eq!(field.text(), "a🐱 猫b");
                assert_eq!(field.marked_text_range(window, cx), Some(1..5));
                let selection = field.selected_text_range(false, window, cx).unwrap();
                assert_eq!(selection.range, 3..4);
                assert!(!selection.reversed);
                field.replace_and_mark_text_in_range(None, "猫", None, window, cx);
                assert_eq!(field.marked_text_range(window, cx), Some(1..2));
                assert_eq!(field.text(), "a猫b");
            });
        });
        cx.update(|window, cx| {
            let host = host.read(cx);
            assert!(host.events.is_empty());
            host.field.clone().update(cx, |field, cx| {
                field.replace_text_in_range(None, "猫", window, cx);
                assert_eq!(field.marked_text_range(window, cx), None);
                assert_eq!(field.text(), "a猫b");
                assert_eq!(field.text_length_utf16(window, cx), Some(3));
            });
        });
        cx.update(|_, cx| assert_eq!(host.read(cx).events, [TextFieldEvent::Changed]));
    }

    // The allocator is local to the app's unit-test executable. Counters apply
    // only to the current test thread and only inside the field's paint scope.
    struct Counting;

    thread_local! {
        static IN_PAINT: Cell<bool> = const { Cell::new(false) };
        static VISITS: Cell<usize> = const { Cell::new(0) };
        static ALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) struct PaintScope(bool);

    impl PaintScope {
        pub(super) fn enter() -> Self {
            VISITS.with(|n| n.set(n.get() + 1));
            Self(IN_PAINT.with(|active| active.replace(true)))
        }
    }

    impl Drop for PaintScope {
        fn drop(&mut self) {
            IN_PAINT.with(|active| active.set(self.0));
        }
    }

    fn record() {
        if IN_PAINT.try_with(Cell::get).unwrap_or(false) {
            let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        }
    }

    // SAFETY: every operation forwards its arguments and contract to System.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record();
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            record();
            unsafe { System.realloc(ptr, layout, size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    #[test]
    fn u3_1_warm_paint_reuses_shaping_without_omnia_allocations() {
        let mut app = TestApp::new();
        let mut window = app.open_window(|_, cx| TextField::new("Threshold", "Enter a value", cx));
        for text in ["", "A long value 🐱 with several words"] {
            window.update(|field, window, cx| {
                field.set_text(text, window, cx);
                field.focus.focus(window, cx);
            });
            window.draw();
            // Exercise the allocator itself so zero is meaningful.
            ALLOCS.set(0);
            text_field_paint(|| std::hint::black_box(Box::new(1)));
            assert_eq!(ALLOCS.get(), 1);

            for anchor in [text.len(), 0] {
                window.update(|field, window, cx| {
                    field.set_selection(anchor, text.len(), window, cx);
                    cx.notify();
                });
                ALLOCS.set(0);
                VISITS.set(0);
                window.draw();
                assert_eq!(
                    VISITS.get(),
                    5,
                    "warm render, layout, prepaint and paint executed"
                );
                assert_eq!(ALLOCS.get(), 0);
            }
        }
    }
}
