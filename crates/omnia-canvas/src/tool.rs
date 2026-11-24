//! T5.2: the tool state machine and the canvas actions' handlers (canvas-plan section 5, INV5).
//!
//! Input goes to the active tool. During a gesture the tool holds preview state and the
//! document does not change; on commit the tool hands its host exactly one [`Command`],
//! which the host applies with `Doc::apply`, so a committed gesture is exactly one undo
//! step. Cancelling (Esc) drops the preview and leaves the revision unchanged (INV5).
//!
//! [`ToolMachine`] is pure: it reads the document only to hit-test a press and never edits
//! it, so every transition is tested without a window. Its states:
//!
//! | State | Entered by | Left by |
//! | --- | --- | --- |
//! | [`ToolState::Idle`] | `Tool::Select` or `Tool::Label` with nothing under the pointer | hover, press on an item, tool change |
//! | [`ToolState::Hovering`] | the pointer over an item in `Tool::Select` | pointer off the item, press |
//! | [`ToolState::Dragging`] | a left press on an item in `Tool::Select` without Shift | release (commit), Esc, a lost release, tool change |
//! | [`ToolState::Placing`] | `Tool::Place(symbol)` | Esc or a tool change; a click requests a placement and stays |
//! | [`ToolState::Wiring`] | `Tool::Wire` (W) | Esc (first drops the start, then leaves the tool) or a tool change |
//!
//! Seams for the tasks that build on this one: a placement click yields
//! [`ToolRequest::Place`] for T5.3's placement provider (`tool/place.rs`); the drag commit is
//! [`DragGesture::command`], which T5.3 (`tool/move_.rs`) and W2.3 extend with ghosts,
//! anchor-pin snapping and follow previews; R, X and Y outside a gesture yield
//! [`ToolRequest::ReorientSelection`] and Del yields [`ToolRequest::DeleteSelection`] for
//! T5.3; [`WireGesture`] holds the wire tool's cursor and start for T6.2 (`tool/wire.rs`),
//! which adds the route preview, snapping and commits.
//!
//! The GPUI side: [`sheet_key_context`] puts a host's focus element in the `Sheet` key
//! context and routes the [`crate::keymap`] actions to the machine; [`ToolHost`] is the
//! small surface a canvas view exposes for that (`OverlayView`, `SheetCanvas`).

use crate::input::SheetCanvas;
use crate::keymap::{
    Cancel, Delete, MirrorHorizontal, MirrorVertical, PlaceWire, Redo, Rotate, SHEET_CONTEXT, Undo,
};
use crate::overlay::OverlayView;
use gpui::{Context, Div, Entity, InteractiveElement, Window};
use omnia_doc::{
    Change, Command, Doc, GRID_NM, ItemId, Orient, Pt, SymbolId, hit_test,
    prefs::{FollowPolicy, WiringPrefs},
    snap_to,
};

/// The active tool (canvas-plan section 9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// Hover, click and band selection, and dragging the selection.
    #[default]
    Select,
    /// Drawing wires (W). T6.2 adds the route preview and commits.
    Wire,
    /// Placing a part of this symbol. T5.3 commits through the placement provider.
    Place(SymbolId),
    /// Placing net labels (W3.6). No gesture yet: the pointer falls through to selection.
    Label,
}

/// The state's kind, without its data (tests and status).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolPhase {
    Idle,
    Hovering,
    Dragging,
    Placing,
    Wiring,
}

/// A drag of the selection, held as preview state until release (INV5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragGesture {
    /// The dragged items in canonical selection order.
    pub items: Vec<ItemId>,
    /// The item under the press.
    pub pressed: ItemId,
    /// World point of the press.
    pub anchor: Pt,
    /// World point of the pointer now.
    pub current: Pt,
    /// R, X and Y pressed during the drag, composed in order.
    pub orient: Orient,
}

impl DragGesture {
    /// The pointer movement, snapped to the grid so items stay on it (K2).
    pub fn delta(&self) -> Pt {
        let d = |a: i32, b: i32| {
            // Both ends are inside K1, so the difference fits i32 (|d| <= 2e9).
            i32::try_from(i64::from(b) - i64::from(a)).expect("K1 keeps a drag inside i32")
        };
        snap_to(
            Pt {
                x: d(self.anchor.x, self.current.x),
                y: d(self.anchor.y, self.current.y),
            },
            GRID_NM,
        )
    }

    /// Whether the release would change the document.
    pub fn is_effective(&self) -> bool {
        self.delta() != Pt::ZERO || self.orient != Orient::IDENTITY
    }

    /// The one command this drag commits, or `None` for a drag that moved nothing: a `Move`
    /// by [`Self::delta`], then, if R, X or Y was pressed, a `Reorient` about the grid point
    /// under the dropped grab point, both in one `Batch` so the drag is one undo step (INV5).
    pub fn command(&self, follow: FollowPolicy) -> Option<Command> {
        let delta = self.delta();
        let mut steps = Vec::with_capacity(2);
        if delta != Pt::ZERO {
            steps.push(Command::Move {
                items: self.items.clone(),
                delta,
                follow,
            });
        }
        if self.orient != Orient::IDENTITY {
            let pivot = Pt {
                x: self.anchor.x.saturating_add(delta.x),
                y: self.anchor.y.saturating_add(delta.y),
            };
            steps.push(Command::Reorient {
                items: self.items.clone(),
                pivot: snap_to(pivot, GRID_NM),
                by: self.orient,
                follow,
            });
        }
        match steps.len() {
            0 => None,
            1 => steps.pop(),
            _ => Some(Command::Batch(steps)),
        }
    }
}

/// `Tool::Place` state: where the part would go and how it is turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceGesture {
    pub symbol: SymbolId,
    /// The grid point under the pointer, once the pointer has been over the sheet.
    pub at: Option<Pt>,
    /// R, X and Y pressed while placing, composed in order.
    pub orient: Orient,
}

