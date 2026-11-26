//! T5.2: the tool state machine and the canvas actions' handlers (canvas-plan section 5, INV5).
//!
//! Input goes to the active tool. During a gesture the tool holds preview state and the
//! document does not change. On commit the tool hands its host exactly one [`Command`],
//! stamped with the document it was planned against ([`DocStamp`]). The host applies it with
//! one `Doc::apply` only if that document is still the one shown, at the same revision, so a
//! committed gesture is exactly one undo step and stale input never edits another document.
//! Cancelling (Esc) drops the preview and leaves the revision unchanged (INV5).
//!
//! [`ToolMachine`] is pure: it reads the document only to hit-test a press and never edits
//! it, so every transition is tested without a window. Its states:
//!
//! | State | Entered by | Left by |
//! | --- | --- | --- |
//! | [`ToolState::Idle`] | `Tool::Select` or `Tool::Label` with nothing under the pointer | hover, press on an item, tool change |
//! | [`ToolState::Hovering`] | the pointer over an item in `Tool::Select` | pointer off the item, press |
//! | [`ToolState::Dragging`] | a left press on a part, label or multi-item selection in `Tool::Select` without Shift | release (commit); Esc, a lost release, a fresh press, window deactivation, a document swap or a tool change (no edit) |
//! | [`ToolState::DraggingSegment`] | a left press on a single wire segment in `Tool::Select` without Shift | release (commits one `Command::DragSegment` if accepted or cancels if refused); Esc, a lost release, a fresh press, window deactivation, a document swap or a tool change (no edit) |
//! | [`ToolState::Placing`] | `Tool::Place(symbol)` | Esc or a tool change; a press and release in the tool request a placement and stay |
//! | [`ToolState::Wiring`] | `Tool::Wire` (W) | Esc (first drops the start, then leaves the tool) or a tool change |
//!
//! One gesture owner: [`OverlayView`] holds the only [`ToolMachine`] (it owns the selection
//! and the pointer). [`SheetCanvas`], the shell-facing canvas, delegates tool, cancel, undo
//! and redo to the overlay attached with `SheetCanvas::set_overlay`, and re-emits the
//! overlay's [`CanvasHistory`], which the overlay emits after every effective edit.
//!
//! Seams for the tasks that build on this one: a placement yields [`ToolRequest::Place`] for
//! T5.3's placement provider (`tool/place.rs`), carried out by the overlay's
//! `ToolHost::request`; the drag commit is [`DragGesture::command`], which T5.3
//! (`tool/move_.rs`) and W2.3 extend with ghosts, anchor-pin snapping and follow previews;
//! R, X and Y outside a gesture yield [`ToolRequest::ReorientSelection`] and Del yields
//! [`ToolRequest::DeleteSelection`] for T5.3; [`WireGesture`] holds the wire tool's cursor
//! and start for T6.2 (`tool/wire.rs`). R, X and Y during a drag are ignored until omnia-doc
//! has one command for a combined move and reorient (T5.2 review, finding 3): a `Move` then
//! a `Reorient` in one `Batch` normalises the sheet in between and can refuse a valid final
//! position.
//!
//! The GPUI side: [`sheet_key_context`] puts a view's focus element in the `Sheet` key
//! context and routes the [`crate::keymap`] actions through [`SheetActions`].

pub mod move_;
pub mod place;
pub mod segment;

pub use place::{PlaceProvider, PlaceRequest};
pub use segment::{SegmentDragGesture, SegmentPreview};

use crate::input::SheetCanvas;
use crate::keymap::{
    Cancel, Delete, MirrorHorizontal, MirrorVertical, PlaceWire, Redo, Rotate, SHEET_CONTEXT, Undo,
};
use crate::overlay::{GestureEndReason, OverlayView};
use gpui::{App, Context, Div, Entity, InteractiveElement, Window};
use omnia_doc::{
    Change, Command, Doc, Isometry, ItemId, Orient, Pt, Rect, SymbolId, hit_test,
    prefs::{FollowPolicy, WiringPrefs},
};
use std::rc::Weak;

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

/// The document a gesture was planned against: the host's identity for the document entity
/// and its revision at the press. A commit applies only to that document at that revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DocStamp {
    pub key: u64,
    pub revision: u64,
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
    /// Deterministic, lattice-safe pivot for mid-drag R, X and Y reorientation (T5.3, T5.5).
    pub pivot: Pt,
    /// Orientation composed from R, X and Y pressed during the drag (T5.3, T5.5).
    pub by: Orient,
    /// The document the drag started on.
    pub base: DocStamp,
    /// World position of the dragged part's anchor pin (pin 0) at drag start for K2 snapping.
    anchor_pin: Option<Pt>,
    /// True once the drag has actually moved or reoriented off its starting position (T5.3, D7,
    /// INV11). Plain selection clicks never lift, keeping click-selection SheetView paints at 0.
    lifted: bool,
}

impl DragGesture {
    /// Whether the drag has visually lifted off the sheet (hidden on `SheetView`, drawn as ghost).
    pub fn is_lifted(&self) -> bool {
        self.lifted
    }

    /// The pointer movement, snapped so a dragged part's anchor pin lands on `GRID_NM` (K2),
    /// or snapped directly to `GRID_NM` for wire/label-only drags.
    pub fn delta(&self) -> Pt {
        move_::drag_delta(
            self.anchor,
            self.current,
            self.pivot,
            self.by,
            self.anchor_pin,
        )
    }

    /// The live preview and commit [`Isometry`] for this drag (T5.3, T5.5).
    pub fn isometry(&self) -> Isometry {
        move_::drag_isometry(self.delta(), self.pivot, self.by)
    }

    /// The one command this drag commits: `Command::Move`, `Command::Transform` (when R, X or Y
    /// was applied mid-drag, T5.5), or `None` for a click that changed nothing (INV5).
    pub fn command(&self, follow: FollowPolicy) -> Option<Command> {
        move_::drag_command(&self.items, self.delta(), self.pivot, self.by, follow)
    }
}

/// `Tool::Place` state: where the part would go and how it is turned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceGesture {
    pub symbol: SymbolId,
    /// The snapped placement origin under the pointer, once the pointer has been over the sheet.
    /// Snapped so anchor pin 0 lands on `GRID_NM` (K2).
    pub at: Option<Pt>,
    /// R, X and Y pressed while placing, composed in order.
    pub orient: Orient,
    /// A left press was made in this tool and not yet released: only its release places.
    pub pressed: bool,
    /// Last raw world cursor position, kept so R/X/Y re-snaps `at` for the new orientation.
    raw_cursor: Option<Pt>,
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
    /// W3.3: dragging a single wire segment perpendicular to its axis (`Command::DragSegment`, WD7).
    DraggingSegment(SegmentDragGesture),
    Placing(PlaceGesture),
    Wiring(WireGesture),
}

impl ToolState {
    pub fn phase(&self) -> ToolPhase {
        match self {
            Self::Idle => ToolPhase::Idle,
            Self::Hovering(_) => ToolPhase::Hovering,
            Self::Dragging(_) | Self::DraggingSegment(_) => ToolPhase::Dragging,
            Self::Placing(_) => ToolPhase::Placing,
            Self::Wiring(_) => ToolPhase::Wiring,
        }
    }
}

/// Work the machine cannot finish alone; the host carries it out (seams for T5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolRequest {
    /// A press on an item that did not become a drag: make it the only selection, if it
    /// still exists in the document the press was made on (`base`).
    Click { item: ItemId, base: DocStamp },
    /// A press and release in `Tool::Place`: T5.3 asks the placement provider for the command.
    Place {
        symbol: SymbolId,
        at: Pt,
        orient: Orient,
    },
    /// R, X or Y outside a gesture: T5.3 turns the selection with one command.
    ReorientSelection(Orient),
    /// Del outside a gesture: T5.3's `delete_selection`. The host reads
    /// `WiringPrefs::delete_wires_with_part` when it carries this out, so every Delete path
    /// (the key, `OverlayView::delete_selection`, `SheetCanvas::delete_selection`) builds one
    /// `Command::Delete` with the current preference (WD10, WQ4, W3.5). T7.1's Cut must do
    /// the same.
    DeleteSelection,
}