/// `Tool::Wire` state. T6.2 adds the route preview, WD5 snapping and the commits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireGesture {
    /// The first click, once made.
    pub start: Option<Pt>,
    /// The world point under the pointer.
    pub cursor: Option<Pt>,
}

/// The tool's state (canvas-plan section 5): idle, hovering, dragging, placing, wiring.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ToolState {
    #[default]
    Idle,
    Hovering(ItemId),
    Dragging(DragGesture),
    Placing(PlaceGesture),
    Wiring(WireGesture),
}

impl ToolState {
    pub fn phase(&self) -> ToolPhase {
        match self {
            Self::Idle => ToolPhase::Idle,
            Self::Hovering(_) => ToolPhase::Hovering,
            Self::Dragging(_) => ToolPhase::Dragging,
            Self::Placing(_) => ToolPhase::Placing,
            Self::Wiring(_) => ToolPhase::Wiring,
        }
    }
}

/// Work the machine cannot finish alone; the host carries it out (seams for T5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolRequest {
    /// A press on an item that did not become a drag: make it the only selection.
    Click(ItemId),
    /// A placement click: T5.3 asks the placement provider for the command.
    Place {
        symbol: SymbolId,
        at: Pt,
        orient: Orient,
    },
    /// R, X or Y outside a gesture: T5.3 turns the selection with one command.
    ReorientSelection(Orient),
    /// Del outside a gesture: T5.3's `delete_selection`.
    DeleteSelection,
}

/// What one input step did. A host applies [`Self::commit`] with `Doc::apply`, repaints
/// the overlay on [`Self::preview_changed`], and holds or releases the pointer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolOutcome {
    /// The tool consumed the input; the host does not pass it on to selection.
    pub handled: bool,
    /// Preview state changed: repaint the overlay (never the sheet, INV11).
    pub preview_changed: bool,
    /// A gesture started that holds the pointer.
    pub capture: bool,
    /// A gesture that held the pointer ended or was cancelled.
    pub release: bool,
    /// The one command a gesture commits (INV5).
    pub commit: Option<Command>,
    pub request: Option<ToolRequest>,
}

impl ToolOutcome {
    fn handled() -> Self {
        Self {
            handled: true,
            ..Self::default()
        }
    }

    fn preview() -> Self {
        Self {
            handled: true,
            preview_changed: true,
            ..Self::default()
        }
    }
}

/// The tool state machine. Pure: no GPUI, and it never edits the document.
#[derive(Clone, Debug, Default)]
pub struct ToolMachine {
    tool: Tool,
    state: ToolState,
    prefs: WiringPrefs,
    scratch: Vec<ItemId>,
}

impl ToolMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_prefs(prefs: WiringPrefs) -> Self {
        Self {
            prefs,
            ..Self::default()
        }
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn state(&self) -> &ToolState {
        &self.state
    }

    pub fn phase(&self) -> ToolPhase {
        self.state.phase()
    }

    pub fn prefs(&self) -> &WiringPrefs {
        &self.prefs
    }

    /// The follow policy is read from these at commit (WD12, WK3).
    pub fn set_prefs(&mut self, prefs: WiringPrefs) {
        self.prefs = prefs;
    }

    /// A drag holds the pointer until release.
    pub fn is_dragging(&self) -> bool {
        matches!(self.state, ToolState::Dragging(_))
    }

    /// Whether pointer moves and releases go to the tool instead of hover and selection.
    pub fn routes_pointer(&self) -> bool {
        matches!(
            self.state,
            ToolState::Dragging(_) | ToolState::Placing(_) | ToolState::Wiring(_)
        )
    }

    fn rest_state(tool: Tool) -> ToolState {
        match tool {
            Tool::Select | Tool::Label => ToolState::Idle,
            Tool::Wire => ToolState::Wiring(WireGesture::default()),
            Tool::Place(symbol) => ToolState::Placing(PlaceGesture {
                symbol,
                at: None,
                orient: Orient::IDENTITY,
            }),
        }
    }

    /// Switches tools. Any gesture under way is cancelled first (revision unchanged, INV5).
    pub fn set_tool(&mut self, tool: Tool) -> ToolOutcome {
        let release = self.is_dragging();
        let next = Self::rest_state(tool);
        let changed = self.tool != tool || self.state != next;
        self.tool = tool;
        self.state = next;
        ToolOutcome {
            handled: true,
            preview_changed: changed,
            release,
            ..ToolOutcome::default()
        }
    }

    /// The item under the pointer, as hover tracking resolved it. Only `Tool::Select` hovers;
    /// a gesture ignores it.
    pub fn hover(&mut self, item: Option<ItemId>) -> ToolOutcome {
        if self.tool != Tool::Select
            || !matches!(self.state, ToolState::Idle | ToolState::Hovering(_))
        {
            return ToolOutcome::default();
        }
        let next = item.map_or(ToolState::Idle, ToolState::Hovering);
        let changed = self.state != next;
        self.state = next;
        ToolOutcome {
            preview_changed: changed,
            ..ToolOutcome::default()
        }
    }

    /// A left press at world point `at`. In `Tool::Select` a press on an item without Shift
    /// starts a drag of the selection, or of that item alone if it is not selected; a Shift
    /// press or a press on empty sheet is left to selection (not `handled`). `tolerance` is
    /// the K14 hit radius in nm.
    pub fn pointer_down(
        &mut self,
        doc: &Doc,
        at: Pt,
        tolerance: i32,
        selection: &[ItemId],
        shift: bool,
    ) -> ToolOutcome {
        match &mut self.state {
            ToolState::Idle | ToolState::Hovering(_) => {
                if self.tool != Tool::Select || shift {
                    return ToolOutcome::default();
                }
                let Some(hit) = hit_test(doc, at, tolerance, &mut self.scratch) else {
                    return ToolOutcome::default();
                };
                let items = if selection.contains(&hit.item) {
                    selection.to_vec()
                } else {
                    vec![hit.item]
                };
                self.state = ToolState::Dragging(DragGesture {
                    items,
                    pressed: hit.item,
                    anchor: at,
                    current: at,
                    orient: Orient::IDENTITY,
                });
                ToolOutcome {
                    handled: true,
                    preview_changed: true,
                    capture: true,
                    ..ToolOutcome::default()
                }
            }
            // A second press during a drag changes nothing.
            ToolState::Dragging(_) => ToolOutcome::handled(),
            ToolState::Placing(place) => {
                place.at = Some(snap_to(at, GRID_NM));
                ToolOutcome::handled()
            }
            ToolState::Wiring(wire) => {
                wire.cursor = Some(at);
                // T6.2 decides what later clicks commit; T5.2 records the first.
                if wire.start.is_none() {
                    wire.start = Some(at);
                    return ToolOutcome::preview();
                }
                ToolOutcome::handled()
            }
        }
    }

    /// Pointer motion at world point `at`; `left_held` is whether the left button is down.
    /// Hover is not handled here (see [`Self::hover`]).
    pub fn pointer_move(&mut self, at: Pt, left_held: bool) -> ToolOutcome {
        match &mut self.state {
            ToolState::Idle | ToolState::Hovering(_) => ToolOutcome::default(),
            ToolState::Dragging(drag) => {
                if !left_held {
                    // The release was lost: cancel rather than commit a drag nobody ended.
                    return self.cancel();
                }
                let before = drag.delta();
                drag.current = at;
                ToolOutcome {
                    handled: true,
                    preview_changed: drag.delta() != before,
                    ..ToolOutcome::default()
                }
            }
            ToolState::Placing(place) => {
                let next = Some(snap_to(at, GRID_NM));
                let changed = place.at != next;
                place.at = next;
                ToolOutcome {
                    handled: true,
                    preview_changed: changed,
                    ..ToolOutcome::default()
                }
            }
            ToolState::Wiring(wire) => {
                let changed = wire.cursor != Some(at);
                wire.cursor = Some(at);
                ToolOutcome {
                    handled: true,
                    preview_changed: changed,
                    ..ToolOutcome::default()
                }
            }
        }
    }

    /// A left release at world point `at`. `alt` is read from the release event, which
    /// decides the follow policy at commit (WK3).
    pub fn pointer_up(&mut self, at: Pt, alt: bool) -> ToolOutcome {
        match &mut self.state {
            ToolState::Dragging(drag) => {
                drag.current = at;
                let commit = drag.command(self.prefs.follow_policy(alt));
                let request = commit.is_none().then_some(ToolRequest::Click(drag.pressed));
                self.state = ToolState::Idle;
                ToolOutcome {
                    handled: true,
                    preview_changed: true,
                    release: true,
                    commit,
                    request,
                    ..ToolOutcome::default()
                }
            }
            ToolState::Placing(place) => {
                let at = snap_to(at, GRID_NM);
                place.at = Some(at);
                ToolOutcome {
                    request: Some(ToolRequest::Place {
                        symbol: place.symbol,
                        at,
                        orient: place.orient,
                    }),
                    ..ToolOutcome::handled()
                }
            }
            ToolState::Wiring(_) => ToolOutcome::handled(),
            ToolState::Idle | ToolState::Hovering(_) => ToolOutcome::default(),
        }
    }

    /// Esc: ends the innermost gesture without touching the document (INV5). A drag drops
    /// its preview; placing leaves for `Tool::Select`; wiring first drops its start, then
    /// leaves. Returns an unhandled outcome when there was nothing to cancel.
    pub fn cancel(&mut self) -> ToolOutcome {
        match &mut self.state {
            ToolState::Dragging(_) => {
                self.state = ToolState::Idle;
                ToolOutcome {
                    release: true,
                    ..ToolOutcome::preview()
                }
            }
            ToolState::Wiring(wire) if wire.start.is_some() => {
                wire.start = None;
                ToolOutcome::preview()
            }
            ToolState::Placing(_) | ToolState::Wiring(_) => {
                self.tool = Tool::Select;
                self.state = ToolState::Idle;
                ToolOutcome::preview()
            }
            ToolState::Idle | ToolState::Hovering(_) => ToolOutcome::default(),
        }
    }

    /// R, X or Y: composes `by` into a drag's or placement's orientation; outside a gesture
    /// asks the host to turn the selection (T5.3). The wire tool ignores it.
    pub fn reorient(&mut self, by: Orient) -> ToolOutcome {
        match &mut self.state {
            ToolState::Dragging(DragGesture { orient, .. })
            | ToolState::Placing(PlaceGesture { orient, .. }) => {
                *orient = orient.then(by);
                ToolOutcome::preview()
            }
            ToolState::Idle | ToolState::Hovering(_) if self.tool == Tool::Select => ToolOutcome {
                request: Some(ToolRequest::ReorientSelection(by)),
                ..ToolOutcome::handled()
            },
            _ => ToolOutcome::default(),
        }
    }

    /// Del: outside a gesture asks the host to delete the selection (T5.3); ignored
    /// mid-gesture so a drag never commits over deleted items.
    pub fn delete(&mut self) -> ToolOutcome {
        match self.state {
            ToolState::Idle | ToolState::Hovering(_) => ToolOutcome {
                request: Some(ToolRequest::DeleteSelection),
                ..ToolOutcome::handled()
            },
            _ => ToolOutcome::default(),
        }
    }
}