/// One gesture's edit, bound to the document it was planned against.
#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub command: Command,
    pub base: DocStamp,
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
    /// The one edit a gesture commits (INV5).
    pub commit: Option<Commit>,
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

    /// The follow policy is read from these at commit (WD12, WK3), and
    /// `delete_wires_with_part` when the host carries out [`ToolRequest::DeleteSelection`]
    /// (WD10), so a change made mid-gesture applies to the next edit, never a preview.
    pub fn set_prefs(&mut self, prefs: WiringPrefs) {
        self.prefs = prefs;
    }

    /// A pointer-driven drag holds the pointer until release.
    pub fn is_dragging(&self) -> bool {
        match &self.state {
            ToolState::Dragging(_) => true,
            ToolState::DraggingSegment(seg) => seg.is_pointer_drag(),
            _ => false,
        }
    }

    /// Whether pointer moves and releases go to the tool instead of hover and selection.
    pub fn routes_pointer(&self) -> bool {
        match &self.state {
            ToolState::Dragging(_) | ToolState::Placing(_) | ToolState::Wiring(_) => true,
            ToolState::DraggingSegment(seg) => seg.is_pointer_drag(),
            ToolState::Idle | ToolState::Hovering(_) => false,
        }
    }

    /// Installs a non-capturing segment-drag preview (golden rendering / programmatic inspection).
    pub(crate) fn set_segment_preview(&mut self, gesture: SegmentDragGesture) {
        self.state = ToolState::DraggingSegment(gesture);
    }

    fn rest_state(tool: Tool) -> ToolState {
        match tool {
            Tool::Select | Tool::Label => ToolState::Idle,
            Tool::Wire => ToolState::Wiring(WireGesture::default()),
            Tool::Place(symbol) => ToolState::Placing(PlaceGesture {
                symbol,
                at: None,
                orient: Orient::IDENTITY,
                pressed: false,
                raw_cursor: None,
            }),
        }
    }

    /// Returns to the current tool's rest state with no edit.
    fn reset(&mut self) -> ToolOutcome {
        let release = self.is_dragging();
        let next = Self::rest_state(self.tool);
        let changed = self.state != next;
        self.state = next;
        ToolOutcome {
            handled: changed,
            preview_changed: changed,
            release,
            ..ToolOutcome::default()
        }
    }

    /// Switches tools. Any gesture under way is cancelled first (revision unchanged, INV5);
    /// a press made in the old tool never completes in the new one.
    pub fn set_tool(&mut self, tool: Tool) -> ToolOutcome {
        let changed = self.tool != tool;
        self.tool = tool;
        let mut outcome = self.reset();
        outcome.handled = true;
        outcome.preview_changed |= changed;
        outcome
    }

    /// The document was replaced: drop every gesture with no edit, keeping the tool.
    pub fn document_replaced(&mut self) -> ToolOutcome {
        self.reset()
    }

    /// The pointer can no longer be trusted (window deactivated): end a drag or a pending
    /// placement press with no edit. Wiring keeps its start; Esc drops it.
    pub fn pointer_lost(&mut self) -> ToolOutcome {
        match &mut self.state {
            ToolState::Dragging(_) | ToolState::DraggingSegment(_) => self.reset(),
            ToolState::Placing(place) if place.pressed => {
                place.pressed = false;
                ToolOutcome::handled()
            }
            _ => ToolOutcome::default(),
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

    /// A left press at world point `at` on the document identified by `doc_key`. In
    /// `Tool::Select` a press on an item without Shift starts a drag of the selection, or of
    /// that item alone if it is not selected; a Shift press or a press on empty sheet is
    /// left to selection (not `handled`). A press during a drag means its release was lost:
    /// the stale drag ends with no edit and the press is taken as a new one. `tolerance` is
    /// the K14 hit radius in nm.
    pub fn pointer_down(
        &mut self,
        doc: &Doc,
        doc_key: u64,
        at: Pt,
        tolerance: i32,
        selection: &[ItemId],
        shift: bool,
    ) -> ToolOutcome {
        let stale = if self.is_dragging() || matches!(self.state, ToolState::DraggingSegment(_)) {
            self.reset()
        } else {
            ToolOutcome::default()
        };
        let mut outcome = match &mut self.state {
            ToolState::Idle | ToolState::Hovering(_) => {
                if self.tool != Tool::Select || shift {
                    ToolOutcome::default()
                } else if let Some(hit) = hit_test(doc, at, tolerance, &mut self.scratch) {
                    let items = if selection.contains(&hit.item) {
                        selection.to_vec()
                    } else {
                        vec![hit.item]
                    };
                    let base = DocStamp {
                        key: doc_key,
                        revision: doc.revision(),
                    };
                    // W3.3 / WD7: dragging a single wire segment starts a perpendicular segment
                    // drag rather than a whole-item Move.
                    if let [ItemId::Wire(wire)] = items.as_slice()
                        && let Some(gesture) = SegmentDragGesture::start(doc, *wire, at, base)
                    {
                        self.state = ToolState::DraggingSegment(gesture);
                    } else {
                        let pivot = move_::selection_pivot(doc, &items).unwrap_or(at);
                        let anchor_pin = move_::drag_anchor_pin(doc, hit.item, &items);
                        self.state = ToolState::Dragging(DragGesture {
                            items,
                            pressed: hit.item,
                            anchor: at,
                            current: at,
                            pivot,
                            by: Orient::IDENTITY,
                            base,
                            anchor_pin,
                            lifted: false,
                        });
                    }
                    ToolOutcome {
                        handled: true,
                        preview_changed: true,
                        capture: true,
                        ..ToolOutcome::default()
                    }
                } else {
                    ToolOutcome::default()
                }
            }
            ToolState::Dragging(_) | ToolState::DraggingSegment(_) => {
                unreachable!("a stale drag was reset above")
            }
            ToolState::Placing(place) => {
                let next = Some(place::snap_place_origin(place.symbol, at, place.orient));
                let changed = place.at != next;
                place.raw_cursor = Some(at);
                place.at = next;
                place.pressed = true;
                ToolOutcome {
                    handled: true,
                    preview_changed: changed,
                    ..ToolOutcome::default()
                }
            }
            ToolState::Wiring(wire) => {
                wire.cursor = Some(at);
                // T6.2 decides what later clicks commit; T5.2 records the first.
                if wire.start.is_none() {
                    wire.start = Some(at);
                    ToolOutcome::preview()
                } else {
                    ToolOutcome::handled()
                }
            }
        };
        outcome.release |= stale.release;
        outcome.preview_changed |= stale.preview_changed;
        outcome
    }

    /// Pointer motion at world point `at`; `left_held` is whether the left button is down.
    /// Hover is not handled here (see [`Self::hover`]).
    ///
    /// Without `&Doc`, an active `DraggingSegment` previews attempted wires as
    /// [`SegmentPreview::Accepted`] without calling [`omnia_doc::route::plan_segment_drag`];
    /// use [`Self::pointer_move_with_doc`] for live planning.
    pub fn pointer_move(&mut self, at: Pt, left_held: bool) -> ToolOutcome {
        self.pointer_move_inner(None, at, left_held)
    }

    /// Like [`Self::pointer_move`], with read access to `doc` so a live segment drag runs
    /// [`omnia_doc::route::plan_segment_drag`] whenever its snapped offset changes (WD7, WD11).
    pub fn pointer_move_with_doc(&mut self, doc: &Doc, at: Pt, left_held: bool) -> ToolOutcome {
        self.pointer_move_inner(Some(doc), at, left_held)
    }

    fn pointer_move_inner(&mut self, doc: Option<&Doc>, at: Pt, left_held: bool) -> ToolOutcome {
        match &mut self.state {
            ToolState::Idle | ToolState::Hovering(_) => ToolOutcome::default(),
            ToolState::Dragging(drag) => {
                if !left_held {
                    // The release was lost: cancel rather than commit a drag nobody ended.
                    return self.cancel();
                }
                let before = drag.delta();
                drag.current = at;
                let after = drag.delta();
                if after != Pt::ZERO {
                    drag.lifted = true;
                }
                ToolOutcome {
                    handled: true,
                    preview_changed: after != before,
                    ..ToolOutcome::default()
                }
            }
            ToolState::DraggingSegment(seg) => {
                if !left_held {
                    return self.cancel();
                }
                let changed = seg.update(doc, at);
                ToolOutcome {
                    handled: true,
                    preview_changed: changed,
                    ..ToolOutcome::default()
                }
            }
            ToolState::Placing(place) => {
                if !left_held {
                    place.pressed = false;
                }
                place.raw_cursor = Some(at);
                let next = Some(place::snap_place_origin(place.symbol, at, place.orient));
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
    ///
    /// Without `&Doc`, an active `DraggingSegment` re-plans its release offset without calling
    /// [`omnia_doc::route::plan_segment_drag`] (`Doc::apply` still validates on commit); use
    /// [`Self::pointer_up_with_doc`] for live planning and refusal handling.
    pub fn pointer_up(&mut self, at: Pt, alt: bool) -> ToolOutcome {
        self.pointer_up_inner(None, at, alt)
    }

    /// Like [`Self::pointer_up`], with read access to `doc` so a segment drag re-plans if the
    /// release moved onto a new grid step before deciding commit vs refusal cancel (WD7, WD11).
    pub fn pointer_up_with_doc(&mut self, doc: &Doc, at: Pt, alt: bool) -> ToolOutcome {
        self.pointer_up_inner(Some(doc), at, alt)
    }

    fn pointer_up_inner(&mut self, doc: Option<&Doc>, at: Pt, alt: bool) -> ToolOutcome {
        match &mut self.state {
            ToolState::Dragging(drag) => {
                drag.current = at;
                let commit = drag
                    .command(self.prefs.follow_policy(alt))
                    .map(|command| Commit {
                        command,
                        base: drag.base,
                    });
                let request = commit.is_none().then_some(ToolRequest::Click {
                    item: drag.pressed,
                    base: drag.base,
                });
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
            ToolState::DraggingSegment(seg) => {
                seg.update(doc, at);
                let commit = seg.command().map(|command| Commit {
                    command,
                    base: seg.base,
                });
                let request = (seg.offset() == 0).then_some(ToolRequest::Click {
                    item: ItemId::Wire(seg.wire),
                    base: seg.base,
                });
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
                place.raw_cursor = Some(at);
                let at = place::snap_place_origin(place.symbol, at, place.orient);
                place.at = Some(at);
                // Only a press made in this tool places: a release left over from another
                // tool (a drag or band cut short by the switch) does nothing.
                let pressed = std::mem::take(&mut place.pressed);
                ToolOutcome {
                    request: pressed.then_some(ToolRequest::Place {
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
            ToolState::Dragging(_) | ToolState::DraggingSegment(_) => self.reset(),
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

    /// R, X or Y: composes `by` into an active drag (T5.3/T5.5, committed as `Command::Transform`)
    /// or placement preview (re-snapping anchor pin 0); outside a gesture asks the host to turn
    /// the selection with `Command::Reorient`. Ignored in `Tool::Wire`.
    pub fn reorient(&mut self, by: Orient) -> ToolOutcome {
        if by == Orient::IDENTITY {
            return ToolOutcome::default();
        }
        match &mut self.state {
            ToolState::Dragging(drag) => {
                drag.by = drag.by.then(by);
                drag.lifted = true;
                ToolOutcome::preview()
            }
            ToolState::Placing(place) => {
                place.orient = place.orient.then(by);
                if let Some(raw) = place.raw_cursor {
                    place.at = Some(place::snap_place_origin(place.symbol, raw, place.orient));
                }
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

/// Undo availability, emitted by [`OverlayView`] after every effective edit it makes (a
/// gesture's commit, undo, redo; later placement and delete) and re-emitted by
/// [`SheetCanvas`] (UD6: the shell drives `can_undo`, `can_redo` and the modified state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasHistory {
    /// The document the edit was made on (`doc_key`); check it on delivery, because events
    /// queued within one GPUI update arrive after any document switch in that update.
    pub doc: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    /// `Doc::history_cursor` after the edit.
    pub cursor: u64,
}

impl CanvasHistory {
    pub fn of(key: u64, doc: &Doc) -> Self {
        Self {
            doc: key,
            can_undo: doc.can_undo(),
            can_redo: doc.can_redo(),
            cursor: doc.history_cursor(),
        }
    }
}

/// An effective edit made through the gesture owner: where the sheet changed (D7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanvasEdit {
    /// The document the edit was made on (`doc_key`), checked on delivery.
    pub doc: u64,
    pub dirty: Option<Rect>,
}

impl gpui::EventEmitter<CanvasHistoryEvent> for OverlayView {}
impl gpui::EventEmitter<CanvasEdit> for OverlayView {}
/// Opaque history event emitted by [`SheetCanvas`] and [`OverlayView`] (contract C2).
///
/// A raw GPUI subscription to this event yields nothing usable: it has no public fields, no
/// accessors, and its [`std::fmt::Debug`] output omits the payload. Outside `omnia-canvas`,
/// [`SheetCanvas::subscribe_history`] is the only way to receive a [`CanvasHistory`], and it
/// delivers an event only when the canvas still shows that event's document at delivery time.
///
/// ```compile_fail
/// use omnia_canvas::tool::CanvasHistoryEvent;
/// fn read(event: &CanvasHistoryEvent) -> u64 {
///     event.0.doc
/// }
/// ```
///
/// ```compile_fail
/// use omnia_canvas::tool::CanvasHistoryEvent;
/// fn read(event: &CanvasHistoryEvent) -> bool {
///     event.can_undo
/// }
/// ```
pub struct CanvasHistoryEvent(pub(crate) CanvasHistory);

impl std::fmt::Debug for CanvasHistoryEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CanvasHistoryEvent(..)")
    }
}

impl gpui::EventEmitter<CanvasHistoryEvent> for SheetCanvas {}

/// The identity a [`DocStamp`] uses for a document entity.
pub fn doc_key(doc: &Entity<Doc>) -> u64 {
    doc.entity_id().as_u64()
}

/// The gesture owner's surface for the tool machine (`OverlayView`).
pub(crate) trait ToolHost: Sized + 'static {
    fn tools(&self) -> &ToolMachine;
    fn tools_mut(&mut self) -> &mut ToolMachine;
    fn tool_doc(&self) -> Entity<Doc>;
    /// Repaint the preview. Never notifies `SheetView` (INV11).
    fn preview_changed(&mut self, cx: &mut Context<Self>);
    /// The document changed through this host (commit, undo or redo): repaint, prune, and
    /// emit [`CanvasHistoryEvent`] and [`CanvasEdit`].
    fn doc_changed(&mut self, change: &Change, cx: &mut Context<Self>);
    /// Carry out a request the machine cannot finish alone. Default: not supported yet
    /// (T5.3 adds placement, reorienting and deleting the selection).
    fn request(&mut self, _request: ToolRequest, _cx: &mut Context<Self>) {}
}

/// Carries out one [`ToolOutcome`]: applies its commit as one `Doc::apply` (INV5), repaints
/// the preview, and passes on its request. Capture release is owned exclusively by
/// [`OverlayView::end_gesture`] (contract C4).
pub(crate) fn finish<H: ToolHost>(
    host: &mut H,
    mut outcome: ToolOutcome,
    cx: &mut Context<H>,
) -> ToolOutcome {
    if let Some(commit) = outcome.commit.take() {
        apply(host, commit, cx);
    }
    if outcome.preview_changed {
        host.preview_changed(cx);
    }
    if let Some(request) = outcome.request.take() {
        host.request(request, cx);
    }
    outcome
}

/// Applies a commit as one document step, only to the document and revision it was planned
/// against: a commit from a stale gesture is dropped with no edit. A refused command changes
/// nothing (INV4), and a validated no-op keeps the revision, so the host hears only of
/// effective edits.
pub(crate) fn apply<H: ToolHost>(host: &mut H, commit: Commit, cx: &mut Context<H>) -> bool {
    let doc = host.tool_doc();
    if doc_key(&doc) != commit.base.key {
        return false;
    }
    let change = doc.update(cx, |doc, _| {
        if doc.revision() != commit.base.revision {
            return None;
        }
        let before = doc.revision();
        doc.apply(commit.command)
            .ok()
            .filter(|_| doc.revision() != before)
    });
    match change {
        Some(change) => {
            host.doc_changed(&change, cx);
            true
        }
        None => false,
    }
}

fn run<H: ToolHost>(
    host: &mut H,
    step: impl FnOnce(&mut ToolMachine) -> ToolOutcome,
    cx: &mut Context<H>,
) {
    let outcome = step(host.tools_mut());
    finish(host, outcome, cx);
}

/// What the `Sheet` key context's actions do on a view (`OverlayView`, `SheetCanvas`).
pub(crate) trait SheetActions: Sized + 'static {
    fn action_reorient(&mut self, by: Orient, window: &mut Window, cx: &mut Context<Self>);
    fn action_place_wire(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn action_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
    fn action_delete(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn action_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
    fn action_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
}

/// Puts `element` (the view's focus-tracking root) in the `Sheet` key context and routes
/// every [`crate::keymap`] action to the view (UD3). The shell binds R, X, Y and W to these
/// actions from its own table; a standalone canvas binds `standalone_bindings()`.
pub(crate) fn sheet_key_context<V: SheetActions>(element: Div, cx: &mut Context<V>) -> Div {
    element
        .key_context(SHEET_CONTEXT)
        .on_action(cx.listener(|v: &mut V, _: &Rotate, window, cx| {
            v.action_reorient(Orient::ROT_90, window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &MirrorHorizontal, window, cx| {
            v.action_reorient(Orient::FLIP_X, window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &MirrorVertical, window, cx| {
            v.action_reorient(Orient::FLIP_Y, window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &PlaceWire, window, cx| {
            v.action_place_wire(window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &Cancel, window, cx| {
            v.action_cancel(window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &Delete, window, cx| {
            v.action_delete(window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &Undo, window, cx| {
            v.action_undo(window, cx);
        }))
        .on_action(cx.listener(|v: &mut V, _: &Redo, window, cx| {
            v.action_redo(window, cx);
        }))
}

impl SheetActions for OverlayView {
    fn action_reorient(&mut self, by: Orient, _window: &mut Window, cx: &mut Context<Self>) {
        run(self, |m| m.reorient(by), cx);
    }

    fn action_place_wire(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_gesture(
            GestureEndReason::ToolChange(Tool::Wire),
            Some(window),
            Some(cx),
        );
    }

    fn action_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.end_gesture(GestureEndReason::Cancel, Some(window), Some(cx))
    }

    fn action_delete(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_rubber_banding() {
            return;
        }
        run(self, ToolMachine::delete, cx);
    }

    fn action_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(false, window, cx)
    }

    fn action_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(true, window, cx)
    }
}

impl OverlayView {
    fn history_step(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        // A gesture's preview refers to the current document: drop it before the step (C4).
        self.end_gesture(GestureEndReason::HistoryStep, Some(window), Some(cx));
        let doc = self.tool_doc();
        let change = doc.update(cx, |doc, _| if redo { doc.redo() } else { doc.undo() });
        match change {
            Some(change) => {
                self.doc_changed(&change, cx);
                true
            }
            None => false,
        }
    }

    pub fn tool(&self) -> Tool {
        self.tools().tool()
    }

    pub fn tool_state(&self) -> &ToolState {
        self.tools().state()
    }

    /// Switches tools, cancelling any gesture under way, the rubber band included (INV5, C4).
    /// Releases the pointer capture via [`OverlayView::end_gesture`].
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        self.end_gesture(GestureEndReason::ToolChange(tool), None, Some(cx));
    }

    /// Esc: cancels the tool's gesture, or else a rubber band. Never edits the document (C4).
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.end_gesture(GestureEndReason::Cancel, Some(window), Some(cx))
    }

    /// Undoes one document step (one committed gesture, INV5); emits [`CanvasHistoryEvent`].
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(false, window, cx)
    }

    /// Redoes one document step; emits [`CanvasHistoryEvent`].
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(true, window, cx)
    }

    /// Deletes the current selection outside a gesture (`ui-plan.md` section 14, T5.3).
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.action_delete(window, cx);
    }
}

/// `SheetCanvas` delegates every tool call to the attached gesture owner, and only while the
/// owner shows the canvas's document (`SheetCanvas::gesture_owner`). Without an owner there
/// are no tools: undo and redo act on the canvas's own document and emit [`CanvasHistory`].
impl SheetActions for SheetCanvas {
    fn action_reorient(&mut self, by: Orient, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.gesture_owner(cx) {
            overlay.update(cx, |o, cx| o.action_reorient(by, window, cx));
        }
    }

    fn action_place_wire(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.gesture_owner(cx) {
            overlay.update(cx, |o, cx| o.action_place_wire(window, cx));
        }
    }

    fn action_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.gesture_owner(cx)
            .is_some_and(|overlay| overlay.update(cx, |o, cx| o.cancel(window, cx)))
    }

    fn action_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(overlay) = self.gesture_owner(cx) {
            overlay.update(cx, |o, cx| o.action_delete(window, cx));
        }
    }

    fn action_undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(false, window, cx)
    }

    fn action_redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.history_step(true, window, cx)
    }
}

impl SheetCanvas {
    fn history_step(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.overlay().is_some() {
            // Delegated edits go only to an owner showing this canvas's document; its
            // `CanvasHistory` is re-emitted by `set_overlay`'s subscription.
            let Some(overlay) = self.gesture_owner(cx) else {
                return false;
            };
            return overlay.update(cx, |o, cx| {
                if redo {
                    o.redo(window, cx)
                } else {
                    o.undo(window, cx)
                }
            });
        }
        let Some(doc) = self.doc().cloned() else {
            return false;
        };
        let change = doc.update(cx, |doc, _| if redo { doc.redo() } else { doc.undo() });
        let Some(change) = change else {
            return false;
        };
        self.repaint_if_visible(change.dirty, cx);
        cx.emit(CanvasHistoryEvent(CanvasHistory::of(
            doc_key(&doc),
            doc.read(cx),
        )));
        true
    }

    /// Whether `history` concerns the document this canvas shows now.
    pub fn is_current_history(&self, history: &CanvasHistory) -> bool {
        self.doc().map(doc_key) == Some(history.doc)
    }

    /// Subscribes to this canvas's history (UD6, contract C2). GPUI delivers events after the
    /// update that emitted them unwinds, so the document may have changed since: an event for any
    /// document but the one the canvas shows at delivery is dropped here, before `on_history`.
    ///
    /// Reads the canvas's current document key from a shared cell rather than re-borrowing the
    /// canvas entity, so even a self-subscription (outside C3) never triggers a GPUI double-borrow
    /// panic.
    pub fn subscribe_history<T: 'static>(
        canvas: &Entity<Self>,
        cx: &mut Context<T>,
        mut on_history: impl FnMut(&mut T, &CanvasHistory, &mut Context<T>) + 'static,
    ) -> gpui::Subscription {
        let current_doc = Self::current_doc_key_for(canvas.entity_id(), cx);
        cx.subscribe(
            canvas,
            move |this, _canvas, event: &CanvasHistoryEvent, cx| {
                let matches = current_doc
                    .as_ref()
                    .and_then(Weak::upgrade)
                    .is_some_and(|key| key.get() == Some(event.0.doc));
                if matches {
                    on_history(this, &event.0, cx);
                }
            },
        )
    }

    /// The active tool of the gesture owner; `Tool::Select` without one.
    pub fn tool(&self, cx: &App) -> Tool {
        self.gesture_owner(cx)
            .map_or(Tool::Select, |o| o.read(cx).tool())
    }

    /// The gesture owner's state; `ToolState::Idle` without one.
    pub fn tool_state(&self, cx: &App) -> ToolState {
        self.gesture_owner(cx)
            .map_or(ToolState::Idle, |o| o.read(cx).tool_state().clone())
    }

    /// Switches the gesture owner's tool. Without an owner there are no tools.
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if let Some(overlay) = self.gesture_owner(cx) {
            overlay.update(cx, |o, cx| o.set_tool(tool, cx));
        }
    }

    /// Esc, called by the shell's `NavCancel` while the sheet is the key target (UQ14).
    /// Returns whether a gesture or tool was cancelled. Never edits the document.
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.action_cancel(window, cx)
    }

    /// Undoes one document step through the gesture owner and emits [`CanvasHistory`]
    /// (UD3: the shell's Ctrl+Z).
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.action_undo(window, cx);
    }

    /// Redoes one document step and emits [`CanvasHistory`] (UD3: the shell's Ctrl+Y).
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.action_redo(window, cx);
    }

    /// Deletes the current selection through the gesture owner (`ui-plan.md` section 14, T5.3).
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.action_delete(window, cx);
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
    use omnia_doc::{Connectivity, GRID_NM, Label, Part, PartId, SHEET_LIMIT_NM, Wire, WireId};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;
    use std::{cell::RefCell, rc::Rc};

    const G: i32 = GRID_NM;
    const R1: ItemId = ItemId::Part(PartId(0));
    const C1: ItemId = ItemId::Part(PartId(1));
    /// A hit radius well inside one grid step.
    const TOL: i32 = G / 4;
    /// The document key the pure tests stamp gestures with.
    const KEY: u64 = 7;

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

    fn press(m: &mut ToolMachine, doc: &Doc, at: Pt, selection: &[ItemId]) -> ToolOutcome {
        m.pointer_down(doc, KEY, at, TOL, selection, false)
    }

    fn command(out: ToolOutcome) -> Option<Command> {
        out.commit.map(|c| c.command)
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
                pressed: false,
                raw_cursor: None,
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
        let out = press(&mut m, &doc, pt(-6, 0), &[R1, C1]);
        assert!(out.handled && out.capture && out.commit.is_none());
        let drag = dragging(&m);
        assert_eq!(drag.items, vec![R1, C1]);
        assert_eq!(drag.pressed, R1);
        assert_eq!(
            drag.base,
            DocStamp {
                key: KEY,
                revision: doc.revision(),
            }
        );

        // Not selected: the drag takes the pressed item alone.
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[C1]);
        assert_eq!(dragging(&m).items, vec![R1]);

        // Shift, empty sheet and other tools are left to selection.
        let mut m = ToolMachine::new();
        assert!(!m.pointer_down(&doc, KEY, pt(-6, 0), TOL, &[], true).handled);
        assert!(!press(&mut m, &doc, pt(0, -8), &[]).handled);
        m.set_tool(Tool::Label);
        assert!(!press(&mut m, &doc, pt(-6, 0), &[]).handled);
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[test]
    fn t5_2_drag_delta_snaps_to_the_grid() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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
        assert!(out.release && out.request.is_none());
        assert_eq!(
            out.commit,
            Some(Commit {
                command: Command::Move {
                    items: vec![R1],
                    delta: pt(2, -1),
                    follow: WiringPrefs::default().follow_policy(false),
                },
                base: DocStamp {
                    key: KEY,
                    revision: doc.revision(),
                },
            })
        );
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[test]
    fn t5_2_release_reads_alt_for_the_follow_policy() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-5, 0), true);
        let Some(Command::Move { follow, .. }) = command(m.pointer_up(pt(-5, 0), true)) else {
            panic!("expected a move");
        };
        assert_eq!(follow, WiringPrefs::default().follow_policy(true));
    }

    #[test]
    fn t5_2_a_click_without_movement_commits_nothing() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1, C1]);
        let out = m.pointer_up(pt(-6, 0), false);
        assert_eq!(out.commit, None);
        assert_eq!(
            out.request,
            Some(ToolRequest::Click {
                item: R1,
                base: DocStamp {
                    key: KEY,
                    revision: doc.revision(),
                },
            })
        );
        assert!(out.release);
    }

    #[test]
    fn t5_2_a_lost_release_cancels_the_drag() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-3, 0), true);
        let out = m.pointer_move(pt(-2, 0), false);
        assert!(out.release && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);
        // The release that follows is not the tool's.
        assert_eq!(m.pointer_up(pt(-2, 0), false), ToolOutcome::default());
    }

    /// Review finding 2: a lost release followed directly by a press must not let that
    /// press's release commit the old drag.
    #[test]
    fn t5_2_a_fresh_press_ends_a_stale_drag_with_no_edit() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-3, 0), true);
        // The press lands on C1 with no unpressed move in between.
        let out = press(&mut m, &doc, pt(6, 0), &[R1]);
        assert!(out.release && out.capture && out.handled && out.commit.is_none());
        let drag = dragging(&m);
        assert_eq!((drag.items.as_slice(), drag.pressed), (&[C1][..], C1));
        // Its release moves C1 only, by its own movement.
        m.pointer_move(pt(7, 1), true);
        let Some(Command::Move { items, delta, .. }) = command(m.pointer_up(pt(7, 1), false))
        else {
            panic!("expected a move");
        };
        assert_eq!((items, delta), (vec![C1], pt(1, 1)));

        // A fresh press on empty sheet ends the stale drag and goes to selection.
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-3, 0), true);
        let out = press(&mut m, &doc, pt(0, -8), &[R1]);
        assert!(out.release && !out.handled && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[test]
    fn t5_2_a_document_swap_or_lost_pointer_drops_gestures() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-3, 0), true);
        let out = m.document_replaced();
        assert!(out.release && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);
        assert_eq!(m.pointer_up(pt(-3, 0), false).commit, None);

        press(&mut m, &doc, pt(-6, 0), &[R1]);
        let out = m.pointer_lost();
        assert!(out.release && out.commit.is_none());
        assert_eq!(m.phase(), ToolPhase::Idle);

        // A pending placement press is dropped too; the tool stays.
        m.set_tool(Tool::Place(symbol()));
        press(&mut m, &doc, pt(1, 1), &[]);
        m.pointer_lost();
        assert_eq!(m.pointer_up(pt(1, 1), false).request, None);
        assert_eq!(m.tool(), Tool::Place(symbol()));

        // A swap drops the wire start; the tool stays.
        m.set_tool(Tool::Wire);
        press(&mut m, &doc, pt(1, 1), &[]);
        m.document_replaced();
        assert_eq!(m.state(), &ToolState::Wiring(WireGesture::default()));
    }

    /// T5.3 / T5.5 (PR 125 review case): R, X and Y during a drag compose into one
    /// `Command::Transform`, planned and normalised once against the original sheet so a valid
    /// final position commits and one undo restores it completely (INV4, INV5).
    ///
    /// Uses a two-item selection (`[Wire(1), Label(0)]` sharing the same `(2,2)-(4,2)` bounds and
    /// `(3,2)` pivot) because W3.3 routes a single selected wire segment to `DraggingSegment`.
    #[test]
    fn t5_3_r_x_y_during_a_drag_commit_one_transform() {
        let mut doc = Doc::from_items(
            [],
            [
                Wire {
                    a: pt(0, 0),
                    b: pt(2, 0),
                },
                Wire {
                    a: pt(2, 2),
                    b: pt(4, 2),
                },
            ],
            [Label {
                at: pt(4, 2),
                text: "L1".into(),
            }],
        )
        .unwrap();
        let original = snapshot(&doc);
        let selected = [
            ItemId::Wire(WireId(1)),
            ItemId::Label(omnia_doc::LabelId(0)),
        ];
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(3, 2), &selected);
        assert_eq!(m.reorient(Orient::IDENTITY), ToolOutcome::default());
        assert!(!dragging(&m).is_lifted());
        m.pointer_move(pt(3, 0), true);
        let expected_orient = Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y);
        for by in [Orient::ROT_90, Orient::FLIP_X, Orient::FLIP_Y] {
            assert!(m.reorient(by).preview_changed);
        }
        let Some(Commit { command, .. }) = m.pointer_up(pt(3, 0), false).commit else {
            panic!("expected a commit");
        };
        assert_eq!(
            command,
            Command::Transform {
                items: selected.to_vec(),
                delta: pt(0, -2),
                pivot: pt(3, 2),
                by: expected_orient,
                follow: WiringPrefs::default().follow_policy(false),
            }
        );
        let rev = doc.revision();
        doc.apply(command)
            .expect("the transformed position is valid");
        assert_eq!(doc.revision(), rev + 1);
        assert_eq!(
            doc.wire(WireId(1)),
            Some(&Wire {
                a: pt(3, 1),
                b: pt(3, -1),
            })
        );
        assert!(doc.undo().is_some());
        assert!(doc.content_eq(&original), "INV4/INV5: one undo restores");
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
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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
        assert!(press(&mut m, &doc, pt(3, -1), &[]).handled);
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
        // One press places once.
        assert_eq!(m.pointer_up(pt(3, -1), false).request, None);
        // Placing continues until Esc, which returns to Select.
        assert_eq!(m.phase(), ToolPhase::Placing);
        assert!(m.cancel().handled);
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));
    }

    /// Review finding 4: a release left over from a gesture in another tool never places.
    #[test]
    fn t5_2_place_needs_a_press_in_the_place_tool() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        press(&mut m, &doc, pt(-6, 0), &[R1]);
        m.pointer_move(pt(-3, 0), true);
        let out = m.set_tool(Tool::Place(symbol()));
        assert!(out.release && out.commit.is_none());
        let out = m.pointer_up(pt(-3, 0), false);
        assert!(out.handled);
        assert_eq!((out.request, out.commit), (None, None));
        // A move with the button up forgets a press made before it, too.
        press(&mut m, &doc, pt(1, 1), &[]);
        m.pointer_move(pt(2, 1), false);
        assert_eq!(m.pointer_up(pt(2, 1), false).request, None);
    }

    #[test]
    fn t5_2_wiring_esc_drops_the_start_then_leaves_the_tool() {
        let doc = test_doc();
        let mut m = ToolMachine::new();
        m.set_tool(Tool::Wire);
        assert!(m.pointer_move(pt(1, 1), false).preview_changed);
        assert!(press(&mut m, &doc, pt(1, 1), &[]).preview_changed);
        assert!(press(&mut m, &doc, pt(5, 1), &[]).handled);
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
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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
        let commit = command(m.pointer_up(pt(-1, 3), false)).expect("an effective drag");
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
        press(&mut m, &doc, pt(-6, 0), &[R1]);
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

    fn down(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext, p: Pt) {
        let at = screen(overlay, cx, p);
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    }

    fn drag_to(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext, p: Pt) {
        let at = screen(overlay, cx, p);
        cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
    }

    fn up(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext, p: Pt) {
        let at = screen(overlay, cx, p);
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    }

    fn captured(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, _| window.captured_hitbox().is_some())
    }

    fn part_at(doc: &Entity<Doc>, cx: &mut VisualTestContext, id: u32) -> Pt {
        cx.update(|_, cx| doc.read(cx).part(PartId(id)).unwrap().at)
    }

    /// INV5 through simulated GPUI input: the revision holds through a drag until mouse-up,
    /// Ctrl+Z reverts the whole drag in one step, and Esc mid-drag changes nothing.
    #[gpui::test]
    fn inv5_simulated_drag_commits_once_and_esc_cancels(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));
        let rev0 = revision(&doc, cx);

        down(&overlay, cx, pt(-6, 0));
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
        up(&overlay, cx, pt(-2, 3));
        assert_eq!(
            revision(&doc, cx),
            rev0 + 1,
            "INV5: the drag commits once, on mouse-up"
        );
        assert_eq!(part_at(&doc, cx, 0), pt(-2, 3));
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx));
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
        down(&overlay, cx, pt(-2, 3));
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        drag_to(&overlay, cx, pt(4, -5));
        cx.simulate_keystrokes("escape");
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx), "Esc releases the drag's capture");
        up(&overlay, cx, pt(4, -5));
        assert_eq!(
            revision(&doc, cx),
            rev,
            "INV5: Esc leaves the revision unchanged"
        );
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&moved)));
    }

    #[gpui::test]
    fn t5_2_a_drag_preview_does_not_repaint_the_sheet(cx: &mut TestAppContext) {
        // D7 / INV11 / T5.3: SheetView paints once at drag start (to hide dragged items), 0 times
        // during drag moves, once at drag end/cancel (to restore hidden items), and 0 on stray
        // release — exactly 2 paints per drag.
        let (overlay, _doc, cx) = open_overlay(cx);
        let paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let press = screen(&overlay, cx, pt(-6, 0));
        cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints,
            "INV11: press before drag lift-off does not paint SheetView"
        );
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
            paints + 1,
            "D7/INV11: drag lift-off paints SheetView once to hide, 0 times across further moves"
        );
        // Cancel through the shell's path (`NavCancel` calls `cancel`, UQ14). A keystroke here
        // would also bring GPUI's input-modality refresh (UG17); that case is counted apart in
        // `t5_2_esc_mid_drag_paints_only_for_the_modality_switch`.
        assert!(cx.update(|window, cx| overlay.update(cx, |o, cx| o.cancel(window, cx))));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints + 2,
            "D7: drag cancel paints SheetView once to restore hidden items"
        );
        cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints + 2,
            "stray release after cancel does not paint SheetView"
        );
    }

    /// INV11 with UQ15 for a real Esc key: drag start hides items (1 canvas notify), drag moves
    /// trigger 0 sheet notifies, Esc unhides items (1 canvas notify, coalescing with GPUI's UG17
    /// modality switch), and the subsequent release triggers 0 paints.
    #[gpui::test]
    fn t5_2_esc_mid_drag_paints_only_for_the_modality_switch(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let notifies = Rc::new(std::cell::Cell::new(0u32));
        let counter = notifies.clone();
        cx.update(|_, cx| {
            let sheet = overlay.read(cx).sheet_entity().clone();
            cx.observe(&sheet, move |_, _| counter.set(counter.get() + 1))
                .detach();
        });
        let paints =
            |cx: &mut VisualTestContext| cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let keyboard =
            |cx: &mut VisualTestContext| cx.update(|window, _| window.last_input_was_keyboard());
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        assert_eq!(notifies.get(), 0, "0 sheet notifies before drag lift-off");
        drag_to(&overlay, cx, pt(-3, 0));
        assert_eq!(notifies.get(), 1, "1 sheet notify at drag lift-off (hide)");
        let after_lift = paints(cx);
        drag_to(&overlay, cx, pt(-1, 0));
        assert_eq!(
            notifies.get(),
            1,
            "0 sheet notifies during subsequent drag moves"
        );
        assert_eq!(
            paints(cx),
            after_lift,
            "0 sheet paints during subsequent drag moves"
        );
        let before = paints(cx);
        let pointer_modality = keyboard(cx);
        assert!(
            !pointer_modality,
            "the drag leaves the window in pointer modality"
        );

        // The Esc: 1 canvas notify to unhide + at most 1 modality-switch refresh (UQ15).
        cx.simulate_keystrokes("escape");
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        let modality_switches = u64::from(keyboard(cx) != pointer_modality);
        assert_eq!(modality_switches, 1, "the Esc switches to keyboard input");
        let after_esc = paints(cx);
        let esc_paints = after_esc - before;
        assert!(
            esc_paints >= 1 && esc_paints <= 1 + modality_switches,
            "D7/UQ15: {esc_paints} sheet paints for unhide + {modality_switches} modality switches"
        );
        assert_eq!(notifies.get(), 2, "1 sheet notify at drag end (unhide)");

        // The release: a mouse-up keeps keyboard modality at the pinned GPUI, so it may not
        // paint the sheet at all.
        up(&overlay, cx, pt(-3, 0));
        assert!(keyboard(cx), "a mouse-up does not switch modality");
        assert_eq!(
            paints(cx),
            after_esc,
            "the release paints the sheet 0 times"
        );
        assert_eq!(notifies.get(), 2, "only start and end notified SheetView");
        assert_eq!(revision(&doc, cx), rev0);
    }

    /// Review 2, finding 2: GPUI clears the capture on any mouse-up, so a second button
    /// ends the drag; its later left release commits nothing, and no repaint reclaims it.
    #[gpui::test]
    fn t5_2_a_drag_that_loses_its_capture_commits_nothing(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 1));
        assert!(captured(cx));
        let at = screen(&overlay, cx, pt(-3, 1));
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        assert!(!captured(cx));
        // A repaint before the next pointer event does not take the capture back.
        cx.update(|_, cx| overlay.update(cx, |_, cx| cx.notify()));
        cx.run_until_parked();
        assert!(!captured(cx), "a lost capture is not reclaimed");
        up(&overlay, cx, pt(-3, 1));
        assert_eq!(revision(&doc, cx), rev0, "the stale drag commits nothing");
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);

        // Same with a left move in between: the move ends the drag.
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 1));
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        drag_to(&overlay, cx, pt(-2, 2));
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        up(&overlay, cx, pt(-2, 2));
        assert_eq!(revision(&doc, cx), rev0);
        assert_eq!(part_at(&doc, cx, 0), pt(-6, 0));
    }

    /// Review 2, finding 3: a band that never moved still releases its capture when a tool
    /// switch or Esc cancels it.
    #[gpui::test]
    fn t5_2_an_unmoved_band_releases_its_capture_when_cancelled(cx: &mut TestAppContext) {
        let (overlay, _doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(0, -8));
        assert!(captured(cx));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_tool(Tool::Select, cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert!(
            !captured(cx),
            "set_tool releases the unmoved band's capture"
        );
        up(&overlay, cx, pt(0, -8));

        // With the window at hand (Esc), the release is immediate.
        down(&overlay, cx, pt(0, -8));
        assert!(captured(cx));
        let released = cx.update(|window, cx| {
            let cancelled = overlay.update(cx, |o, cx| o.cancel(window, cx));
            (cancelled, window.captured_hitbox().is_none())
        });
        assert_eq!(released, (true, true));
        up(&overlay, cx, pt(0, -8));
    }

    /// Review finding 1: a drag never commits against a replacement document.
    #[gpui::test]
    fn t5_2_a_drag_does_not_survive_a_document_swap(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let (a0, b0) = cx.update(|_, cx| (snapshot(doc_a.read(cx)), snapshot(doc_b.read(cx))));
        down(&overlay, cx, pt(0, 0));
        let ToolState::DraggingSegment(seg) =
            cx.update(|_, cx| overlay.read(cx).tool_state().clone())
        else {
            panic!("expected a segment drag of wire 0");
        };
        assert_eq!(seg.wire, WireId(0));
        drag_to(&overlay, cx, pt(0, 2));
        assert!(captured(cx));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_document(doc_b.clone(), cx)));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx), "the swap releases the drag's capture");
        up(&overlay, cx, pt(0, 2));
        assert!(
            cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)),
            "doc B untouched"
        );
        assert_eq!(revision(&doc_b, cx), b0.revision());
        assert!(
            cx.update(|_, cx| doc_a.read(cx).content_eq(&a0)),
            "doc A untouched"
        );
    }

    /// Review finding 1: a part drag (`ToolState::Dragging`) never survives a document swap.
    #[gpui::test]
    fn t5_2_a_part_drag_does_not_survive_a_document_swap(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let (a0, b0) = cx.update(|_, cx| (snapshot(doc_a.read(cx)), snapshot(doc_b.read(cx))));
        down(&overlay, cx, pt(-6, 0));
        assert!(matches!(
            cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Dragging(ref drag) if drag.pressed == R1
        ));
        drag_to(&overlay, cx, pt(-4, 2));
        assert!(captured(cx));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_document(doc_b.clone(), cx)));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx), "the swap releases the drag's capture");
        up(&overlay, cx, pt(-4, 2));
        assert!(
            cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)),
            "doc B untouched"
        );
        assert_eq!(revision(&doc_b, cx), b0.revision());
        assert!(
            cx.update(|_, cx| doc_a.read(cx).content_eq(&a0)),
            "doc A untouched"
        );
    }

    /// Review finding 1: an edit made elsewhere during a drag makes its commit stale.
    #[gpui::test]
    fn t5_2_a_drag_does_not_commit_over_another_edit(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::Move {
                    items: vec![C1],
                    delta: pt(0, 1),
                    follow: WiringPrefs::default().follow_policy(false),
                })
                .unwrap();
            })
        });
        let rev = revision(&doc, cx);
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(revision(&doc, cx), rev, "the stale drag commits nothing");
        assert_eq!(part_at(&doc, cx, 0), pt(-6, 0));
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
    }

    /// Review finding 2: a lost mouse-up followed by a press commits only the new gesture.
    #[gpui::test]
    fn t5_2_a_fresh_press_after_a_lost_release_drops_the_old_drag(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 0));
        // The release never arrives; the next event is a press on C1.
        down(&overlay, cx, pt(6, 0));
        assert_eq!(revision(&doc, cx), rev0);
        let ToolState::Dragging(drag) = cx.update(|_, cx| overlay.read(cx).tool_state().clone())
        else {
            panic!("the fresh press starts its own drag");
        };
        assert_eq!(drag.pressed, C1);
        drag_to(&overlay, cx, pt(8, 2));
        up(&overlay, cx, pt(8, 2));
        assert_eq!(revision(&doc, cx), rev0 + 1);
        assert_eq!(
            part_at(&doc, cx, 0),
            pt(-6, 0),
            "the stale drag never commits"
        );
        assert_eq!(part_at(&doc, cx, 1), pt(8, 2));
    }

    /// Review finding 2: deactivating the window ends a drag with no edit.
    #[gpui::test]
    fn t5_2_window_deactivation_cancels_a_drag(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 0));
        cx.deactivate_window();
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        up(&overlay, cx, pt(-3, 0));
        assert_eq!(revision(&doc, cx), rev0);
    }

    /// Review finding 4: a tool switch cancels the band and releases its capture; one made
    /// during an item drag releases the drag's capture without waiting for a mouse-up.
    #[gpui::test]
    fn t5_2_a_tool_switch_leaves_no_pointer_state(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(3, -6));
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_some()));
        assert!(captured(cx));
        cx.simulate_keystrokes("w");
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Wire);
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert!(!captured(cx), "the band's capture is released");
        up(&overlay, cx, pt(3, -6));
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        cx.simulate_keystrokes("escape escape");
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Select);

        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 0));
        assert!(captured(cx));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_tool(Tool::Place(symbol()), cx)));
        cx.run_until_parked();
        assert!(!captured(cx), "the drag's capture is released at once");
        up(&overlay, cx, pt(-3, 0));
        assert_eq!(revision(&doc, cx), rev0);
        assert_eq!(phase(&overlay, cx), ToolPhase::Placing);
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

        // R, X and Y apply to the drag preview (T5.3/T5.5); Del is ignored mid-drag; release
        // commits one `Command::Transform` reverted by a single Ctrl+Z (INV5).
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 1));
        cx.simulate_keystrokes("r x y delete");
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert_eq!(revision(&doc, cx), rev0);
        up(&overlay, cx, pt(-4, 1));
        assert_eq!(revision(&doc, cx), rev0 + 1);
        assert_eq!(part_at(&doc, cx, 0), pt(-4, 1));
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y)
        );
        cx.simulate_keystrokes("ctrl-z");
        assert_eq!(part_at(&doc, cx, 0), pt(-6, 0));
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::IDENTITY
        );

        // With no tool gesture, Esc cancels a rubber band instead.
        down(&overlay, cx, pt(0, -8));
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

    fn record_history(
        canvas: &Entity<SheetCanvas>,
        cx: &mut VisualTestContext,
    ) -> Rc<RefCell<Vec<CanvasHistory>>> {
        let history = Rc::new(RefCell::new(Vec::new()));
        let sink = history.clone();
        let canvas = canvas.clone();
        // A subscriber as the shell would be: through the canvas's checked helper.
        let recorder = cx.update(|_, cx| {
            cx.new(|cx| Recorder {
                _subscription: SheetCanvas::subscribe_history(&canvas, cx, move |_, event, _| {
                    sink.borrow_mut().push(*event)
                }),
            })
        });
        // The test keeps the recorder (and its subscription) for the window's lifetime.
        std::mem::forget(recorder);
        history
    }

    /// The subscriber entity `record_history` keeps alive; it holds the subscription.
    struct Recorder {
        _subscription: gpui::Subscription,
    }

    /// Review finding 5: one gesture owner. `SheetCanvas` delegates to the attached overlay
    /// and reports history for a drag's commit as well as for undo and redo.
    #[gpui::test]
    fn t5_2_sheet_canvas_delegates_to_the_gesture_owner(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let canvas = cx.update(|_, cx| {
            let doc = doc.clone();
            let overlay = overlay.clone();
            cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                let mut canvas = SheetCanvas::new(camera, &GRAPHITE, cx).with_doc(doc);
                canvas.set_overlay(Some(overlay), cx);
                canvas
            })
        });
        let history = record_history(&canvas, cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));

        // A drag on the overlay reaches the shell as history.
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        up(&overlay, cx, pt(-4, 2));
        let moved = cx.update(|_, cx| doc.read(cx).history_cursor());
        assert_eq!(
            history.borrow().as_slice(),
            &[CanvasHistory {
                doc: doc_key(&doc),
                can_undo: true,
                can_redo: false,
                cursor: moved,
            }]
        );

        // Undo through the canvas acts on the owner's document and reports again.
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert_eq!(history.borrow().len(), 2);
        assert_eq!(
            (history.borrow()[1].can_undo, history.borrow()[1].can_redo),
            (false, true)
        );

        // Tool and cancel reach the overlay's machine, mid-drag included.
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_tool(Tool::Wire, cx)));
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Wire);
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool(cx)), Tool::Wire);
        assert!(cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool(cx)), Tool::Select);
        let rev = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-2, 0));
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).tool_state(cx).phase()),
            ToolPhase::Dragging
        );
        assert!(cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
        up(&overlay, cx, pt(-2, 0));
        assert_eq!(
            revision(&doc, cx),
            rev,
            "the shell's Esc cancels the owner's drag"
        );
        assert!(!cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
    }

    #[gpui::test]
    fn t5_2_sheet_canvas_without_an_overlay_steps_its_own_history(cx: &mut TestAppContext) {
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
        let history = record_history(&canvas, cx);
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
                doc: doc_key(&doc),
                can_undo: true,
                can_redo: false,
                cursor: moved_cursor,
            }
        );
        // No gesture owner, no tools.
        cx.simulate_keystrokes("w");
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool(cx)), Tool::Select);
        assert!(!cx.update(|window, cx| canvas.update(cx, |c, cx| c.cancel(window, cx))));
    }

    fn canvas_for(
        cx: &mut VisualTestContext,
        doc: Option<Entity<Doc>>,
        overlay: Option<Entity<OverlayView>>,
    ) -> Entity<SheetCanvas> {
        cx.update(|_, cx| {
            cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                let mut canvas = SheetCanvas::new(camera, &GRAPHITE, cx);
                if let Some(doc) = doc {
                    canvas = canvas.with_doc(doc);
                }
                canvas.set_overlay(overlay, cx);
                canvas
            })
        })
    }

    fn nudge(doc: &Entity<Doc>, cx: &mut VisualTestContext) {
        cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::Move {
                    items: vec![C1],
                    delta: pt(0, 1),
                    follow: WiringPrefs::default().follow_policy(false),
                })
                .unwrap();
            })
        });
    }

    fn overlay_doc(overlay: &Entity<OverlayView>, cx: &mut VisualTestContext) -> Entity<Doc> {
        cx.update(|_, cx| overlay.read(cx).doc().clone())
    }

    /// Review 2, finding 1, first order: attach, then switch the canvas's document. The owner
    /// follows in the same call, a pending drag on the old document dies, and undo and
    /// history then concern the new document only.
    #[gpui::test]
    fn t5_2_switching_the_canvas_document_switches_the_owner(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        let a0 = cx.update(|_, cx| snapshot(doc_a.read(cx)));
        nudge(&doc_a, cx);
        let a1 = cx.update(|_, cx| snapshot(doc_a.read(cx)));
        let b0 = cx.update(|_, cx| snapshot(doc_b.read(cx)));

        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_doc(Some(doc_b.clone()), cx)));
        cx.run_until_parked();
        assert_eq!(overlay_doc(&overlay, cx), doc_b);
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx));
        up(&overlay, cx, pt(-4, 2));
        assert!(cx.update(|_, cx| doc_a.read(cx).content_eq(&a1)));
        assert!(cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)));

        // Undo has nothing to do on B and must not reach A.
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        assert!(cx.update(|_, cx| doc_a.read(cx).content_eq(&a1)));
        assert!(!cx.update(|_, cx| doc_a.read(cx).content_eq(&a0)));
        assert!(history.borrow().is_empty());

        // A drag now edits B, and the shell hears B's history.
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        up(&overlay, cx, pt(-4, 2));
        let b_cursor = cx.update(|_, cx| doc_b.read(cx).history_cursor());
        assert_eq!(part_at(&doc_b, cx, 0), pt(-4, 2));
        assert_eq!(history.borrow().last().map(|h| h.cursor), Some(b_cursor));
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        assert!(cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)));
        assert!(cx.update(|_, cx| doc_a.read(cx).content_eq(&a1)));

        // Clearing the canvas's document detaches the owner.
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_doc(None, cx)));
        assert!(cx.update(|_, cx| canvas.read(cx).overlay().is_none()));
    }

    /// Review 2, finding 1, second order: switch first, then attach. Attaching moves the
    /// overlay onto the canvas's document, so undo acts there.
    #[gpui::test]
    fn t5_2_attaching_an_owner_aligns_its_document(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        nudge(&doc_a, cx);
        let a1 = cx.update(|_, cx| snapshot(doc_a.read(cx)));
        let b0 = cx.update(|_, cx| snapshot(doc_b.read(cx)));
        nudge(&doc_b, cx);
        let canvas = canvas_for(cx, Some(doc_a.clone()), None);
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_doc(Some(doc_b.clone()), cx)));
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_overlay(Some(overlay.clone()), cx)));
        assert_eq!(overlay_doc(&overlay, cx), doc_b);
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        assert!(cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)));
        assert!(cx.update(|_, cx| doc_a.read(cx).content_eq(&a1)));

        // A canvas without a document adopts the owner's.
        let canvas = canvas_for(cx, None, Some(overlay.clone()));
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).doc().cloned()),
            Some(doc_b)
        );
    }

    /// Review 2, finding 1: if the owner is moved to another document behind the canvas's
    /// back, delegated edits and forwarded history are refused.
    #[gpui::test]
    fn t5_2_a_mismatched_owner_gets_no_edits_and_sends_no_history(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_c = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        nudge(&doc_a, cx);
        nudge(&doc_c, cx);
        let (a1, c1) = cx.update(|_, cx| (snapshot(doc_a.read(cx)), snapshot(doc_c.read(cx))));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_document(doc_c.clone(), cx)));

        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        assert!(
            cx.update(|_, cx| doc_c.read(cx).content_eq(&c1)),
            "no delegated undo"
        );
        assert!(cx.update(|_, cx| doc_a.read(cx).content_eq(&a1)));
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_tool(Tool::Wire, cx)));
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Select);
        assert_eq!(cx.update(|_, cx| canvas.read(cx).tool(cx)), Tool::Select);

        // The owner's own edit on C is not reported as the canvas's history.
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(part_at(&doc_c, cx, 0), pt(-4, 2));
        assert!(history.borrow().is_empty());
    }

    /// Review 3, finding 1: an undo on A and a switch to B in one GPUI update. The queued
    /// events name A, so the canvas (now on B) drops them.
    #[gpui::test]
    fn t5_2_events_queued_before_a_switch_never_reach_the_new_document(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        let a0 = cx.update(|_, cx| snapshot(doc_a.read(cx)));
        nudge(&doc_a, cx);
        let b0 = cx.update(|_, cx| snapshot(doc_b.read(cx)));
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| {
                c.undo(window, cx);
                c.set_doc(Some(doc_b.clone()), cx);
            })
        });
        cx.run_until_parked();
        assert!(
            cx.update(|_, cx| doc_a.read(cx).content_eq(&a0)),
            "the undo ran on A"
        );
        assert!(cx.update(|_, cx| doc_b.read(cx).content_eq(&b0)));
        assert!(
            history.borrow().is_empty(),
            "A's queued history must not be delivered as B's"
        );
        // B's own edits are reported, with B's identity.
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(
            history.borrow().iter().map(|h| h.doc).collect::<Vec<_>>(),
            vec![doc_key(&doc_b)]
        );
    }

    /// Review 3, finding 2: detaching the owner, or replacing it, ends its drag with no edit
    /// and releases its capture, even while the old overlay stays mounted.
    #[gpui::test]
    fn t5_2_a_detached_or_replaced_owner_keeps_no_drag(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let canvas = canvas_for(cx, Some(doc.clone()), Some(overlay.clone()));
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_doc(None, cx)));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx), "detaching releases the drag's capture");
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(revision(&doc, cx), rev0);

        let canvas = canvas_for(cx, Some(doc.clone()), Some(overlay.clone()));
        let other = cx.update(|_, cx| {
            let doc = doc.clone();
            cx.new(|cx| {
                OverlayView::new(
                    doc,
                    Camera::new(Pt::ZERO, 1.0, Bounds::default()),
                    &GRAPHITE,
                    cx,
                )
            })
        });
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_overlay(Some(other.clone()), cx)));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        assert!(!captured(cx), "replacing releases the old owner's capture");
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(revision(&doc, cx), rev0);
        assert_eq!(part_at(&doc, cx, 0), pt(-6, 0));
    }

    /// Review 3, finding 3: after a second button clears the capture, a move outside the
    /// sheet still ends the drag, and its release commits nothing.
    #[gpui::test]
    fn t5_2_a_lost_capture_drag_ends_on_a_move_off_the_sheet(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 1));
        let at = screen(&overlay, cx, pt(-3, 1));
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        let outside = gpui::point(gpui::px(-400.0), gpui::px(-400.0));
        cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::none());
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        up(&overlay, cx, pt(-3, 1));
        assert_eq!(revision(&doc, cx), rev0);
    }

    /// Review 3, finding 4: the public `cancel_rubber_band` releases an unmoved band's capture.
    #[gpui::test]
    fn t5_2_cancel_rubber_band_releases_an_unmoved_band(cx: &mut TestAppContext) {
        let (overlay, _doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(0, -8));
        assert!(captured(cx));
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.cancel_rubber_band(cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert!(!captured(cx));
        up(&overlay, cx, pt(0, -8));
    }

    /// A host view that can drop its overlay while it is mounted.
    struct Host {
        overlay: Option<Entity<OverlayView>>,
    }

    impl gpui::Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            use gpui::{ParentElement, Styled};
            gpui::div().size_full().children(self.overlay.clone())
        }
    }

    /// Audit: an overlay dropped mid-drag releases its capture and edits nothing.
    #[gpui::test]
    fn t5_2_a_dropped_overlay_releases_its_capture(cx: &mut TestAppContext) {
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (host, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            let overlay = cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                OverlayView::new(doc_clone, camera, &GRAPHITE, cx)
            });
            Host {
                overlay: Some(overlay),
            }
        });
        cx.run_until_parked();
        let overlay = cx.update(|_, cx| host.read(cx).overlay.clone().unwrap());
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert!(captured(cx));
        let weak = overlay.downgrade();
        drop(overlay);
        cx.update(|_, cx| {
            host.update(cx, |h, cx| {
                h.overlay = None;
                cx.notify();
            })
        });
        cx.run_until_parked();
        assert!(weak.upgrade().is_none(), "the overlay is gone");
        assert!(!captured(cx), "its capture went with it");
        let at = gpui::point(gpui::px(100.0), gpui::px(100.0));
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        assert_eq!(revision(&doc, cx), rev0);
    }

    /// Audit: a drag that still owns its capture follows the pointer off the sheet and
    /// commits there, once, to its own document.
    #[gpui::test]
    fn t5_2_a_captured_drag_commits_when_released_off_the_sheet(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        let bounds = cx.update(|_, cx| overlay.read(cx).viewport());
        let outside = gpui::point(
            bounds.origin.x + bounds.size.width + gpui::px(40.0),
            bounds.origin.y + gpui::px(40.0),
        );
        cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::none());
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert!(captured(cx));
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        assert_eq!(
            revision(&doc, cx),
            rev0 + 1,
            "one commit, to its own document"
        );
        assert_ne!(part_at(&doc, cx, 0), pt(-6, 0));
        assert!(!captured(cx));
    }

    /// Review 4, finding 1, without an overlay: undo on A and a switch to B in one update.
    /// The subscriber hears nothing: at delivery the canvas shows B.
    #[gpui::test]
    fn t5_2_history_after_a_switch_in_the_same_update_is_dropped(cx: &mut TestAppContext) {
        let (_overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), None);
        let history = record_history(&canvas, cx);
        let a0 = cx.update(|_, cx| snapshot(doc_a.read(cx)));
        nudge(&doc_a, cx);
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| {
                c.undo(window, cx);
                c.set_doc(Some(doc_b.clone()), cx);
            })
        });
        cx.run_until_parked();
        assert!(
            cx.update(|_, cx| doc_a.read(cx).content_eq(&a0)),
            "the undo ran on A"
        );
        assert!(
            history.borrow().is_empty(),
            "A's history is dropped at delivery"
        );
        // The canvas's own document is still reported.
        nudge(&doc_b, cx);
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        assert_eq!(
            history.borrow().iter().map(|h| h.doc).collect::<Vec<_>>(),
            vec![doc_key(&doc_b)]
        );
    }

    /// Review 4, finding 1, with an overlay: undo, then a switch queued with `defer`. The
    /// forwarded event passed while A was current; the subscriber boundary still drops it.
    #[gpui::test]
    fn t5_2_history_before_a_deferred_switch_is_dropped(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        nudge(&doc_a, cx);
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| {
                c.undo(window, cx);
                let canvas = cx.entity();
                let doc_b = doc_b.clone();
                cx.defer(move |cx| canvas.update(cx, |c, cx| c.set_doc(Some(doc_b), cx)));
            })
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).doc().cloned()),
            Some(doc_b)
        );
        assert!(
            history.borrow().is_empty(),
            "A's history is dropped after the deferred switch"
        );
    }

    /// Review 4, finding 2: an overlay detached and unmounted in one update, with a handle
    /// still held, releases its capture without painting again.
    #[gpui::test]
    fn t5_2_a_detached_unmounted_overlay_releases_its_capture(cx: &mut TestAppContext) {
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (host, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            let overlay = cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                OverlayView::new(doc_clone, camera, &GRAPHITE, cx)
            });
            Host {
                overlay: Some(overlay),
            }
        });
        cx.run_until_parked();
        let overlay = cx.update(|_, cx| host.read(cx).overlay.clone().unwrap());
        let canvas = canvas_for(cx, Some(doc.clone()), Some(overlay.clone()));
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-4, 2));
        assert!(captured(cx));
        let paints = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
        cx.update(|_, cx| {
            canvas.update(cx, |c, cx| c.set_doc(None, cx));
            host.update(cx, |h, cx| {
                h.overlay = None;
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).overlay_paint_count()),
            paints,
            "the unmounted overlay never paints again"
        );
        assert!(!captured(cx), "the capture is released without a paint");
        assert_eq!(phase(&overlay, cx), ToolPhase::Idle);
        let at = gpui::point(gpui::px(100.0), gpui::px(100.0));
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        assert_eq!(revision(&doc, cx), rev0);
    }

    /// Review 5, finding A: a band whose release was lost ends, with no selection change,
    /// when a fresh press on an item starts a tool drag; a later release resolves nothing.
    #[gpui::test]
    fn t5_2_a_fresh_press_on_an_item_ends_a_stale_band(cx: &mut TestAppContext) {
        let (overlay, _doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(9, 8));
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_some()));
        // The band's release is lost; the next event is a press on R1.
        down(&overlay, cx, pt(-6, 0));
        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert!(
            cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()),
            "the stale band ends at the fresh press"
        );
        up(&overlay, cx, pt(-6, 0));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );
        // A later left release must not resolve the old band and replace the selection.
        up(&overlay, cx, pt(9, 8));
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );
        assert!(!captured(cx));
    }

    /// Review 5, finding B: a click whose item another command deleted mid-press selects
    /// nothing.
    #[gpui::test]
    fn t5_2_a_click_on_a_deleted_item_selects_nothing(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(-6, 0));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );
        cx.update(|_, cx| {
            let change = doc.update(cx, |doc, _| {
                doc.apply(Command::Delete {
                    items: vec![R1],
                    with_wires: false,
                })
                .unwrap()
            });
            overlay.update(cx, |o, cx| o.on_doc_change(&change, cx));
        });
        assert!(cx.update(|_, cx| overlay.read(cx).selection().is_empty()));
        up(&overlay, cx, pt(-6, 0));
        assert!(
            cx.update(|_, cx| overlay.read(cx).selection().is_empty()),
            "the deleted id is not selected again"
        );
    }

    fn open_hosted(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Host>,
        Entity<OverlayView>,
        Entity<Doc>,
        &mut VisualTestContext,
    ) {
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (host, cx) = cx.add_window_view(move |window, cx| {
            window.activate_window();
            let overlay = cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                OverlayView::new(doc_clone, camera, &GRAPHITE, cx)
            });
            Host {
                overlay: Some(overlay),
            }
        });
        cx.run_until_parked();
        let overlay = cx.update(|_, cx| host.read(cx).overlay.clone().unwrap());
        (host, overlay, doc, cx)
    }

    fn unmount(host: &Entity<Host>, cx: &mut App) {
        host.update(cx, |h, cx| {
            h.overlay = None;
            cx.notify();
        });
    }

    /// Review 6, finding 3: a Shift-click on an item while a band is live ends the band,
    /// and its capture is released even when a `SelectionChanged` subscriber unmounts the
    /// retained overlay before it can paint.
    #[gpui::test]
    fn t5_2_a_shift_click_ending_a_band_releases_without_a_paint(cx: &mut TestAppContext) {
        let (host, overlay, _doc, cx) = open_hosted(cx);
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(9, 8));
        assert!(captured(cx));
        let unmount_host = host.clone();
        cx.update(|_, cx| {
            cx.subscribe(
                &overlay,
                move |_, event: &crate::overlay::OverlayEvent, cx| {
                    if *event == crate::overlay::OverlayEvent::SelectionChanged {
                        unmount(&unmount_host, cx);
                    }
                },
            )
            .detach();
        });
        let at = screen(&overlay, cx, pt(-6, 0));
        cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::shift());
        cx.run_until_parked();
        assert!(
            cx.update(|_, cx| host.read(cx).overlay.is_none()),
            "unmounted"
        );
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert!(!captured(cx), "the band's capture is released");
    }

    /// Review 6, finding 3: `set_doc(Some(..))` ending a live band, with the retained overlay
    /// unmounted in the same update, releases the band's capture.
    #[gpui::test]
    fn t5_2_a_document_switch_ending_a_band_releases_without_a_paint(cx: &mut TestAppContext) {
        let (host, overlay, doc, cx) = open_hosted(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc), Some(overlay.clone()));
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(9, 8));
        assert!(captured(cx));
        cx.update(|_, cx| {
            canvas.update(cx, |c, cx| c.set_doc(Some(doc_b.clone()), cx));
            unmount(&host, cx);
        });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().is_none()));
        assert!(!captured(cx), "the band's capture is released");
    }

    /// Contract C2: a raw GPUI subscription to `CanvasHistoryEvent` yields nothing usable
    /// (`Debug` reveals no payload; the `compile_fail` doctests on `CanvasHistoryEvent` check
    /// field opacity from outside the crate).
    #[gpui::test]
    fn t5_2_raw_history_subscription_exposes_no_payload(cx: &mut TestAppContext) {
        let (_overlay, doc, cx) = open_overlay(cx);
        let canvas = canvas_for(cx, Some(doc.clone()), None);
        nudge(&doc, cx);
        let formatted = Rc::new(RefCell::new(Vec::new()));
        let sink = formatted.clone();
        cx.update(|_, cx| {
            let canvas_entity = canvas.clone();
            let sub = cx.new(|cx| Recorder {
                _subscription: cx.subscribe(
                    &canvas_entity,
                    move |_, _, event: &CanvasHistoryEvent, _| {
                        sink.borrow_mut().push(format!("{event:?}"));
                    },
                ),
            });
            std::mem::forget(sub);
        });
        let key_str = doc_key(&doc).to_string();
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        let logs = formatted.borrow();
        assert_eq!(logs.as_slice(), &["CanvasHistoryEvent(..)"]);
        assert!(
            !logs[0].contains(&key_str)
                && !logs[0].contains("can_undo")
                && !logs[0].contains("can_redo")
                && !logs[0].contains("cursor"),
            "raw Debug output must not reveal payload: {}",
            logs[0]
        );
    }

    /// Contract C3: a canvas subscribed to its own history via `subscribe_history` does not
    /// panic on a GPUI double-borrow, receives its own current-document history, and still
    /// drops stale history after a document switch in the same update.
    #[gpui::test]
    fn t5_2_a_canvas_can_subscribe_to_its_own_history(cx: &mut TestAppContext) {
        let (_overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.update(|_, cx| cx.new(|_| test_doc()));
        let canvas = canvas_for(cx, Some(doc_a.clone()), None);
        nudge(&doc_a, cx);
        nudge(&doc_a, cx);
        let heard = Rc::new(RefCell::new(Vec::new()));
        let sink = heard.clone();
        cx.update(|_, cx| {
            canvas.update(cx, |_, cx| {
                let me = cx.entity();
                SheetCanvas::subscribe_history(&me, cx, move |_, history, _| {
                    sink.borrow_mut().push(*history);
                })
                .detach();
            })
        });
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        assert_eq!(
            heard.borrow().iter().map(|h| h.doc).collect::<Vec<_>>(),
            vec![doc_key(&doc_a)]
        );
        // Undo on A and switch to B in one update: no panic, and A's stale history is dropped.
        heard.borrow_mut().clear();
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| {
                c.undo(window, cx);
                c.set_doc(Some(doc_b.clone()), cx);
            })
        });
        cx.run_until_parked();
        assert!(
            heard.borrow().is_empty(),
            "stale history for A is dropped after switching to B"
        );
    }

    /// Contract C4: a deferred capture release scheduled when a gesture ends without a window
    /// never releases a newer capture taken before the deferred update runs, even when GPUI
    /// reuses the same hitbox id within the frame (generation guard).
    #[gpui::test]
    fn t5_2_deferred_release_never_releases_a_newer_capture(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let rev0 = revision(&doc, cx);
        // Gesture 1: rubber band on empty space captures the current frame's hitbox.
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(4, -4));
        assert!(captured(cx));
        let first_hitbox = cx.update(|window, _| window.captured_hitbox().unwrap());

        // In one update (so no repaint occurs and GPUI keeps the exact same hitbox id):
        // end gesture 1 without a window (queuing a deferred release), then start gesture 2
        // (a drag on R1) on the same hitbox id before the deferred effect runs.
        let r1_screen = screen(&overlay, cx, pt(-6, 0));
        cx.update(|window, cx| {
            overlay.update(cx, |o, cx| o.cancel_rubber_band(cx));
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: MouseButton::Left,
                    position: r1_screen,
                    modifiers: Modifiers::none(),
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            // Both gestures captured the exact same hitbox id before any repaint, so only the
            // generation guard prevents gesture 1's deferred release from dropping gesture 2's
            // capture when effects flush.
            assert_eq!(window.captured_hitbox(), Some(first_hitbox));
        });
        cx.run_until_parked();

        assert_eq!(phase(&overlay, cx), ToolPhase::Dragging);
        assert!(
            captured(cx),
            "the deferred release from gesture 1 must not release gesture 2's capture"
        );
        // Gesture 2 still owns capture and commits once on release.
        drag_to(&overlay, cx, pt(-4, 2));
        up(&overlay, cx, pt(-4, 2));
        assert_eq!(revision(&doc, cx), rev0 + 1);
        assert_eq!(part_at(&doc, cx, 0), pt(-4, 2));
        assert!(!captured(cx));
    }

    /// Contract C4 (review P1): replacing a live pointer-driven rubber band with a programmatic
    /// preview ends the live band through `end_gesture` and releases its capture before any
    /// further pointer event.
    #[gpui::test]
    fn t5_2_set_rubber_band_preview_ends_live_band_and_releases_capture(cx: &mut TestAppContext) {
        let (overlay, _doc, cx) = open_overlay(cx);
        down(&overlay, cx, pt(0, -8));
        drag_to(&overlay, cx, pt(4, -4));
        assert!(captured(cx));
        cx.update(|_, cx| {
            overlay.update(cx, |o, cx| {
                o.set_rubber_band_preview(
                    pt(-2, -2),
                    pt(2, 2),
                    omnia_doc::prefs::SelectMode::Enclosed,
                    cx,
                );
            });
        });
        cx.run_until_parked();
        assert!(
            !captured(cx),
            "replacing a live band with a preview releases capture before any further pointer event"
        );
        let band = cx
            .update(|_, cx| overlay.read(cx).rubber_band())
            .expect("programmatic preview installed");
        assert!(!band.is_pointer_drag());
    }

    /// Contract C4 table-driven test: walks every gesture end path listed in canvas-plan /
    /// T5.2 section 3 (plus undo/redo and `set_rubber_band_preview`) across drag, rubber band,
    /// and pending place press, asserting at most one command, to the stamped document only,
    /// clean gesture state, and capture released.
    #[gpui::test]
    fn t5_2_every_gesture_end_path_satisfies_c4(cx: &mut TestAppContext) {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum GestureKind {
            Drag,
            Band,
            PendingPlace,
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum EndPath {
            Release,
            EscCancel,
            FreshPress,
            LostCapture,
            MoveOffSheet,
            ToolChange,
            WindowDeactivated,
            SetDocSome,
            SetDocNone,
            Attach,
            Replace,
            Detach,
            DropOverlay,
            ShiftClickWithBand,
            CancelRubberBand,
            SetRubberBandPreview,
            Undo,
            Redo,
        }

        let cases: &[(GestureKind, EndPath, u64)] = &[
            // Drag paths
            (GestureKind::Drag, EndPath::Release, 1),
            (GestureKind::Drag, EndPath::EscCancel, 0),
            (GestureKind::Drag, EndPath::FreshPress, 0),
            (GestureKind::Drag, EndPath::LostCapture, 0),
            (GestureKind::Drag, EndPath::MoveOffSheet, 0),
            (GestureKind::Drag, EndPath::ToolChange, 0),
            (GestureKind::Drag, EndPath::WindowDeactivated, 0),
            (GestureKind::Drag, EndPath::SetDocSome, 0),
            (GestureKind::Drag, EndPath::SetDocNone, 0),
            (GestureKind::Drag, EndPath::Attach, 0),
            (GestureKind::Drag, EndPath::Replace, 0),
            (GestureKind::Drag, EndPath::Detach, 0),
            (GestureKind::Drag, EndPath::DropOverlay, 0),
            (GestureKind::Drag, EndPath::Undo, 1),
            (GestureKind::Drag, EndPath::Redo, 1),
            // Rubber-band paths
            (GestureKind::Band, EndPath::Release, 0),
            (GestureKind::Band, EndPath::EscCancel, 0),
            (GestureKind::Band, EndPath::FreshPress, 0),
            (GestureKind::Band, EndPath::LostCapture, 0),
            (GestureKind::Band, EndPath::MoveOffSheet, 0),
            (GestureKind::Band, EndPath::ToolChange, 0),
            (GestureKind::Band, EndPath::WindowDeactivated, 0),
            (GestureKind::Band, EndPath::SetDocSome, 0),
            (GestureKind::Band, EndPath::SetDocNone, 0),
            (GestureKind::Band, EndPath::Attach, 0),
            (GestureKind::Band, EndPath::Replace, 0),
            (GestureKind::Band, EndPath::Detach, 0),
            (GestureKind::Band, EndPath::DropOverlay, 0),
            (GestureKind::Band, EndPath::ShiftClickWithBand, 0),
            (GestureKind::Band, EndPath::CancelRubberBand, 0),
            (GestureKind::Band, EndPath::SetRubberBandPreview, 0),
            (GestureKind::Band, EndPath::Undo, 1),
            (GestureKind::Band, EndPath::Redo, 1),
            // Pending place-press paths
            (GestureKind::PendingPlace, EndPath::Release, 1),
            (GestureKind::PendingPlace, EndPath::EscCancel, 0),
            (GestureKind::PendingPlace, EndPath::LostCapture, 0),
            (GestureKind::PendingPlace, EndPath::MoveOffSheet, 0),
            (GestureKind::PendingPlace, EndPath::ToolChange, 0),
            (GestureKind::PendingPlace, EndPath::WindowDeactivated, 0),
            (GestureKind::PendingPlace, EndPath::SetDocSome, 0),
            (GestureKind::PendingPlace, EndPath::SetDocNone, 0),
            (GestureKind::PendingPlace, EndPath::Attach, 0),
            (GestureKind::PendingPlace, EndPath::Replace, 0),
            (GestureKind::PendingPlace, EndPath::Detach, 0),
            (GestureKind::PendingPlace, EndPath::DropOverlay, 0),
            (GestureKind::PendingPlace, EndPath::Undo, 1),
            (GestureKind::PendingPlace, EndPath::Redo, 1),
        ];

        for &(kind, path, expected_edits) in cases {
            cx.update(|cx| cx.bind_keys(standalone_bindings()));
            let (host, overlay, doc_a, vcx) = open_hosted(cx);
            let doc_b = vcx.update(|_, cx| cx.new(|_| test_doc()));
            let b0 = vcx.update(|_, cx| snapshot(doc_b.read(cx)));
            let rev_b0 = revision(&doc_b, vcx);

            // Seed reversible history for Undo/Redo cases so the step actually runs and can be
            // distinguished from any stale drag commit (R1 stays at (-6, 0) while C1 moves).
            let expected_a = match path {
                EndPath::Undo => {
                    let before = vcx.update(|_, cx| snapshot(doc_a.read(cx)));
                    nudge(&doc_a, vcx);
                    Some(before)
                }
                EndPath::Redo => {
                    nudge(&doc_a, vcx);
                    let nudged = vcx.update(|_, cx| snapshot(doc_a.read(cx)));
                    vcx.update(|_, cx| {
                        doc_a.update(cx, |doc, _| {
                            assert!(doc.undo().is_some());
                        });
                    });
                    Some(nudged)
                }
                _ => {
                    let initial = vcx.update(|_, cx| snapshot(doc_a.read(cx)));
                    (expected_edits == 0).then_some(initial)
                }
            };
            let rev_a0 = revision(&doc_a, vcx);

            let canvas = match path {
                EndPath::Attach => Some(canvas_for(vcx, Some(doc_a.clone()), None)),
                EndPath::SetDocSome | EndPath::SetDocNone | EndPath::Replace | EndPath::Detach => {
                    Some(canvas_for(vcx, Some(doc_a.clone()), Some(overlay.clone())))
                }
                _ => None,
            };

            // Start the gesture.
            match kind {
                GestureKind::Drag => {
                    down(&overlay, vcx, pt(-6, 0));
                    drag_to(&overlay, vcx, pt(-4, 2));
                    assert_eq!(
                        phase(&overlay, vcx),
                        ToolPhase::Dragging,
                        "{kind:?}/{path:?}"
                    );
                    assert!(captured(vcx), "{kind:?}/{path:?}: drag takes capture");
                }
                GestureKind::Band => {
                    down(&overlay, vcx, pt(0, -8));
                    drag_to(&overlay, vcx, pt(9, 8));
                    assert!(
                        vcx.update(|_, cx| overlay.read(cx).rubber_band().is_some()),
                        "{kind:?}/{path:?}"
                    );
                    assert!(captured(vcx), "{kind:?}/{path:?}: band takes capture");
                }
                GestureKind::PendingPlace => {
                    vcx.update(|_, cx| {
                        overlay.update(cx, |o, cx| o.set_tool(Tool::Place(symbol()), cx));
                    });
                    vcx.run_until_parked();
                    down(&overlay, vcx, pt(2, 2));
                    assert!(
                        vcx.update(|_, cx| matches!(
                            overlay.read(cx).tool_state(),
                            ToolState::Placing(p) if p.pressed
                        )),
                        "{kind:?}/{path:?}"
                    );
                }
            }

            // Trigger the end path.
            let mut dropped = false;
            let weak = overlay.downgrade();
            match path {
                EndPath::Release => {
                    up(&overlay, vcx, pt(-4, 2));
                }
                EndPath::EscCancel => {
                    vcx.update(|window, cx| {
                        overlay.update(cx, |o, cx| o.cancel(window, cx));
                    });
                }
                EndPath::FreshPress => {
                    // Plain fresh press on C1 ends the stale drag or band with no edit and
                    // takes a brand-new capture for C1 (distinct from ShiftClickWithBand).
                    down(&overlay, vcx, pt(6, 0));
                }
                EndPath::ShiftClickWithBand => {
                    // Shift-click on R1 ends a live band without taking a new capture.
                    let at = screen(&overlay, vcx, pt(-6, 0));
                    vcx.simulate_mouse_down(at, MouseButton::Left, Modifiers::shift());
                }
                EndPath::LostCapture => {
                    let at = screen(&overlay, vcx, pt(-4, 2));
                    vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
                    vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
                    let button = match kind {
                        GestureKind::PendingPlace => None,
                        _ => Some(MouseButton::Left),
                    };
                    vcx.simulate_mouse_move(at, button, Modifiers::none());
                }
                EndPath::MoveOffSheet => {
                    let at = screen(&overlay, vcx, pt(-4, 2));
                    let button = match kind {
                        GestureKind::PendingPlace => None,
                        _ => {
                            vcx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
                            vcx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
                            Some(MouseButton::Left)
                        }
                    };
                    let outside = gpui::point(gpui::px(-400.0), gpui::px(-400.0));
                    vcx.simulate_mouse_move(outside, button, Modifiers::none());
                }
                EndPath::ToolChange => {
                    vcx.update(|_, cx| overlay.update(cx, |o, cx| o.set_tool(Tool::Wire, cx)));
                }
                EndPath::WindowDeactivated => {
                    vcx.deactivate_window();
                }
                EndPath::SetDocSome => {
                    let c = canvas.as_ref().unwrap();
                    vcx.update(|_, cx| c.update(cx, |c, cx| c.set_doc(Some(doc_b.clone()), cx)));
                }
                EndPath::SetDocNone => {
                    let c = canvas.as_ref().unwrap();
                    vcx.update(|_, cx| c.update(cx, |c, cx| c.set_doc(None, cx)));
                }
                EndPath::Attach => {
                    let c = canvas.as_ref().unwrap();
                    vcx.update(|_, cx| {
                        c.update(cx, |c, cx| c.set_overlay(Some(overlay.clone()), cx));
                    });
                }
                EndPath::Replace => {
                    let c = canvas.as_ref().unwrap();
                    let other = vcx.update(|_, cx| {
                        let d = doc_a.clone();
                        cx.new(|cx| {
                            OverlayView::new(
                                d,
                                Camera::new(Pt::ZERO, 1.0, Bounds::default()),
                                &GRAPHITE,
                                cx,
                            )
                        })
                    });
                    vcx.update(|_, cx| c.update(cx, |c, cx| c.set_overlay(Some(other), cx)));
                }
                EndPath::Detach => {
                    let c = canvas.as_ref().unwrap();
                    vcx.update(|_, cx| c.update(cx, |c, cx| c.set_overlay(None, cx)));
                }
                EndPath::DropOverlay => {
                    drop(overlay);
                    vcx.update(|_, cx| unmount(&host, cx));
                    dropped = true;
                }
                EndPath::CancelRubberBand => {
                    vcx.update(|_, cx| overlay.update(cx, |o, cx| o.cancel_rubber_band(cx)));
                }
                EndPath::SetRubberBandPreview => {
                    vcx.update(|_, cx| {
                        overlay.update(cx, |o, cx| {
                            o.set_rubber_band_preview(
                                pt(-2, -2),
                                pt(2, 2),
                                omnia_doc::prefs::SelectMode::Enclosed,
                                cx,
                            );
                        });
                    });
                }
                EndPath::Undo => {
                    vcx.update(|window, cx| {
                        assert!(overlay.update(cx, |o, cx| o.undo(window, cx)));
                    });
                }
                EndPath::Redo => {
                    vcx.update(|window, cx| {
                        assert!(overlay.update(cx, |o, cx| o.redo(window, cx)));
                    });
                }
            }
            vcx.run_until_parked();

            // FreshPress starts a brand-new capture on C1 after ending the stale gesture; verify
            // the stale gesture is gone and then cancel the fresh drag to confirm clean release.
            if path == EndPath::FreshPress {
                let live = weak.upgrade().unwrap();
                assert!(
                    captured(vcx),
                    "C4 ({kind:?}/FreshPress): fresh press on C1 takes its own capture"
                );
                assert!(
                    vcx.update(|_, cx| matches!(
                        live.read(cx).tool_state(),
                        ToolState::Dragging(d) if d.pressed == C1
                    )),
                    "C4 ({kind:?}/FreshPress): fresh drag is on C1, not the stale gesture"
                );
                assert!(
                    vcx.update(|_, cx| live.read(cx).rubber_band().is_none()),
                    "C4 ({kind:?}/FreshPress): stale rubber band cleared"
                );
                vcx.update(|window, cx| {
                    live.update(cx, |o, cx| o.cancel(window, cx));
                });
                vcx.run_until_parked();
            }

            // Assert C4: capture released, at most one command to stamped doc_a, doc_b untouched.
            assert!(
                !captured(vcx),
                "C4 failed ({kind:?}/{path:?}): capture was not released"
            );
            let edits_a = revision(&doc_a, vcx) - rev_a0;
            assert_eq!(
                edits_a, expected_edits,
                "C4 failed ({kind:?}/{path:?}): unexpected edit count on stamped doc"
            );
            assert!(
                edits_a <= 1,
                "C4 failed ({kind:?}/{path:?}): more than one command applied"
            );
            assert_eq!(
                revision(&doc_b, vcx),
                rev_b0,
                "C4 failed ({kind:?}/{path:?}): edited another document"
            );
            assert!(
                vcx.update(|_, cx| doc_b.read(cx).content_eq(&b0)),
                "C4 failed ({kind:?}/{path:?}): doc B content changed"
            );
            if let Some(expected) = &expected_a {
                assert!(
                    vcx.update(|_, cx| doc_a.read(cx).content_eq(expected)),
                    "C4 failed ({kind:?}/{path:?}): doc A content does not match expected state"
                );
                assert_eq!(
                    part_at(&doc_a, vcx, 0),
                    pt(-6, 0),
                    "C4 failed ({kind:?}/{path:?}): stale R1 drag must not commit"
                );
            }
            if dropped {
                assert!(
                    weak.upgrade().is_none(),
                    "{kind:?}/{path:?}: overlay dropped"
                );
            } else {
                let live = weak.upgrade().unwrap();
                assert_ne!(
                    phase(&live, vcx),
                    ToolPhase::Dragging,
                    "C4 failed ({kind:?}/{path:?}): drag state not cleared"
                );
                if path != EndPath::SetRubberBandPreview {
                    assert!(
                        vcx.update(|_, cx| live.read(cx).rubber_band().is_none()),
                        "C4 failed ({kind:?}/{path:?}): rubber band not cleared"
                    );
                } else {
                    assert!(
                        vcx.update(|_, cx| live
                            .read(cx)
                            .rubber_band()
                            .is_some_and(|b| !b.is_pointer_drag())),
                        "C4 failed ({kind:?}/{path:?}): preview should be non-capturing"
                    );
                }
                assert!(
                    vcx.update(|_, cx| !matches!(
                        live.read(cx).tool_state(),
                        ToolState::Placing(p) if p.pressed
                    )),
                    "C4 failed ({kind:?}/{path:?}): pending place press not cleared"
                );
                assert!(
                    vcx.update(|_, cx| live
                        .read(cx)
                        .sheet_entity()
                        .read(cx)
                        .hidden_items()
                        .is_empty()),
                    "T5.3/C4 failed ({kind:?}/{path:?}): hidden items not restored"
                );
            }

            // A stray mouse-up afterwards must commit nothing and leave capture clear.
            let rev_after = revision(&doc_a, vcx);
            vcx.simulate_mouse_up(
                gpui::point(gpui::px(100.0), gpui::px(100.0)),
                MouseButton::Left,
                Modifiers::none(),
            );
            vcx.run_until_parked();
            assert_eq!(
                revision(&doc_a, vcx),
                rev_after,
                "C4 failed ({kind:?}/{path:?}): stray release committed after end"
            );
            assert_eq!(revision(&doc_b, vcx), rev_b0);
            assert!(!captured(vcx));
        }
    }

    // ---- T5.3 simulated-input tests ----

    /// T5.3 done-when: placing a part from `Tool::Place` with R/X/Y reorientation, anchor-pin
    /// grid snapping (K2, verified on `IC-COM-OpAmp`), default `SymbolLibrary::place_command`
    /// fallback, Esc cancel, and undo/redo.
    #[gpui::test]
    fn t5_3_simulated_place_with_reorient_anchor_pin_snap_esc_and_undo(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let opamp_def = lib.get(opamp);

        // Enter Tool::Place(OpAmp).
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_tool(Tool::Place(opamp), cx)));
        cx.run_until_parked();
        assert_eq!(phase(&overlay, cx), ToolPhase::Placing);

        // Move pointer to an off-grid coordinate, then press R and X before placing.
        let raw = Pt {
            x: 2 * G + G / 4,
            y: -3 * G - G / 5,
        };
        let raw_screen = screen(&overlay, cx, raw);
        cx.simulate_mouse_move(raw_screen, None, Modifiers::none());
        cx.simulate_keystrokes("r x");
        let expected_orient = Orient::ROT_90.then(Orient::FLIP_X);
        let preview_at = cx.update(|_, cx| match overlay.read(cx).tool_state() {
            ToolState::Placing(p) => {
                assert_eq!(p.orient, expected_orient);
                p.at.expect("preview position computed")
            }
            other => panic!("expected Placing, got {other:?}"),
        });
        // Verify anchor pin 0 lies strictly on GRID_NM (K2) even for half-grid OpAmp.
        let preview_pin0 = preview_at + expected_orient.apply(opamp_def.pins[0].at);
        assert_eq!(preview_pin0.x % G, 0);
        assert_eq!(preview_pin0.y % G, 0);

        // Click to commit placement (fallback to SymbolLibrary::place_command).
        let rev0 = revision(&doc, cx);
        cx.simulate_mouse_down(raw_screen, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(raw_screen, MouseButton::Left, Modifiers::none());
        assert_eq!(revision(&doc, cx), rev0 + 1);
        assert_eq!(cx.update(|_, cx| doc.read(cx).part_count()), 3);

        let (placed_id, placed_part) = cx.update(|_, cx| {
            doc.read(cx)
                .parts()
                .find(|(_, p)| p.symbol == opamp)
                .map(|(id, p)| (id, p.clone()))
                .expect("placed op-amp exists")
        });
        assert_eq!(placed_part.orient, expected_orient);
        assert_eq!(&*placed_part.designator, "U1");
        let world_pin0 = cx.update(|_, cx| doc.read(cx).pins(placed_id).unwrap()[0]);
        assert_eq!(world_pin0.x % G, 0, "K2: anchor pin 0 x on grid");
        assert_eq!(world_pin0.y % G, 0, "K2: anchor pin 0 y on grid");

        // Esc leaves Tool::Place cleanly with no extra edit.
        let rev_placed = revision(&doc, cx);
        let placed_snap = cx.update(|_, cx| snapshot(doc.read(cx)));
        cx.simulate_keystrokes("escape");
        assert_eq!(cx.update(|_, cx| overlay.read(cx).tool()), Tool::Select);
        assert_eq!(revision(&doc, cx), rev_placed);

        // Undo and redo revert and restore the placement cleanly (INV4/INV5).
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        cx.simulate_keystrokes("ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&placed_snap)));
    }

    /// T5.3 done-when: `SheetCanvas::set_place_provider` (parts-plan.md §14) commits the
    /// stub provider's returned `Command`, works regardless of `set_overlay` call order,
    /// honours `None` refusal, emits `CanvasHistory`, and undoes cleanly.
    #[gpui::test]
    fn t5_3_stub_placement_provider_command_is_committed_and_undone(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let captured_requests = Rc::new(RefCell::new(Vec::<PlaceRequest>::new()));
        let reqs_clone = captured_requests.clone();
        let lib_clone = lib.clone();
        let stub: PlaceProvider = Rc::new(move |req| {
            reqs_clone.borrow_mut().push(req.clone());
            Some(lib_clone.place_command(
                req.symbol,
                req.at,
                req.orient,
                req.designator,
                std::sync::Arc::from("4.7k-stub"),
            ))
        });

        // Install provider on SheetCanvas BEFORE attaching overlay to prove order-independence.
        let canvas = cx.update(|_, cx| {
            let doc = doc.clone();
            let overlay = overlay.clone();
            cx.new(|cx| {
                let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
                let mut canvas = SheetCanvas::new(camera, &GRAPHITE, cx).with_doc(doc);
                canvas.set_place_provider(Some(stub), cx);
                canvas.set_overlay(Some(overlay), cx);
                canvas
            })
        });
        let history = record_history(&canvas, cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));

        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_tool(Tool::Place(res), cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("y");
        let target = pt(2, -3);
        down(&overlay, cx, target);
        up(&overlay, cx, target);

        // Stub provider received the exact snapped request and its custom value was committed.
        let reqs = captured_requests.borrow();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].symbol, res);
        assert_eq!(reqs[0].at, target);
        assert_eq!(reqs[0].orient, Orient::FLIP_Y);
        assert_eq!(&*reqs[0].designator, "R2"); // R1 already exists in test_doc().
        let placed = cx.update(|_, cx| {
            doc.read(cx)
                .parts()
                .find(|(_, p)| &*p.designator == "R2")
                .map(|(_, p)| p.clone())
                .expect("stub-provided part committed")
        });
        assert_eq!(&*placed.value, "4.7k-stub");
        assert_eq!(placed.at, target);
        assert_eq!(placed.orient, Orient::FLIP_Y);
        assert_eq!(history.borrow().len(), 1, "C2: placement emits history");

        // A provider returning None declines placement without modifying the document.
        let rev_before = revision(&doc, cx);
        let decline: PlaceProvider = Rc::new(|_| None);
        cx.update(|_, cx| canvas.update(cx, |c, cx| c.set_place_provider(Some(decline), cx)));
        down(&overlay, cx, pt(4, -3));
        up(&overlay, cx, pt(4, -3));
        assert_eq!(revision(&doc, cx), rev_before);

        // Undo via SheetCanvas reverts the stub-placed part.
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
    }

    /// T5.3 done-when: `SheetView` paints **exactly twice per drag** (once at drag start to
    /// hide dragged items, 0 times across drag motion, once at drag end to restore & commit),
    /// and hidden items are present during drag and cleared on end.
    #[gpui::test]
    fn t5_3_sheet_view_paints_exactly_twice_per_drag_commit_and_cancel(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));
        let paints =
            |cx: &mut VisualTestContext| cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let hidden = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                overlay
                    .read(cx)
                    .sheet_entity()
                    .read(cx)
                    .hidden_items()
                    .to_vec()
            })
        };

        // 1. Drag-move commit: exactly 2 SheetView paints (start + end).
        let p0 = paints(cx);
        down(&overlay, cx, pt(-6, 0));
        assert_eq!(
            paints(cx),
            p0,
            "press before drag lift-off does not paint SheetView"
        );
        assert!(hidden(cx).is_empty(), "items stay visible until drag lifts");
        drag_to(&overlay, cx, pt(-5, 0));
        assert_eq!(
            paints(cx),
            p0 + 1,
            "drag start (lift-off) paints SheetView once (hide)"
        );
        assert_eq!(hidden(cx), vec![R1], "R1 hidden from SheetView during drag");
        for step in 1..=20 {
            let at = Pt {
                x: -5 * G + step * G / 3,
                y: step * G / 5,
            };
            drag_to(&overlay, cx, at);
            assert_eq!(
                paints(cx),
                p0 + 1,
                "INV11: 0 SheetView paints during drag move step {step}"
            );
        }
        up(&overlay, cx, pt(-2, 2));
        assert_eq!(
            paints(cx),
            p0 + 2,
            "T5.3 done-when: SheetView painted exactly twice for the entire drag"
        );
        assert!(hidden(cx).is_empty(), "hidden items cleared on drag commit");
        assert_eq!(part_at(&doc, cx, 0), pt(-2, 2));
        let after_move = cx.update(|_, cx| snapshot(doc.read(cx)));

        // Undo restores original state, redo reapplies the plain Move, undo returns to start.
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        cx.simulate_keystrokes("ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&after_move)));
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));

        // 2. Drag cancelled via shell path (`cancel`, no modality switch): also exactly 2 paints.
        // Switch back to pointer modality first with a benign hover move so `p1` is clean.
        let hover_pt = screen(&overlay, cx, pt(0, -8));
        cx.simulate_mouse_move(hover_pt, None, Modifiers::none());
        let p1 = paints(cx);
        let rev1 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        assert_eq!(paints(cx), p1);
        drag_to(&overlay, cx, pt(-5, 1));
        assert_eq!(paints(cx), p1 + 1);
        assert_eq!(hidden(cx), vec![R1]);
        for step in 2..=10 {
            drag_to(&overlay, cx, pt(-6 + step, 1));
            assert_eq!(paints(cx), p1 + 1);
        }
        assert!(cx.update(|window, cx| overlay.update(cx, |o, cx| o.cancel(window, cx))));
        cx.run_until_parked();
        assert_eq!(
            paints(cx),
            p1 + 2,
            "T5.3 done-when: cancelled drag also paints SheetView exactly twice"
        );
        assert!(hidden(cx).is_empty(), "hidden items restored on cancel");
        assert_eq!(
            revision(&doc, cx),
            rev1,
            "INV5: cancel leaves revision unchanged"
        );
    }

    /// T5.3 done-when: rotate (R) and mirror (X, Y) during a drag commit as one `Command::Transform`
    /// reverted by a single undo, and idle R/X/Y on a selection commit `Command::Reorient` with
    /// undo/redo.
    #[gpui::test]
    fn t5_3_rotate_and_mirror_during_drag_and_idle_with_undo(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));

        // 1. Mid-drag rotate + mirror X + mirror Y -> one Command::Transform, one undo.
        let rev0 = revision(&doc, cx);
        down(&overlay, cx, pt(-6, 0));
        drag_to(&overlay, cx, pt(-3, 2));
        cx.simulate_keystrokes("r");
        assert_eq!(
            revision(&doc, cx),
            rev0,
            "INV5: R mid-drag does not edit doc"
        );
        cx.simulate_keystrokes("x");
        assert_eq!(
            revision(&doc, cx),
            rev0,
            "INV5: X mid-drag does not edit doc"
        );
        cx.simulate_keystrokes("y");
        assert_eq!(
            revision(&doc, cx),
            rev0,
            "INV5: Y mid-drag does not edit doc"
        );
        up(&overlay, cx, pt(-3, 2));
        assert_eq!(
            revision(&doc, cx),
            rev0 + 1,
            "one Transform commit on release"
        );
        assert_eq!(part_at(&doc, cx, 0), pt(-3, 2));
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y)
        );
        let after_drag = cx.update(|_, cx| snapshot(doc.read(cx)));
        cx.simulate_keystrokes("ctrl-z");
        assert!(
            cx.update(|_, cx| doc.read(cx).content_eq(&original)),
            "one undo reverts the entire drag + R + X + Y"
        );
        cx.simulate_keystrokes("ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&after_drag)));
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));

        // 2. Idle R, X, Y on a selected part -> direct Command::Reorient + undo/redo.
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_selection([R1], cx)));
        let rev1 = revision(&doc, cx);
        cx.simulate_keystrokes("r");
        assert_eq!(revision(&doc, cx), rev1 + 1);
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::ROT_90
        );
        cx.simulate_keystrokes("x");
        assert_eq!(revision(&doc, cx), rev1 + 2);
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::ROT_90.then(Orient::FLIP_X)
        );
        cx.simulate_keystrokes("y");
        assert_eq!(revision(&doc, cx), rev1 + 3);
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).part(PartId(0)).unwrap().orient),
            Orient::ROT_90.then(Orient::FLIP_X).then(Orient::FLIP_Y)
        );
        let reoriented = cx.update(|_, cx| snapshot(doc.read(cx)));
        cx.simulate_keystrokes("ctrl-z ctrl-z ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        cx.simulate_keystrokes("ctrl-y ctrl-y ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&reoriented)));
    }

    /// T5.3 done-when: Del key and `SheetCanvas::delete_selection` delete the selected items,
    /// prune selection, emit `CanvasHistory`, and undo/redo restore exact items and IDs.
    #[gpui::test]
    fn t5_3_delete_selection_via_key_and_canvas_method_with_undo(cx: &mut TestAppContext) {
        let (overlay, doc, cx) = open_overlay(cx);
        let canvas = canvas_for(cx, Some(doc.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        let original = cx.update(|_, cx| snapshot(doc.read(cx)));

        // Del with empty selection is a no-op.
        let rev0 = revision(&doc, cx);
        cx.simulate_keystrokes("delete");
        assert_eq!(revision(&doc, cx), rev0);
        assert!(history.borrow().is_empty());

        // Select R1 and press Del.
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_selection([R1], cx)));
        cx.simulate_keystrokes("delete");
        assert_eq!(revision(&doc, cx), rev0 + 1);
        assert!(cx.update(|_, cx| doc.read(cx).part(PartId(0)).is_none()));
        assert!(cx.update(|_, cx| overlay.read(cx).selection().is_empty()));
        assert_eq!(history.borrow().len(), 1);
        let after_del = cx.update(|_, cx| snapshot(doc.read(cx)));

        // Undo restores R1 with exact id, redo deletes again.
        cx.simulate_keystrokes("ctrl-z");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        cx.simulate_keystrokes("ctrl-y");
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&after_del)));

        // Select C1 and call `SheetCanvas::delete_selection` (shell's Del path, ui-plan §14).
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_selection([C1], cx)));
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.delete_selection(window, cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| doc.read(cx).part(PartId(1)).is_none()));
        cx.update(|window, cx| canvas.update(cx, |c, cx| c.undo(window, cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| doc.read(cx).content_eq(&after_del)));
    }

    /// T5.3 done-when: dragging an off-grid part snaps its anchor pin (pin 0) onto `GRID_NM` (K2).
    #[gpui::test]
    fn t5_3_drag_part_snaps_anchor_pin_to_grid(cx: &mut TestAppContext) {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        // Start with OpAmp at (0, 0) so its pin 0 is off-grid at (-4.5*G, -1.5*G).
        let doc = cx.new(|_| {
            Doc::from_items(
                [(
                    Part {
                        symbol: opamp,
                        at: Pt::ZERO,
                        orient: Orient::IDENTITY,
                        designator: "U1".into(),
                        value: "".into(),
                    },
                    def.pins.iter().map(|p| p.at).collect(),
                    def.bounds,
                )],
                [],
                [],
            )
            .unwrap()
        });
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            window.activate_window();
            window.focus(view.focus_handle(), cx);
            view
        });
        cx.run_until_parked();

        down(&overlay, cx, Pt::ZERO);
        let dest = Pt {
            x: 2 * G + G / 4,
            y: -G - G / 5,
        };
        drag_to(&overlay, cx, dest);
        up(&overlay, cx, dest);

        let pin0 = cx.update(|_, cx| doc.read(cx).pins(PartId(0)).unwrap()[0]);
        assert_eq!(
            pin0.x % G,
            0,
            "K2: dragged OpAmp pin 0 x must land on GRID_NM"
        );
        assert_eq!(
            pin0.y % G,
            0,
            "K2: dragged OpAmp pin 0 y must land on GRID_NM"
        );
    }

    /// T5.3 P1 regression: clicking an off-grid part (`IC-COM-OpAmp` at `(0, 0)`) with 1 px of
    /// pointer jitter selects it with unchanged revision and 0 `SheetView` paints instead of
    /// snapping into a half-grid move.
    #[gpui::test]
    fn t5_3_click_with_one_pixel_jitter_on_off_grid_part_selects_without_moving(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        let doc = cx.new(|_| {
            Doc::from_items(
                [(
                    Part {
                        symbol: opamp,
                        at: Pt::ZERO,
                        orient: Orient::IDENTITY,
                        designator: "U1".into(),
                        value: "".into(),
                    },
                    def.pins.iter().map(|p| p.at).collect(),
                    def.bounds,
                )],
                [],
                [],
            )
            .unwrap()
        });
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            window.activate_window();
            window.focus(view.focus_handle(), cx);
            view
        });
        cx.run_until_parked();

        let rev0 = revision(&doc, cx);
        let paints0 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let p = screen(&overlay, cx, Pt::ZERO);
        let jitter = gpui::point(p.x + gpui::px(1.0), p.y);
        cx.simulate_mouse_down(p, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(jitter, Some(MouseButton::Left), Modifiers::none());
        cx.simulate_mouse_up(jitter, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();

        assert_eq!(
            revision(&doc, cx),
            rev0,
            "1 px click jitter on off-grid part must not commit a move"
        );
        assert_eq!(part_at(&doc, cx, 0), Pt::ZERO);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0))],
            "1 px click jitter on off-grid part must still select it"
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints0,
            "INV11: 1 px click jitter must cause 0 SheetView paints"
        );
    }

    /// T5.3 P2 regression: installing `set_place_provider` while the attached overlay temporarily
    /// displays another document still applies once `set_doc` aligns them (parts-plan §14).
    #[gpui::test]
    fn t5_3_place_provider_set_during_doc_mismatch_applies_after_set_doc(cx: &mut TestAppContext) {
        let (overlay, doc_a, cx) = open_overlay(cx);
        let doc_b = cx.new(|_| Doc::new());
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let lib_clone = lib.clone();
        let stub: PlaceProvider = Rc::new(move |req| {
            Some(lib_clone.place_command(
                req.symbol,
                req.at,
                req.orient,
                req.designator,
                std::sync::Arc::from("from-mismatch-stub"),
            ))
        });

        let canvas = canvas_for(cx, Some(doc_a.clone()), Some(overlay.clone()));
        // Temporarily point the overlay at doc_b so `gesture_owner` returns None.
        cx.update(|_, cx| {
            overlay.update(cx, |o, cx| o.set_document(doc_b.clone(), cx));
            canvas.update(cx, |c, cx| c.set_place_provider(Some(stub), cx));
        });
        // Now realign canvas + overlay on doc_b via `set_doc` and commit a placement.
        cx.update(|_, cx| {
            canvas.update(cx, |c, cx| {
                c.set_doc(Some(doc_b.clone()), cx);
                c.set_tool(Tool::Place(res), cx);
            });
        });
        cx.run_until_parked();
        down(&overlay, cx, pt(1, 1));
        up(&overlay, cx, pt(1, 1));

        let placed = cx.update(|_, cx| {
            doc_b
                .read(cx)
                .parts()
                .next()
                .map(|(_, p)| p.clone())
                .expect("provider committed part on doc_b")
        });
        assert_eq!(&*placed.value, "from-mismatch-stub");
    }

    // ---- W3.5: Delete with wires through the canvas (WD10, WQ4, TC-del-01..03) ----

    /// A resistor (pins at `at` ± 2 grid steps on x) for the TC-del fixtures.
    fn del_resistor(at: Pt, designator: &str) -> (Part, Vec<Pt>, Rect) {
        let lib = SymbolLibrary::builtin();
        let symbol = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(symbol);
        (
            Part {
                symbol,
                at,
                orient: Orient::IDENTITY,
                designator: designator.into(),
                value: "1".into(),
            },
            def.pins.iter().map(|p| p.at).collect(),
            def.bounds,
        )
    }

    fn del_wire(a: (i32, i32), b: (i32, i32)) -> Wire {
        Wire {
            a: pt(a.0, a.1),
            b: pt(b.0, b.1),
        }
    }

    /// TC-del-01/02: R1 at (-6, 0) with pins (-8, 0) and (-4, 0); C1 at (6, 6) with pins
    /// (4, 6) and (8, 6). The bent run W0-W1-W2 goes from R1's right pin to C1's left pin,
    /// the run W4 goes from R1's left pin to a free end, and W3 continues beyond C1 to the
    /// label OUT.
    fn tc_del_01_doc() -> Doc {
        Doc::from_items(
            [del_resistor(pt(-6, 0), "R1"), del_resistor(pt(6, 6), "C1")],
            [
                del_wire((-4, 0), (0, 0)),
                del_wire((0, 0), (0, 6)),
                del_wire((0, 6), (4, 6)),
                del_wire((8, 6), (12, 6)),
                del_wire((-12, 0), (-8, 0)),
            ],
            [Label {
                at: pt(12, 6),
                text: "OUT".into(),
            }],
        )
        .unwrap()
    }

    /// TC-del-03: R1's run W0-W1 ends on the junction (0, 6), where W2 and W3 meet
    /// collinearly and go on to free ends.
    fn tc_del_03_doc() -> Doc {
        Doc::from_items(
            [del_resistor(pt(-6, 0), "R1")],
            [
                del_wire((-4, 0), (0, 0)),
                del_wire((0, 0), (0, 6)),
                del_wire((-6, 6), (0, 6)),
                del_wire((0, 6), (6, 6)),
            ],
            [],
        )
        .unwrap()
    }

    fn live_wires(doc: &Doc) -> Vec<(WireId, Wire)> {
        doc.wires().map(|(id, &w)| (id, w)).collect()
    }

    fn doc_points(doc: &Doc) -> Vec<Pt> {
        let mut points: Vec<Pt> = doc
            .wires()
            .flat_map(|(_, w)| [w.a, w.b])
            .chain(
                doc.parts()
                    .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec()),
            )
            .chain(doc.labels().map(|(_, l)| l.at))
            .collect();
        points.sort_by_key(|p| (p.x, p.y));
        points.dedup();
        points
    }

    /// INV10: the document's incremental connectivity and junctions agree with an
    /// independent rebuild over its live parts, wires and labels, including the union of pre-
    /// and post-edit fixture points and empty `net_of` for removed wire ids.
    fn assert_inv10(doc: &Doc, extra_points: &[Pt], known_wires: &[WireId]) {
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &w)| (id, w)),
            doc.labels(),
            doc.parts()
                .map(|(id, _)| (id, doc.pins(id).unwrap().to_vec())),
        );
        assert_eq!(doc.net_count(), oracle.net_count(), "INV10: net count");
        let ends = || {
            doc.wires().flat_map(|(_, w)| [w.a, w.b]).chain(
                doc.parts()
                    .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec()),
            )
        };
        let mut points = doc_points(doc);
        points.extend_from_slice(extra_points);
        points.sort_by_key(|p| (p.x, p.y));
        points.dedup();
        for &p in &points {
            assert_eq!(
                doc.net_at(p).is_some(),
                oracle.net_at(p).is_some(),
                "INV10: {p:?}"
            );
            for &q in &points {
                assert_eq!(
                    doc.net_at(p) == doc.net_at(q),
                    oracle.net_at(p) == oracle.net_at(q),
                    "INV10: {p:?} and {q:?}"
                );
            }
        }
        for (id, w) in doc.wires() {
            assert_eq!(doc.net_of(id), doc.net_at(w.a), "INV10: {id:?}");
            assert_eq!(doc.net_of(id), doc.net_at(w.b), "INV10: {id:?}");
        }
        for &id in known_wires {
            if doc.wire(id).is_none() {
                assert_eq!(doc.net_of(id), None, "INV10: removed {id:?} kept a net");
            }
        }
        let mut degree = std::collections::BTreeMap::<(i32, i32), usize>::new();
        for p in ends() {
            *degree.entry((p.x, p.y)).or_default() += 1;
        }
        let expected: Vec<(i32, i32)> = degree
            .into_iter()
            .filter_map(|(p, n)| (n >= 3).then_some(p))
            .collect();
        let sheet = Rect {
            min: Pt {
                x: -SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
            max: Pt {
                x: SHEET_LIMIT_NM,
                y: SHEET_LIMIT_NM,
            },
        };
        let mut found = Vec::new();
        doc.junctions(sheet, &mut found);
        let mut found: Vec<(i32, i32)> = found.iter().map(|p| (p.x, p.y)).collect();
        found.sort_unstable();
        assert_eq!(found, expected, "INV10: junctions");
    }

    /// The pieces a W3.5 test drives: the overlay, its document, an attached `SheetCanvas`
    /// and the history that canvas re-emits.
    struct DelRig<'a> {
        overlay: Entity<OverlayView>,
        doc: Entity<Doc>,
        canvas: Entity<SheetCanvas>,
        history: Rc<RefCell<Vec<CanvasHistory>>>,
        cx: &'a mut VisualTestContext,
    }

    fn del_prefs(with_wires: bool) -> WiringPrefs {
        let mut prefs = WiringPrefs::default();
        prefs.delete_wires_with_part = with_wires;
        prefs
    }

    /// An overlay showing `doc` with `prefs`, focused, with its canvas and history log.
    fn open_del_rig(cx: &mut TestAppContext, doc: Doc, with_wires: bool) -> DelRig<'_> {
        cx.update(|cx| cx.bind_keys(standalone_bindings()));
        let prefs = del_prefs(with_wires);
        let doc = cx.new(|_| doc);
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::with_prefs(doc_clone, camera, &GRAPHITE, &prefs, cx);
            window.activate_window();
            window.focus(view.focus_handle(), cx);
            view
        });
        cx.run_until_parked();
        let canvas = canvas_for(cx, Some(doc.clone()), Some(overlay.clone()));
        let history = record_history(&canvas, cx);
        DelRig {
            overlay,
            doc,
            canvas,
            history,
            cx,
        }
    }

    /// How a test asks for Delete.
    #[derive(Clone, Copy, Debug)]
    enum DelPath {
        Key,
        Overlay,
        Canvas,
    }

    impl DelRig<'_> {
        fn snapshot(&mut self) -> Doc {
            let doc = self.doc.clone();
            self.cx.update(|_, cx| snapshot(doc.read(cx)))
        }

        fn select(&mut self, items: &[ItemId]) {
            let overlay = self.overlay.clone();
            let items = items.to_vec();
            self.cx
                .update(|_, cx| overlay.update(cx, |o, cx| o.set_selection(items, cx)));
        }

        fn set_with_wires(&mut self, with_wires: bool) {
            let overlay = self.overlay.clone();
            self.cx.update(|_, cx| {
                overlay.update(cx, |o, cx| o.apply_prefs(&del_prefs(with_wires), cx))
            });
        }

        fn delete(&mut self, path: DelPath) {
            let (overlay, canvas) = (self.overlay.clone(), self.canvas.clone());
            match path {
                DelPath::Key => self.cx.simulate_keystrokes("delete"),
                DelPath::Overlay => self.cx.update(|window, cx| {
                    overlay.update(cx, |o, cx| o.delete_selection(window, cx))
                }),
                DelPath::Canvas => self
                    .cx
                    .update(|window, cx| canvas.update(cx, |c, cx| c.delete_selection(window, cx))),
            }
            self.cx.run_until_parked();
        }

        /// Deletes `selection` through `path` and checks one command, one undo step and one
        /// history event; INV10 after the delete, after undo and after redo; and that undo
        /// restores exact content and ids and redo repeats the delete (INV4). Returns the
        /// document after the delete.
        fn delete_roundtrip(&mut self, selection: &[ItemId], path: DelPath) -> Doc {
            let doc = self.doc.clone();
            let before = self.snapshot();
            let before_points = doc_points(&before);
            let before_wires: Vec<WireId> = before.wires().map(|(id, _)| id).collect();
            let rev = revision(&doc, self.cx);
            let events = self.history.borrow().len();
            self.select(selection);
            self.delete(path);

            assert_eq!(revision(&doc, self.cx), rev + 1, "{path:?}: one command");
            let cursor = self.cx.update(|_, cx| doc.read(cx).history_cursor());
            assert_eq!(
                &self.history.borrow()[events..],
                &[CanvasHistory {
                    doc: doc_key(&doc),
                    can_undo: true,
                    can_redo: false,
                    cursor,
                }],
                "{path:?}: one history event"
            );
            let after = self.snapshot();
            let mut all_points = before_points;
            all_points.extend(doc_points(&after));
            all_points.sort_by_key(|p| (p.x, p.y));
            all_points.dedup();
            let mut all_wires = before_wires;
            all_wires.extend(after.wires().map(|(id, _)| id));
            all_wires.sort_unstable_by_key(|id| id.0);
            all_wires.dedup();
            let overlay = self.overlay.clone();
            assert!(
                self.cx
                    .update(|_, cx| overlay.read(cx).selection().is_empty())
            );
            assert_inv10(&after, &all_points, &all_wires);
            self.cx
                .update(|_, cx| assert_inv10(doc.read(cx), &all_points, &all_wires));

            self.cx.simulate_keystrokes("ctrl-z");
            assert!(self.cx.update(|_, cx| doc.read(cx).content_eq(&before)));
            self.cx
                .update(|_, cx| assert_inv10(doc.read(cx), &all_points, &all_wires));
            self.cx.simulate_keystrokes("ctrl-y");
            assert!(self.cx.update(|_, cx| doc.read(cx).content_eq(&after)));
            self.cx
                .update(|_, cx| assert_inv10(doc.read(cx), &all_points, &all_wires));
            assert_eq!(self.history.borrow().len(), events + 3);
            after
        }

        /// Undoes the last step and checks the document is `content` again.
        fn undo_to(&mut self, content: &Doc) {
            let doc = self.doc.clone();
            self.cx.simulate_keystrokes("ctrl-z");
            assert!(self.cx.update(|_, cx| doc.read(cx).content_eq(content)));
        }
    }

    /// TC-del-02 (preference off, the WQ4 default) through the Del key: only R1 goes; every
    /// wire stays with its id and keeps a free end where R1's pins were.
    #[gpui::test]
    fn tc_del_02_del_key_flag_off_removes_only_the_part_inv4_inv10(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), false);
        let original = rig.snapshot();
        let after = rig.delete_roundtrip(&[R1], DelPath::Key);
        assert!(after.part(PartId(0)).is_none());
        assert_eq!(
            after.part(PartId(1)).cloned(),
            original.part(PartId(1)).cloned()
        );
        assert_eq!(live_wires(&after), live_wires(&original));
        assert_eq!(
            after
                .labels()
                .map(|(id, l)| (id, l.clone()))
                .collect::<Vec<_>>(),
            original
                .labels()
                .map(|(id, l)| (id, l.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// TC-del-01 (preference on) through the Del key: R1 goes with both runs that start at
    /// its pins, up to their far anchors (C1's pin; a free end). W3 beyond C1 and the label
    /// stay with their ids.
    #[gpui::test]
    fn tc_del_01_del_key_flag_on_removes_runs_to_far_anchors_inv4_inv10(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), true);
        let original = rig.snapshot();
        let after = rig.delete_roundtrip(&[R1], DelPath::Key);
        assert!(after.part(PartId(0)).is_none());
        assert_eq!(
            after.part(PartId(1)).cloned(),
            original.part(PartId(1)).cloned()
        );
        assert_eq!(
            live_wires(&after),
            vec![(WireId(3), del_wire((8, 6), (12, 6)))]
        );
        assert_eq!(
            after
                .labels()
                .map(|(id, l)| (id, l.clone()))
                .collect::<Vec<_>>(),
            original
                .labels()
                .map(|(id, l)| (id, l.clone()))
                .collect::<Vec<_>>()
        );
    }

    /// TC-del-03 (preference on) through the Del key: R1's run ends on a junction; the run
    /// goes, and the junction's other two wires, now collinear through a degree-2 point,
    /// merge into one.
    #[gpui::test]
    fn tc_del_03_del_key_run_to_junction_merges_the_rest_inv4_inv10(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_03_doc(), true);
        let after = rig.delete_roundtrip(&[R1], DelPath::Key);
        assert_eq!(after.parts().count(), 0);
        let wires = live_wires(&after);
        assert_eq!(wires.len(), 1);
        assert_eq!(wires[0].1, del_wire((-6, 6), (6, 6)));
        assert!(after.wire(WireId(0)).is_none() && after.wire(WireId(1)).is_none());
        let mut junctions = Vec::new();
        after.junctions(
            Rect {
                min: pt(-20, -20),
                max: pt(20, 20),
            },
            &mut junctions,
        );
        assert!(junctions.is_empty());
    }

    /// W3.5 audit (WD10, WQ4): the Del key, `OverlayView::delete_selection` and
    /// `SheetCanvas::delete_selection` each read the preference current at the Delete, so a
    /// change through `apply_prefs` reaches every path.
    #[gpui::test]
    fn w3_5_every_delete_path_reads_the_current_preference(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), false);
        let original = rig.snapshot();
        let all_wires = live_wires(&original);
        let beyond = vec![(WireId(3), del_wire((8, 6), (12, 6)))];
        for (with_wires, path) in [
            (false, DelPath::Canvas),
            (false, DelPath::Overlay),
            (true, DelPath::Key),
            (true, DelPath::Overlay),
            (true, DelPath::Canvas),
            (false, DelPath::Key),
        ] {
            rig.set_with_wires(with_wires);
            let after = rig.delete_roundtrip(&[R1], path);
            let expected = if with_wires { &beyond } else { &all_wires };
            assert_eq!(&live_wires(&after), expected, "{with_wires} {path:?}");
            assert!(after.part(PartId(0)).is_none());
            rig.undo_to(&original);
        }
    }

    /// W3.5 edge cases with the preference on: Del mid-drag changes nothing (the drag never
    /// commits over deleted items), and a hovered wire removed with its part leaves neither
    /// the overlay's hover nor the tool's hover mirror pointing at it.
    #[gpui::test]
    fn w3_5_del_mid_drag_is_ignored_and_hover_on_a_removed_run_clears(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), true);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());

        // Mid-drag: Del is ignored; Esc cancels with no edit (INV5).
        rig.select(&[R1]);
        let rev = revision(&doc, rig.cx);
        down(&overlay, rig.cx, pt(-6, 0));
        drag_to(&overlay, rig.cx, pt(-6, 2));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Dragging);
        rig.cx.simulate_keystrokes("delete");
        assert_eq!(revision(&doc, rig.cx), rev);
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Dragging);
        rig.cx.simulate_keystrokes("escape");
        up(&overlay, rig.cx, pt(-6, 2));
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert!(rig.history.borrow().is_empty());

        // Hover W1 (in R1's run), then delete R1 with its runs.
        let w1 = ItemId::Wire(WireId(1));
        let at = screen(&overlay, rig.cx, pt(0, 3));
        rig.cx.simulate_mouse_move(at, None, Modifiers::none());
        assert_eq!(rig.cx.update(|_, cx| overlay.read(cx).hovered()), Some(w1));
        assert_eq!(
            rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Hovering(w1)
        );
        rig.select(&[R1]);
        rig.delete(DelPath::Key);
        assert!(
            rig.cx
                .update(|_, cx| doc.read(cx).wire(WireId(1)).is_none())
        );
        assert_eq!(rig.cx.update(|_, cx| overlay.read(cx).hovered()), None);
        assert_eq!(
            rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Idle
        );
        assert_eq!(rig.history.borrow().len(), 1);
    }

    /// W3.5 review finding B1: direct `ToolHost::request(ToolRequest::DeleteSelection)` must
    /// honour the active-gesture guard (`Dragging`, `Placing`, `Wiring`) just like
    /// `ToolMachine::delete` (INV5).
    #[gpui::test]
    fn w3_5_direct_delete_request_is_ignored_during_active_gestures(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), true);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        let rev = revision(&doc, rig.cx);

        // Dragging
        rig.select(&[R1]);
        down(&overlay, rig.cx, pt(-6, 0));
        drag_to(&overlay, rig.cx, pt(-6, 2));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Dragging);
        rig.cx.update(|_, cx| {
            overlay.update(cx, |o, cx| {
                ToolHost::request(o, ToolRequest::DeleteSelection, cx)
            });
        });
        rig.cx.run_until_parked();
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Dragging);
        rig.cx.simulate_keystrokes("escape");
        up(&overlay, rig.cx, pt(-6, 2));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle);

        // Placing
        rig.select(&[R1]);
        rig.cx.update(|_, cx| {
            overlay.update(cx, |o, cx| o.set_tool(Tool::Place(symbol()), cx));
        });
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Placing);
        rig.cx.update(|_, cx| {
            overlay.update(cx, |o, cx| {
                ToolHost::request(o, ToolRequest::DeleteSelection, cx)
            });
        });
        rig.cx.run_until_parked();
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Placing);
        rig.cx.simulate_keystrokes("escape");
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle);

        // Wiring
        rig.select(&[R1]);
        rig.cx.simulate_keystrokes("w");
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Wiring);
        rig.cx.update(|_, cx| {
            overlay.update(cx, |o, cx| {
                ToolHost::request(o, ToolRequest::DeleteSelection, cx)
            });
        });
        rig.cx.run_until_parked();
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Wiring);
        rig.cx.simulate_keystrokes("escape");
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle);
        assert!(rig.history.borrow().is_empty());
    }

    /// W3.5 review finding B2: Delete (key, overlay, canvas, or direct request) is ignored
    /// during a live pointer rubber-band drag so no edit commits mid-gesture (INV5).
    #[gpui::test]
    fn w3_5_delete_is_ignored_during_rubber_band_drag(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, tc_del_01_doc(), true);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        rig.select(&[R1]);
        let rev = revision(&doc, rig.cx);

        down(&overlay, rig.cx, pt(-12, -10));
        drag_to(&overlay, rig.cx, pt(-10, -8));
        assert!(rig.cx.update(|_, cx| overlay.read(cx).is_rubber_banding()));
        assert_eq!(
            rig.cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );

        for path in [DelPath::Key, DelPath::Overlay, DelPath::Canvas] {
            rig.delete(path);
            assert_eq!(revision(&doc, rig.cx), rev, "{path:?} during rubber band");
            assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
            assert!(rig.cx.update(|_, cx| overlay.read(cx).is_rubber_banding()));
            assert_eq!(
                rig.cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
                vec![R1]
            );
        }

        rig.cx.update(|_, cx| {
            overlay.update(cx, |o, cx| {
                ToolHost::request(o, ToolRequest::DeleteSelection, cx)
            });
        });
        rig.cx.run_until_parked();
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert!(rig.cx.update(|_, cx| overlay.read(cx).is_rubber_banding()));
        assert_eq!(
            rig.cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![R1]
        );
        assert!(rig.history.borrow().is_empty());

        // Cancel the band cleanly, then verify idle Delete removes R1 and its runs.
        rig.cx.simulate_keystrokes("escape");
        up(&overlay, rig.cx, pt(-10, -8));
        assert!(!rig.cx.update(|_, cx| overlay.read(cx).is_rubber_banding()));
        assert_eq!(revision(&doc, rig.cx), rev);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));

        rig.delete(DelPath::Key);
        assert_eq!(revision(&doc, rig.cx), rev + 1);
        assert!(
            rig.cx
                .update(|_, cx| doc.read(cx).part(PartId(0)).is_none())
        );
        assert_eq!(
            live_wires(&rig.snapshot()),
            vec![(WireId(3), del_wire((8, 6), (12, 6)))]
        );
        assert_eq!(rig.history.borrow().len(), 1);
    }

    // ---- W3.3 Segment drag tool simulated-input GPUI tests (WD7, WD11, WINV3, WINV4, INV5, INV11) ----

    /// Z-shaped run between R1 at (-8, 0) (right pin (-6, 0)) and C1 at (8, 4) (left pin (6, 4)):
    /// - Wire(0): (-6, 0) -> (0, 0)
    /// - Wire(1): (0, 0) -> (0, 4)  (middle vertical segment)
    /// - Wire(2): (0, 4) -> (6, 4)
    ///
    /// Plus a foreign part R2 at (4, 2) with left pin at (2, 2) that blocks dragging Wire(1)
    /// right by +2G to x = 2.
    fn w3_3_segment_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let cap = lib.by_name("Capacitor-IEEE-NonPolarized").unwrap();
        let res_def = lib.get(res);
        let cap_def = lib.get(cap);

        let parts = [
            (
                Part {
                    symbol: res,
                    at: pt(-8, 0),
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                res_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                res_def.bounds,
            ),
            (
                Part {
                    symbol: cap,
                    at: pt(8, 4),
                    orient: Orient::IDENTITY,
                    designator: "C1".into(),
                    value: "100n".into(),
                },
                cap_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                cap_def.bounds,
            ),
            (
                Part {
                    symbol: res,
                    at: pt(4, 2),
                    orient: Orient::IDENTITY,
                    designator: "R2".into(),
                    value: "1k".into(),
                },
                res_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                res_def.bounds,
            ),
        ];
        let wires = [
            Wire {
                a: pt(-6, 0),
                b: pt(0, 0),
            },
            Wire {
                a: pt(0, 0),
                b: pt(0, 4),
            },
            Wire {
                a: pt(0, 4),
                b: pt(6, 4),
            },
        ];
        Doc::from_items(parts, wires, []).unwrap()
    }

    /// W3.3 done-when: dragging one selected wire segment starts `DraggingSegment` (not `Move`),
    /// previews via `plan_segment_drag`, commits one `Command::DragSegment` on release, preserves
    /// normal form and connectivity (`WINV3`), and round-trips exact ids via Ctrl+Z / Ctrl+Y
    /// (`WINV4`, `INV5`). Also confirms a multi-item selection containing a wire still starts a
    /// whole-item `Dragging` move (`TC-move-17`).
    #[gpui::test]
    fn w3_3_accepted_segment_drag_commits_once_and_round_trips_undo_redo(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, w3_3_segment_doc(), false);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        let middle = WireId(1);
        let rev0 = revision(&doc, rig.cx);

        // Select the single middle segment of the Z-run and drag it left by -2G (valid).
        rig.select(&[ItemId::Wire(middle)]);
        down(&overlay, rig.cx, pt(0, 2));
        assert!(
            matches!(
                rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
                ToolState::DraggingSegment(ref seg) if seg.wire == middle && !seg.is_lifted()
            ),
            "pressing one selected wire segment must start DraggingSegment"
        );
        // Dragging parallel along y stays at offset 0 and does not lift.
        drag_to(&overlay, rig.cx, pt(0, 3));
        assert!(matches!(
            rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::DraggingSegment(ref seg) if seg.offset() == 0 && !seg.is_lifted()
        ));

        // Dragging left by -2G lifts and matches `plan_segment_drag` exactly; revision unchanged.
        drag_to(&overlay, rig.cx, pt(-2, 3));
        assert_eq!(
            revision(&doc, rig.cx),
            rev0,
            "INV5: revision unchanged during segment drag"
        );
        let mut expected_plan = omnia_doc::route::SegmentDragPlan::default();
        rig.cx.update(|_, cx| {
            omnia_doc::route::plan_segment_drag(doc.read(cx), middle, -2 * G, &mut expected_plan)
                .unwrap();
        });
        match rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
            ToolState::DraggingSegment(seg) => {
                assert!(seg.is_lifted());
                assert!(!seg.is_refused());
                assert_eq!(seg.offset(), -2 * G);
                assert_eq!(seg.preview_wires(), expected_plan.add.as_slice());
            }
            other => panic!("expected DraggingSegment, got {other:?}"),
        }

        // Release commits one Command::DragSegment (one undo step, WINV3, WINV4).
        up(&overlay, rig.cx, pt(-2, 3));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle);
        assert!(!captured(rig.cx));
        assert_eq!(revision(&doc, rig.cx), rev0 + 1);
        assert_eq!(rig.history.borrow().len(), 1);
        let after = rig.snapshot();
        after.assert_normal_form();
        assert_eq!(
            after.wire(middle),
            Some(&Wire {
                a: pt(-2, 0),
                b: pt(-2, 4),
            })
        );
        assert_eq!(
            after.wire(WireId(0)),
            Some(&Wire {
                a: pt(-6, 0),
                b: pt(-2, 0),
            })
        );
        assert_eq!(
            after.wire(WireId(2)),
            Some(&Wire {
                a: pt(-2, 4),
                b: pt(6, 4),
            })
        );
        assert_inv10(
            &after,
            &[
                pt(-6, 0),
                pt(-2, 0),
                pt(-2, 4),
                pt(6, 4),
                pt(0, 0),
                pt(0, 4),
            ],
            &[WireId(0), WireId(1), WireId(2)],
        );

        // Ctrl+Z undoes in one step with exact ids; Ctrl+Y redoes identically (WINV4).
        rig.cx.simulate_keystrokes("ctrl-z");
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        rig.cx.simulate_keystrokes("ctrl-y");
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&after)));
        rig.cx.simulate_keystrokes("ctrl-z");
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));

        // Multi-item selection including a wire still starts whole-item Dragging (not DraggingSegment).
        rig.select(&[ItemId::Part(PartId(0)), ItemId::Wire(middle)]);
        down(&overlay, rig.cx, pt(0, 2));
        assert!(matches!(
            rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Dragging(ref drag) if drag.items.len() == 2
        ));
        rig.cx.simulate_keystrokes("escape");
        up(&overlay, rig.cx, pt(0, 2));
    }

    /// W3.3 done-when: dragging a segment across a foreign pin (TC-seg-03) shows the refusal and
    /// `Blocker::Pin`, leaves the document unchanged on mouse-up (`WINV4`, `INV5`), and releases
    /// pointer capture.
    #[gpui::test]
    fn w3_3_refused_segment_drag_shows_blocker_and_leaves_doc_unchanged_on_release(
        cx: &mut TestAppContext,
    ) {
        let mut rig = open_del_rig(cx, w3_3_segment_doc(), false);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        let middle = WireId(1);
        let rev0 = revision(&doc, rig.cx);

        rig.select(&[ItemId::Wire(middle)]);
        down(&overlay, rig.cx, pt(0, 2));
        // Drag right by +2G to x = 2, which runs straight across R2's left pin at (2, 2).
        drag_to(&overlay, rig.cx, pt(2, 2));
        match rig.cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
            ToolState::DraggingSegment(seg) => {
                assert!(seg.is_lifted());
                assert!(seg.is_refused());
                assert_eq!(
                    seg.blocker(),
                    Some(omnia_doc::route::Blocker::Pin {
                        part: PartId(2),
                        pin: 0,
                    })
                );
                assert!(
                    !seg.preview_wires().is_empty(),
                    "refused preview must still carry attempted wires for mark/conflict rendering"
                );
            }
            other => panic!("expected refused DraggingSegment, got {other:?}"),
        }
        assert_eq!(revision(&doc, rig.cx), rev0);

        // Mouse-up on a refused drag cancels cleanly like Esc (WD7, WD11, WINV4).
        up(&overlay, rig.cx, pt(2, 2));
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle);
        assert!(!captured(rig.cx));
        assert_eq!(revision(&doc, rig.cx), rev0);
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        assert!(rig.history.borrow().is_empty());
        assert!(rig.cx.update(|_, cx| {
            overlay
                .read(cx)
                .sheet_entity()
                .read(cx)
                .hidden_items()
                .is_empty()
        }));
    }

    /// W3.3 done-when: Esc and every C4 gesture-end path (tool switch, lost capture, window
    /// deactivation) cancel a segment drag with unchanged revision, cleared hidden items, and
    /// released pointer capture (`INV5`, `C4`).
    #[gpui::test]
    fn w3_3_segment_drag_esc_and_c4_end_paths_cancel_cleanly(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, w3_3_segment_doc(), false);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        let rev0 = revision(&doc, rig.cx);

        let assert_cancelled = |rig: &mut DelRig<'_>, label: &str| {
            assert_eq!(phase(&overlay, rig.cx), ToolPhase::Idle, "{label}: Idle");
            assert!(!captured(rig.cx), "{label}: capture released");
            assert_eq!(revision(&doc, rig.cx), rev0, "{label}: revision unchanged");
            assert!(
                rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)),
                "{label}: doc content unchanged"
            );
            assert!(
                rig.cx.update(|_, cx| {
                    overlay
                        .read(cx)
                        .sheet_entity()
                        .read(cx)
                        .hidden_items()
                        .is_empty()
                }),
                "{label}: hidden items cleared"
            );
            assert!(rig.history.borrow().is_empty(), "{label}: no history event");
        };

        // 1. Esc cancels mid-drag (valid offset).
        down(&overlay, rig.cx, pt(0, 2));
        drag_to(&overlay, rig.cx, pt(-2, 2));
        assert!(captured(rig.cx));
        rig.cx.simulate_keystrokes("escape");
        assert_cancelled(&mut rig, "Esc");
        up(&overlay, rig.cx, pt(-2, 2));
        assert_cancelled(&mut rig, "release after Esc");

        // 2. Switching tools (W) cancels mid-drag and releases capture.
        down(&overlay, rig.cx, pt(0, 2));
        drag_to(&overlay, rig.cx, pt(-1, 2));
        rig.cx.simulate_keystrokes("w");
        assert_eq!(phase(&overlay, rig.cx), ToolPhase::Wiring);
        assert!(!captured(rig.cx));
        assert_eq!(revision(&doc, rig.cx), rev0);
        rig.cx.simulate_keystrokes("escape");
        up(&overlay, rig.cx, pt(-1, 2));
        assert_cancelled(&mut rig, "tool switch");

        // 3. Lost release (mouse move with pressed_button = None) cancels cleanly.
        down(&overlay, rig.cx, pt(0, 2));
        drag_to(&overlay, rig.cx, pt(-1, 2));
        let p = screen(&overlay, rig.cx, pt(-2, 2));
        rig.cx.simulate_mouse_move(p, None, Modifiers::none());
        rig.cx.run_until_parked();
        assert_cancelled(&mut rig, "lost release");

        // 4. Window deactivation cancels cleanly (C4).
        down(&overlay, rig.cx, pt(0, 2));
        drag_to(&overlay, rig.cx, pt(-1, 2));
        assert!(captured(rig.cx));
        rig.cx.deactivate_window();
        rig.cx.run_until_parked();
        assert_cancelled(&mut rig, "window deactivation");
    }

    /// W3.3 done-when: `SheetView` paints **exactly twice** per segment drag (1 on lift-off hiding
    /// the dragged segment and its corner neighbours, 0 across all drag moves including accepted,
    /// zero-offset, shrink-to-zero, and refused transitions, and 1 on end/commit/refusal/cancel),
    /// and 0 times on a plain click (`INV11`, `D7`).
    #[gpui::test]
    fn w3_3_sheet_view_paints_exactly_twice_per_segment_drag(cx: &mut TestAppContext) {
        let mut rig = open_del_rig(cx, w3_3_segment_doc(), false);
        let original = rig.snapshot();
        let (overlay, doc) = (rig.overlay.clone(), rig.doc.clone());
        let middle = WireId(1);
        let paints =
            |cx: &mut VisualTestContext| cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let hidden = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                overlay
                    .read(cx)
                    .sheet_entity()
                    .read(cx)
                    .hidden_items()
                    .to_vec()
            })
        };

        // 0. Plain click on wire segment -> 0 SheetView paints.
        let p_click = paints(rig.cx);
        down(&overlay, rig.cx, pt(0, 2));
        up(&overlay, rig.cx, pt(0, 2));
        assert_eq!(paints(rig.cx), p_click, "INV11: wire click causes 0 paints");

        // 1. Accepted segment drag through valid, refused, zero, and shrink-to-zero steps -> 2 paints.
        let p0 = paints(rig.cx);
        down(&overlay, rig.cx, pt(0, 2));
        assert_eq!(paints(rig.cx), p0, "press before lift causes 0 paints");
        assert!(hidden(rig.cx).is_empty());

        drag_to(&overlay, rig.cx, pt(-1, 2));
        assert_eq!(paints(rig.cx), p0 + 1, "lift paints SheetView once");
        assert_eq!(
            hidden(rig.cx),
            vec![
                ItemId::Wire(WireId(0)),
                ItemId::Wire(WireId(1)),
                ItemId::Wire(WireId(2)),
            ],
            "dragged segment and both stretching corner neighbours hidden on lift"
        );

        // Sweep across valid (-2, -3, -6 shrink-to-zero), back to 0, into refused (+2), and back to -2.
        for x in [-2, -3, -6, 0, 1, 2, 3, -1, -2] {
            drag_to(&overlay, rig.cx, pt(x, 2));
            assert_eq!(
                paints(rig.cx),
                p0 + 1,
                "INV11: 0 SheetView paints during drag move to x={x}"
            );
        }

        up(&overlay, rig.cx, pt(-2, 2));
        assert_eq!(
            paints(rig.cx),
            p0 + 2,
            "W3.3 done-when: SheetView painted exactly twice for accepted segment drag"
        );
        assert!(hidden(rig.cx).is_empty());

        // Restore original doc for refused-release and cancel checks.
        rig.cx.simulate_keystrokes("ctrl-z");
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));
        let hover_pt = screen(&overlay, rig.cx, pt(0, -8));
        rig.cx
            .simulate_mouse_move(hover_pt, None, Modifiers::none());
        rig.cx.run_until_parked();

        // 2. Refused drag release -> exactly 2 paints.
        let p1 = paints(rig.cx);
        down(&overlay, rig.cx, pt(0, 2));
        assert_eq!(paints(rig.cx), p1);
        drag_to(&overlay, rig.cx, pt(2, 2));
        assert_eq!(paints(rig.cx), p1 + 1);
        up(&overlay, rig.cx, pt(2, 2));
        assert_eq!(
            paints(rig.cx),
            p1 + 2,
            "W3.3 done-when: refused segment drag paints SheetView exactly twice"
        );
        assert!(hidden(rig.cx).is_empty());
        assert!(rig.cx.update(|_, cx| doc.read(cx).content_eq(&original)));

        // 3. Cancelled drag (via overlay.cancel, no keyboard modality switch) -> exactly 2 paints.
        let p2 = paints(rig.cx);
        down(&overlay, rig.cx, pt(0, 2));
        assert_eq!(paints(rig.cx), p2);
        drag_to(&overlay, rig.cx, pt(-1, 2));
        assert_eq!(paints(rig.cx), p2 + 1);
        for x in [-2, -3, 2, -1] {
            drag_to(&overlay, rig.cx, pt(x, 2));
            assert_eq!(paints(rig.cx), p2 + 1);
        }
        assert!(
            rig.cx
                .update(|window, cx| overlay.update(cx, |o, cx| o.cancel(window, cx)))
        );
        rig.cx.run_until_parked();
        assert_eq!(
            paints(rig.cx),
            p2 + 2,
            "W3.3 done-when: cancelled segment drag paints SheetView exactly twice"
        );
        assert!(hidden(rig.cx).is_empty());
        assert_eq!(middle, WireId(1));
    }
}