/// Undo availability, emitted by [`SheetCanvas`] after every document step it makes
/// (UD6: the shell drives `can_undo`, `can_redo` and the modified state from it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasHistory {
    pub can_undo: bool,
    pub can_redo: bool,
    /// `Doc::history_cursor` after the step.
    pub cursor: u64,
}

impl CanvasHistory {
    pub fn of(doc: &Doc) -> Self {
        Self {
            can_undo: doc.can_undo(),
            can_redo: doc.can_redo(),
            cursor: doc.history_cursor(),
        }
    }
}

/// The surface a canvas view gives the tool machine and its action handlers.
pub(crate) trait ToolHost: Sized + 'static {
    fn tools(&self) -> &ToolMachine;
    fn tools_mut(&mut self) -> &mut ToolMachine;
    fn tool_doc(&self) -> Option<Entity<Doc>>;
    /// Repaint the preview. Never notifies `SheetView` (INV11).
    fn preview_changed(&mut self, cx: &mut Context<Self>);
    /// The document changed through this host (commit, undo or redo).
    fn doc_changed(&mut self, change: &Change, cx: &mut Context<Self>);
    /// Carry out a request the machine cannot finish alone. Default: not supported yet
    /// (T5.3 adds placement, reorienting and deleting the selection).
    fn request(&mut self, _request: ToolRequest, _cx: &mut Context<Self>) {}
    /// Cancel a gesture the host owns (the rubber band). Returns whether one was cancelled.
    fn cancel_host_gesture(&mut self, _cx: &mut Context<Self>) -> bool {
        false
    }
}

/// Carries out one [`ToolOutcome`]: applies its commit as one `Doc::apply` (INV5), repaints
/// the preview, passes on its request and, given the window, releases a held pointer
/// (pointer handlers release their own capture). Returns the outcome's flags for the
/// caller's pointer bookkeeping.
pub(crate) fn finish<H: ToolHost>(
    host: &mut H,
    mut outcome: ToolOutcome,
    window: Option<&mut Window>,
    cx: &mut Context<H>,
) -> ToolOutcome {
    if let Some(command) = outcome.commit.take() {
        apply(host, command, cx);
    }
    if outcome.preview_changed {
        host.preview_changed(cx);
    }
    if let Some(request) = outcome.request.take() {
        host.request(request, cx);
    }
    if outcome.release
        && let Some(window) = window
        && window.captured_hitbox().is_some()
    {
        window.release_pointer();
    }
    outcome
}

/// Applies `command` as one document step. A refused command changes nothing (INV4), and a
/// validated no-op keeps the revision, so the host hears only of effective edits.
pub(crate) fn apply<H: ToolHost>(host: &mut H, command: Command, cx: &mut Context<H>) {
    let Some(doc) = host.tool_doc() else {
        return;
    };
    let result = doc.update(cx, |doc, _| {
        let before = doc.revision();
        doc.apply(command).ok().filter(|_| doc.revision() != before)
    });
    if let Some(change) = result {
        host.doc_changed(&change, cx);
    }
}

fn history_step<H: ToolHost>(
    host: &mut H,
    redo: bool,
    window: &mut Window,
    cx: &mut Context<H>,
) -> bool {
    // A gesture's preview refers to the current document: drop it before the step.
    let cancel = host.tools_mut().cancel();
    finish(host, cancel, Some(window), cx);
    let Some(doc) = host.tool_doc() else {
        return false;
    };
    let change = doc.update(cx, |doc, _| if redo { doc.redo() } else { doc.undo() });
    match change {
        Some(change) => {
            host.doc_changed(&change, cx);
            true
        }
        None => false,
    }
}

/// Undoes one document step, cancelling any gesture first. Returns whether one was undone.
pub(crate) fn undo<H: ToolHost>(host: &mut H, window: &mut Window, cx: &mut Context<H>) -> bool {
    history_step(host, false, window, cx)
}

/// Redoes one document step, cancelling any gesture first. Returns whether one was redone.
pub(crate) fn redo<H: ToolHost>(host: &mut H, window: &mut Window, cx: &mut Context<H>) -> bool {
    history_step(host, true, window, cx)
}

/// Esc: cancels the tool's gesture or, failing that, the host's. Never edits the document.
pub(crate) fn cancel<H: ToolHost>(host: &mut H, window: &mut Window, cx: &mut Context<H>) -> bool {
    let outcome = host.tools_mut().cancel();
    let handled = finish(host, outcome, Some(window), cx).handled;
    handled || host.cancel_host_gesture(cx)
}

pub(crate) fn set_tool<H: ToolHost>(
    host: &mut H,
    tool: Tool,
    window: Option<&mut Window>,
    cx: &mut Context<H>,
) {
    let outcome = host.tools_mut().set_tool(tool);
    finish(host, outcome, window, cx);
}

fn run<H: ToolHost>(
    host: &mut H,
    step: impl FnOnce(&mut ToolMachine) -> ToolOutcome,
    window: &mut Window,
    cx: &mut Context<H>,
) {
    let outcome = step(host.tools_mut());
    finish(host, outcome, Some(window), cx);
}

/// Puts `element` (the host's focus-tracking root) in the `Sheet` key context and routes
/// every [`crate::keymap`] action to the tool machine (UD3). The shell binds R, X, Y and W
/// to these actions; a standalone canvas also binds Esc, Del, Ctrl+Z and Ctrl+Y.
pub(crate) fn sheet_key_context<H: ToolHost>(element: Div, cx: &mut Context<H>) -> Div {
    element
        .key_context(SHEET_CONTEXT)
        .on_action(cx.listener(|host: &mut H, _: &Rotate, window, cx| {
            run(host, |m| m.reorient(Orient::ROT_90), window, cx);
        }))
        .on_action(
            cx.listener(|host: &mut H, _: &MirrorHorizontal, window, cx| {
                run(host, |m| m.reorient(Orient::FLIP_X), window, cx);
            }),
        )
        .on_action(cx.listener(|host: &mut H, _: &MirrorVertical, window, cx| {
            run(host, |m| m.reorient(Orient::FLIP_Y), window, cx);
        }))
        .on_action(cx.listener(|host: &mut H, _: &PlaceWire, window, cx| {
            set_tool(host, Tool::Wire, Some(window), cx);
        }))
        .on_action(cx.listener(|host: &mut H, _: &Cancel, window, cx| {
            cancel(host, window, cx);
        }))
        .on_action(cx.listener(|host: &mut H, _: &Delete, window, cx| {
            run(host, ToolMachine::delete, window, cx);
        }))
        .on_action(cx.listener(|host: &mut H, _: &Undo, window, cx| {
            undo(host, window, cx);
        }))
        .on_action(cx.listener(|host: &mut H, _: &Redo, window, cx| {
            redo(host, window, cx);
        }))
}

impl OverlayView {
    pub fn tool(&self) -> Tool {
        self.tools().tool()
    }

    pub fn tool_state(&self) -> &ToolState {
        self.tools().state()
    }

    /// Switches tools, cancelling any gesture under way (INV5).
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        set_tool(self, tool, None, cx);
    }

    /// Esc: cancels the tool's gesture, or else a rubber band. Never edits the document.
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        cancel(self, window, cx)
    }

    /// Undoes one document step (one committed gesture, INV5).
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        undo(self, window, cx)
    }

    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        redo(self, window, cx)
    }
}

impl SheetCanvas {
    pub fn tool(&self) -> Tool {
        self.tools().tool()
    }

    pub fn tool_state(&self) -> &ToolState {
        self.tools().state()
    }

    /// Switches tools, cancelling any gesture under way (INV5).
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        set_tool(self, tool, None, cx);
    }

    /// Esc, called by the shell's `NavCancel` while the sheet is the key target (UQ14).
    /// Returns whether a gesture or tool was cancelled. Never edits the document.
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        cancel(self, window, cx)
    }

    /// Undoes one document step and emits [`CanvasHistory`] (UD3: the shell's Ctrl+Z).
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        undo(self, window, cx);
    }

    /// Redoes one document step and emits [`CanvasHistory`] (UD3: the shell's Ctrl+Y).
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        redo(self, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::keymap::standalone_bindings;
    use gpui::{
        AppContext, Bounds, Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext,
    };
    use omnia_doc::{Label, Part, PartId, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;
    use std::{cell::RefCell, rc::Rc};

    const G: i32 = GRID_NM;
    const R1: ItemId = ItemId::Part(PartId(0));
    const C1: ItemId = ItemId::Part(PartId(1));
    /// A hit radius well inside one grid step.
    const TOL: i32 = G / 4;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    /// R1 at (-6, 0) and C1 at (6, 0) grid, a wire between their pins, and a label.
    fn test_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let part = |name: &str, at: Pt, designator: &str| {
            let symbol = lib.by_name(name).unwrap();
            let def = lib.get(symbol);
            (
                Part {
                    symbol,
                    at,
                    orient: Orient::IDENTITY,
                    designator: designator.into(),
                    value: "1".into(),
                },
                def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                def.bounds,
            )
        };
        Doc::from_items(
            [
                part("Resistor-IEEE-Standard", pt(-6, 0), "R1"),
                part("Capacitor-IEEE-NonPolarized", pt(6, 0), "C1"),
            ],
            [Wire {
                a: pt(-4, 0),
                b: pt(4, 0),
            }],
            [Label {
                at: pt(0, 4),
                text: "OUT".into(),
            }],
        )
        .unwrap()
    }

    fn snapshot(doc: &Doc) -> Doc {
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        Doc::load(bytes.as_slice()).unwrap()
    }

    fn symbol() -> SymbolId {
        SymbolLibrary::builtin()
            .by_name("Resistor-IEEE-Standard")
            .unwrap()
    }

    fn dragging(m: &ToolMachine) -> &DragGesture {
        match m.state() {
            ToolState::Dragging(drag) => drag,
            other => panic!("expected a drag, got {other:?}"),
        }
    }

    // ---- state machine transitions ----

    #[test]
    fn t5_2_each_tool_enters_its_rest_state() {
        let mut m = ToolMachine::new();
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));
        assert!(m.set_tool(Tool::Wire).preview_changed);
        assert_eq!(m.state(), &ToolState::Wiring(WireGesture::default()));
        m.set_tool(Tool::Place(symbol()));
        assert_eq!(
            m.state(),
            &ToolState::Placing(PlaceGesture {
                symbol: symbol(),
                at: None,
                orient: Orient::IDENTITY,
            })
        );
        m.set_tool(Tool::Label);
        assert_eq!(m.phase(), ToolPhase::Idle);
        m.set_tool(Tool::Select);
        assert_eq!(m.phase(), ToolPhase::Idle);
        // Choosing the active tool at rest changes nothing.
        assert!(!m.set_tool(Tool::Select).preview_changed);
    }

    #[test]
    fn t5_2_hover_moves_between_idle_and_hovering() {
        let mut m = ToolMachine::new();
        assert!(m.hover(Some(R1)).preview_changed);
        assert_eq!(m.state(), &ToolState::Hovering(R1));
        assert!(!m.hover(Some(R1)).preview_changed);
        assert!(m.hover(Some(C1)).preview_changed);
        assert!(m.hover(None).preview_changed);
        assert_eq!(m.phase(), ToolPhase::Idle);
        // Hover is never handled: selection keeps its own hover outline.
        assert!(!m.hover(Some(R1)).handled);
        // Other tools do not hover.
        m.set_tool(Tool::Wire);
        assert_eq!(m.hover(Some(R1)), ToolOutcome::default());
        assert_eq!(m.phase(), ToolPhase::Wiring);
    }

    #[test]
    fn t5_2_press_on_an_item_drags_the_selection_or_that_item() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        let out = m.pointer_down(&doc, pt(-6, 0), TOL, &[R1, C1], false);
        assert!(out.handled && out.capture && out.commit.is_none());
        let drag = dragging(&m);
        assert_eq!(drag.items, vec![R1, C1]);
        assert_eq!(drag.pressed, R1);

        // Not selected: the drag takes the pressed item alone.
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[C1], false);
        assert_eq!(dragging(&m).items, vec![R1]);

        // Shift, empty sheet and other tools are left to selection.
        let mut m = ToolMachine::new();
        assert!(!m.pointer_down(&doc, pt(-6, 0), TOL, &[], true).handled);
        assert!(!m.pointer_down(&doc, pt(0, -8), TOL, &[], false).handled);
        m.set_tool(Tool::Label);
        assert!(!m.pointer_down(&doc, pt(-6, 0), TOL, &[], false).handled);
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[test]
    fn t5_2_drag_delta_snaps_to_the_grid() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        // Under half a grid step: no movement, so no repaint.
        let out = m.pointer_move(
            Pt {
                x: -6 * G + G / 3,
                y: 0,
            },
            true,
        );
        assert!(out.handled && !out.preview_changed);
        let out = m.pointer_move(
            Pt {
                x: -4 * G + G / 3,
                y: -G - G / 3,
            },
            true,
        );
        assert!(out.preview_changed);
        assert_eq!(dragging(&m).delta(), pt(2, -1));
        let out = m.pointer_up(
            Pt {
                x: -4 * G + 7,
                y: -G + 3,
            },
            false,
        );
        assert_eq!(
            out.commit,
            Some(Command::Move {
                items: vec![R1],
                delta: pt(2, -1),
                follow: WiringPrefs::default().follow_policy(false),
            })
        );
        assert!(out.release && out.request.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[test]
    fn t5_2_release_reads_alt_for_the_follow_policy() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.pointer_move(pt(-5, 0), true);
        let Some(Command::Move { follow, .. }) = m.pointer_up(pt(-5, 0), true).commit else {
            panic!("expected a move");
        };
        assert_eq!(follow, WiringPrefs::default().follow_policy(true));
    }

    #[test]
    fn t5_2_a_click_without_movement_commits_nothing() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1, C1], false);
        let out = m.pointer_up(pt(-6, 0), false);
        assert_eq!(out.commit, None);
        assert_eq!(out.request, Some(ToolRequest::Click(R1)));
        assert!(out.release);
    }

    #[test]
    fn t5_2_a_lost_release_cancels_the_drag() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.pointer_move(pt(-3, 0), true);
        let out = m.pointer_move(pt(-2, 0), false);
        assert!(out.release && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);
        // The release that follows is not the tool's.
        assert_eq!(m.pointer_up(pt(-2, 0), false), ToolOutcome::default());
    }

    #[test]
    fn t5_2_r_x_y_compose_during_a_drag_into_one_batch() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.pointer_move(pt(-4, 1), true);
        for by in [Orient::ROT_90, Orient::FLIP_X, Orient::FLIP_Y] {
            let out = m.reorient(by);
            assert!(out.handled && out.preview_changed && out.request.is_none());
        }
        let by = Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y);
        assert_eq!(dragging(&m).orient, by);
        let follow = WiringPrefs::default().follow_policy(false);
        assert_eq!(
            m.pointer_up(pt(-4, 1), false).commit,
            Some(Command::Batch(vec![
                Command::Move {
                    items: vec![R1],
                    delta: pt(2, 1),
                    follow,
                },
                Command::Reorient {
                    items: vec![R1],
                    pivot: pt(-4, 1),
                    by,
                    follow,
                },
            ]))
        );

        // Turned in place without moving: a lone Reorient.
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.reorient(Orient::ROT_90);
        assert!(matches!(
            m.pointer_up(pt(-6, 0), false).commit,
            Some(Command::Reorient {
                by: Orient::ROT_90,
                ..
            })
        ));
    }

    #[test]
    fn t5_2_r_x_y_and_del_outside_a_gesture_are_requests() {
        let mut m = ToolMachine::new();
        assert_eq!(
            m.reorient(Orient::FLIP_X).request,
            Some(ToolRequest::ReorientSelection(Orient::FLIP_X))
        );
        assert_eq!(m.delete().request, Some(ToolRequest::DeleteSelection));
        m.hover(Some(R1));
        assert_eq!(m.delete().request, Some(ToolRequest::DeleteSelection));
        // Not mid-drag, and not in the wire tool.
        let doc = test_doc();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        assert_eq!(m.delete(), ToolOutcome::default());
        m.set_tool(Tool::Wire);
        assert_eq!(m.reorient(Orient::ROT_90), ToolOutcome::default());
        assert_eq!(m.delete(), ToolOutcome::default());
    }

    #[test]
    fn t5_2_placing_follows_the_grid_and_requests_a_placement() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.set_tool(Tool::Place(symbol()));
        let out = m.pointer_move(
            Pt {
                x: 3 * G + 10,
                y: -G - 10,
            },
            false,
        );
        assert!(out.handled && out.preview_changed);
        let out = m.pointer_move(
            Pt {
                x: 3 * G - 10,
                y: -G + 10,
            },
            false,
        );
        assert!(!out.preview_changed);
        assert!(m.reorient(Orient::ROT_90).preview_changed);
        assert!(m.pointer_down(&doc, pt(3, -1), TOL, &[], false).handled);
        let out = m.pointer_up(
            Pt {
                x: 3 * G + 5,
                y: -G,
            },
            false,
        );
        assert_eq!(
            out.request,
            Some(ToolRequest::Place {
                symbol: symbol(),
                at: pt(3, -1),
                orient: Orient::ROT_90,
            })
        );
        assert_eq!(out.commit, None);
        // Placing continues until Esc, which returns to Select.
        assert_eq!(m.phase(), ToolPhase::Placing);
        assert!(m.cancel().handled);
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));
    }

    #[test]
    fn t5_2_wiring_esc_drops_the_start_then_leaves_the_tool() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.set_tool(Tool::Wire);
        assert!(m.pointer_move(pt(1, 1), false).preview_changed);
        assert!(
            m.pointer_down(&doc, pt(1, 1), TOL, &[], false)
                .preview_changed
        );
        assert!(m.pointer_down(&doc, pt(5, 1), TOL, &[], false).handled);
        assert!(m.pointer_up(pt(5, 1), false).commit.is_none());
        assert_eq!(
            m.state(),
            &ToolState::Wiring(WireGesture {
                start: Some(pt(1, 1)),
                cursor: Some(pt(5, 1)),
            })
        );
        assert!(m.cancel().handled);
        assert_eq!(m.tool(), Tool::Wire);
        assert!(matches!(m.state(), ToolState::Wiring(w) if w.start.is_none()));
        assert!(m.cancel().handled);
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));
        // Nothing left to cancel.
        assert!(!m.cancel().handled);
    }

    #[test]
    fn t5_2_switching_tools_mid_drag_cancels_it() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.pointer_move(pt(-2, 0), true);
        let out = m.set_tool(Tool::Wire);
        assert!(out.release && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Wiring);
    }

    #[test]
    fn t5_2_routes_the_pointer_only_during_gestures() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        assert!(!m.routes_pointer());
        m.hover(Some(R1));
        assert!(!m.routes_pointer());
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        assert!(m.routes_pointer() && m.is_dragging());
        m.cancel();
        m.set_tool(Tool::Wire);
        assert!(m.routes_pointer() && !m.is_dragging());
        m.set_tool(Tool::Place(symbol()));
        assert!(m.routes_pointer());
    }

    /// INV5 on the pure machine: the revision holds through the drag until release, the
    /// commit is one step that one undo reverts, and Esc mid-drag commits nothing.
    #[test]
    fn inv5_drag_is_one_undo_step_and_esc_commits_nothing() {
        let mut doc = test_doc();
        let original = snapshot(&doc);
        let rev0 = doc.revision();
        let mut m = ToolMachine::new();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        for step in 1..=20 {
            let at = Pt {
                x: -6 * G + step * G / 4,
                y: step * G / 7,
            };
            assert!(m.pointer_move(at, true).commit.is_none());
            assert_eq!(
                doc.revision(),
                rev0,
                "INV5: the drag must not edit the document"
            );
        }
        let commit = m
            .pointer_up(pt(-1, 3), false)
            .commit
            .expect("an effective drag");
        doc.apply(commit).unwrap();
        assert_eq!(doc.revision(), rev0 + 1);
        assert_eq!(doc.part(PartId(0)).unwrap().at, pt(-1, 3));
        assert!(doc.undo().is_some());
        assert!(
            doc.content_eq(&original),
            "INV5: one undo reverts the whole drag"
        );
        assert!(!doc.can_undo());

        let rev = doc.revision();
        m.pointer_down(&doc, pt(-6, 0), TOL, &[R1], false);
        m.pointer_move(pt(0, 0), true);
        let out = m.cancel();
        assert!(out.handled && out.release && out.commit.is_none());
        assert_eq!(m.pointer_up(pt(0, 0), false).commit, None);
        assert_eq!(
            doc.revision(),
            rev,
            "INV5: Esc leaves the revision unchanged"
        );
    }

    // ---- GPUI: simulated input, actions and key bindings ----

    fn open_overlay(
        cx: &mut TestAppContext,
    ) -> (Entity<OverlayView>, Entity<Doc>, &mut VisualTestContext) {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            // Keys reach the focused element only in an active window (headless quirk).
            window.activate_window();
            window.focus(view.focus_handle(), cx);
            view
        });
        cx.run_until_parked();
        (overlay, doc, cx)
    }

    fn screen(
        overlay: &Entity<OverlayView>,
        cx: &mut VisualTestContext,
        p: Pt,
    ) -> gpui::Point<Pixels> {
        cx.update(|_, cx| overlay.read(cx).camera().world_to_pixels(p))
    }

    fn revision(doc: &Entity<Doc>, cx: &mut VisualTestContext) -> u64 {
        cx.update(|_, cx| doc.read(cx).revision())
    }

    fn phase(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext) -> ToolPhase {
        cx.update(|_, cx| overlay.read(cx).tool_state().phase())
    }

    fn drag_to(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext, p: Pt) {
        let at = screen(overlay, cx, p);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }

    /// INV5 through simulated GPUI input: the revision holds through a drag until mouse-up,
    /// Ctrl+Z reverts the whole drag in one step, and Esc mid-drag changes nothing.
    #[gpui::test]
    fn inv5_simulated_drag_commits_once_and_esc_cancels(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));
        let rev0 = revision(&doc, cx);

        let press = screen(&overlay, cx, pt(-6, 0));
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );
        for step in 1..=12 {
            let at = Pt {
                x: -6 * G + step * G / 3,
                y: step * G / 4,
            };
            drag_to(&overlay, cx, at);
            assert_eq!(
                revision(&doc, cx),
                rev0,
                "INV5: no edit before mouse-up (step {step})"
            );
        }
        drag_to(&overlay, cx, pt(-2, 3));
        assert_eq!(revision(&doc, cx), rev0);
        let release = screen(&overlay, cx, pt(-2, 3));
        cx.simulate_mouse_up(release, MouseButton::Left, Modifiers::none());
        assert_eq!(
            revision(&doc, cx),
            rev0 + 1,
            "INV5: the drag commits once, on mouse-up"
        );
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().at),
            pt(-2, 3)
        );
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        let moved = cx.update(|_, cx| snapshot(doc.read(cx)));

        cx.simulate_keystrokes("ctrl-z");
        assert!(
            cx.update(|_, cx| doc.read(cx).content_eq(&original)),
            "INV5: one undo reverts the whole drag"
        );
        assert!(!cx.update(|_, cx| doc.read(cx).can_undo()));
        cx.simulate_keystrokes("ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&moved)));

        // Esc mid-drag: nothing commits, even when the button is released afterwards.
        let rev = revision(&doc, cx);
        let press = screen(&overlay, cx, pt(-2, 3));
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        drag_to(&overlay, cx, pt(4, -5));
        cx.simulate_keystrokes("escape");
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        let release = screen(&overlay, cx, pt(4, -5));
        cx.simulate_mouse_up(release, MouseButton::Left, Modifiers::none());
        assert_eq!(
            revision(&doc, cx),
            rev,
            "INV5: Esc leaves the revision unchanged"
        );
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&moved)));
    }

    #[gpui::test]
    fn t5_2_a_drag_preview_does_not_repaint_the_sheet(cx: &mut TestAppContext) {
        // INV11: only a commit's dirty rectangle repaints the sheet; the preview never does.
        let (overlay, _doc, cx) = open_overlay(cx);
        let paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let press = screen(&overlay, cx, pt(-6, 0));
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        for step in 1..=8 {
            drag_to(
                &overlay,
                cx,
                Pt {
                    x: -6 * G + step * G / 2,
                    y: 0,
                },
            );
        }
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints
        );
        // Cancel through the shell's path (`NavCancel` calls `cancel`, UQ14). A keystroke here
        // would repaint the sheet once for a GPUI reason, not ours: the pinned `Window`
        // refreshes when input switches between pointer and keyboard (`last_input_modality`).
        assert!(cx.update(|window, cx| overlay.update(cx, |o, cx| o.cancel(window, cx))));
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints
        );
    }

    #[gpui::test]
    fn t5_2_keys_drive_the_tool(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let tool = |cx: &mut VisualTestContext| cx.update(|_, cx| overlay.read(cx).tool());

        // W starts the wire tool; Esc leaves it.
        cx.simulate_keystrokes("w");
        assert_eq!(tool(cx), Tool::Wire);
        assert_eq!(phase(&overlay, cx), ToolPhase::Wiring);
        cx.simulate_keystrokes("escape");
        assert_eq!(tool(cx), Tool::Select);

        // R, X and Y turn a drag under way; Del is ignored mid-drag.
        let rev0 = revision(&doc, cx);
        let press = screen(&overlay, cx, pt(-6, 0));
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        cx.simulate_keystrokes("r x y delete");
        let state = cx.update(|_, cx| overlay.read(cx).tool_state().clone());
        let ToolState::Dragging(drag) = state else {
            panic!("expected a drag, got {state:?}");
        };
        assert_eq!(
            drag.orient,
            Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y)
        );
        assert_eq!(revision(&doc, cx), rev0);
        cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::none());
        assert_eq!(
            revision(&doc, cx),
            rev0 + 1,
            "the turned drag commits one step"
        );
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(revision(&doc, cx), rev0 + 2);
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::IDENTITY
        );

        // With no tool gesture, Esc cancels a rubber band instead.
        let empty = screen(&overlay, cx, pt(0, -8));
        cx.simulate_mouse_down(empty, MouseButton::Left, Modifiers::none());
        drag_to(&overlay, cx, pt(3, -6));
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_some()));
        cx.simulate_keystrokes("escape");
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
    }

    #[gpui::test]
    fn t5_2_keys_need_the_sheet_focused(cx: &mut TestAppContext) {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let doc = cx.new(|_| test_doc());
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            OverlayView::new(doc, camera, &GRAPHITE, cx)
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("w");
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Select);
    }

    #[gpui::test]
    fn t5_2_sheet_canvas_undo_redo_emit_history(cx: &mut TestAppContext) {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (canvas, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let canvas = SheetCanvas::new(camera, &GRAPHITE, cx).with_doc(doc_clone);
            window.activate_window();
            window.focus(canvas.focus_handle(), cx);
            canvas
        });
        cx.run_until_parked();
        let history = Rc::new(RefCell::new(Vec::new()));
        let sink = history.clone();
        cx.update(|_, cx| {
            cx.subscribe(&canvas, move |_, event: &CanvasHistory, _| {
                sink.borrow_mut().push(*event);
            })
            .detach();
        });
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));
        cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::Move {
                    items: vec![R1],
                    delta: pt(0, 2),
                    follow: WiringPrefs::default().follow_policy(false),
                })
                .unwrap();
            })
        });
        let moved_cursor = cx.update(|_, cx| doc.read(cx).history_cursor());

        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        cx.simulate_keystrokes("ctrl-y");
        // Nothing left to redo: no step and no event.
        cx.simulate_keystrokes("ctrl-y");
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).history_cursor()),
            moved_cursor
        );
        let events = history.borrow().clone();
        assert_eq!(events.len(), 2);
        assert_eq!((events[0].can_undo, events[0].can_redo), (false, true));
        assert_eq!(
            events[1],
            CanvasHistory {
                can_undo: true,
                can_redo: false,
                cursor: moved_cursor,
            }
        );

        // W and the shell's Esc path (`cancel`) work the canvas's tool too.
        cx.simulate_keystrokes("w");
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool()), Tool::Wire);
        assert!(cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool()), Tool::Select);
        assert!(!cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
    }
}
