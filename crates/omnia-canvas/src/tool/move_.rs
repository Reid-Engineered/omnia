//! T5.3 / W2.3: move, rotate, mirror and delete command planning, plus live follow preview,
//! modifier handling, and refusal state for drags (`D2`, `K2`, `INV5`, `INV11`, `WD3`, `WD10`,
//! `WD11`, `WD12`, `WK3`, `WK7`, `WINV5`, `WINV11`, `T5.5`).
//!
//! Pure geometry, follow-preview caching, and command builders used by [`crate::tool::ToolMachine`]
//! and [`crate::OverlayView`]:
//! - Anchor-pin grid snapping for dragged parts (K2), keeping zero-movement clicks as clicks.
//! - Deterministic, lattice-safe pivot selection for in-place and mid-drag reorientation.
//! - Fixed lift-off hidden sets (`items ∪ affected_wires`) up to [`FOLLOW_PREVIEW_CAP`] so
//!   `SheetView` paints strictly twice per drag (`WINV11`), while `plan_follow` runs only when
//!   `(delta, by, alt)` changes (`WD11`).
//! - Single-command commits via `Command::Move`, `Command::Transform` (T5.5), `Command::Reorient`,
//!   and `Command::Delete` so one undo reverts the whole gesture (INV5).

use crate::{
    camera_anim::selection_bounds,
    selection::{item_exists, item_sort_key},
    tool::DocStamp,
};
use gpui::SharedString;
use omnia_doc::{
    Anchor, Boundary, Command, CommandError, Doc, FollowError, FollowPlan, GRID_NM, Isometry,
    ItemId, LabelId, Orient, Pt, Rect, SHEET_LIMIT_NM, Wire, WireId, plan_follow,
    prefs::{FollowPolicy, WiringPrefs},
    route::Blocker,
    snap_to,
};

/// Largest multiple of [`GRID_NM`] that lies within K1 (`±SHEET_LIMIT_NM`).
const MAX_GRID_NM: i64 = (SHEET_LIMIT_NM / GRID_NM) as i64 * GRID_NM as i64;

/// Applies `orient` to an `i64` offset `(dx, dy)` without risking `i32` overflow.
#[inline]
fn apply_orient_i64(orient: Orient, dx: i64, dy: i64) -> (i64, i64) {
    match orient.to_raw() {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        3 => (dy, -dx),
        4 => (-dx, dy),
        5 => (-dy, -dx),
        6 => (dx, -dy),
        _ => (dy, dx),
    }
}

/// Deterministic, lattice-safe pivot for reorienting `items` (in place or during a drag).
///
/// - A single live `Part` pivots around `part.at` (centre for standard symbols, pin for ground),
///   preserving half-grid pin parity for symbols like `IC-COM-OpAmp`.
/// - A single live `Label` pivots around `label.at`.
/// - A single live `Wire` or multi-item selection pivots around the grid-snapped centre of the
///   live selection bounds (always on `GRID_NM` ⊂ `PIN_LATTICE_NM`, avoiding `OffLattice`).
pub fn selection_pivot(doc: &Doc, items: &[ItemId]) -> Option<Pt> {
    let mut single: Option<ItemId> = None;
    let mut count = 0usize;
    for &item in items {
        if item_exists(doc, item) {
            count += 1;
            if count == 1 {
                single = Some(item);
            } else {
                break;
            }
        }
    }
    if count == 0 {
        return None;
    }
    if count == 1 {
        match single.expect("count == 1") {
            ItemId::Part(id) => return doc.part(id).map(|part| part.at),
            ItemId::Label(id) => return doc.label(id).map(|label| label.at),
            ItemId::Wire(id) => {
                let wire = doc.wire(id)?;
                let rect = Rect {
                    min: Pt {
                        x: wire.a.x.min(wire.b.x),
                        y: wire.a.y.min(wire.b.y),
                    },
                    max: Pt {
                        x: wire.a.x.max(wire.b.x),
                        y: wire.a.y.max(wire.b.y),
                    },
                };
                return Some(snap_to(rect.center(), GRID_NM));
            }
        }
    }
    selection_bounds(doc, items).map(|bounds| snap_to(bounds.center(), GRID_NM))
}

/// Picks the world-space anchor pin used to snap a drag to [`GRID_NM`] (K2).
///
/// Uses pin 0 of `pressed` when it is a live `Part`; otherwise pin 0 of the first live `Part` in
/// canonical `items` order; otherwise `None` (wire/label-only drags snap their raw delta to
/// [`GRID_NM`]).
pub fn drag_anchor_pin(doc: &Doc, pressed: ItemId, items: &[ItemId]) -> Option<Pt> {
    if let ItemId::Part(id) = pressed
        && let Some(pin) = doc.pins(id).and_then(|pins| pins.first().copied())
    {
        return Some(pin);
    }
    for &item in items {
        if let ItemId::Part(id) = item
            && let Some(pin) = doc.pins(id).and_then(|pins| pins.first().copied())
        {
            return Some(pin);
        }
    }
    None
}

/// Computes the snapped translation delta for a drag from `anchor` to `current` with mid-drag
/// reorientation `by` about `pivot` (K2).
///
/// When `by == Orient::IDENTITY` and the raw pointer movement `current - anchor` snaps to
/// [`Pt::ZERO`] (less than half a grid step on both axes), returns [`Pt::ZERO`] so clicking an
/// off-grid part with slight pointer jitter remains a click rather than jumping onto the grid.
///
/// Otherwise, if `anchor_pin` is `Some(p0)`, transforms `p0` by `by` around `pivot`, adds
/// `current - anchor`, snaps that world pin position onto [`GRID_NM`], and returns the exact
/// delta from the rotated pin to the snapped grid point so the anchor pin lands on [`GRID_NM`].
pub fn drag_delta(anchor: Pt, current: Pt, pivot: Pt, by: Orient, anchor_pin: Option<Pt>) -> Pt {
    let raw_dx = i64::from(current.x) - i64::from(anchor.x);
    let raw_dy = i64::from(current.y) - i64::from(anchor.y);
    if by == Orient::IDENTITY
        && let (Ok(dx), Ok(dy)) = (i32::try_from(raw_dx), i32::try_from(raw_dy))
        && snap_to(Pt { x: dx, y: dy }, GRID_NM) == Pt::ZERO
    {
        return Pt::ZERO;
    }
    if let Some(p0) = anchor_pin {
        let dx = i64::from(p0.x) - i64::from(pivot.x);
        let dy = i64::from(p0.y) - i64::from(pivot.y);
        let (rx, ry) = apply_orient_i64(by, dx, dy);
        let p_rot_x = i64::from(pivot.x) + rx;
        let p_rot_y = i64::from(pivot.y) + ry;
        let target = Pt {
            x: (p_rot_x + raw_dx).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
            y: (p_rot_y + raw_dy).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
        };
        let snapped = snap_to(target, GRID_NM);
        let del_x = i64::from(snapped.x) - p_rot_x;
        let del_y = i64::from(snapped.y) - p_rot_y;
        Pt {
            x: del_x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: del_y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    } else {
        let raw = Pt {
            x: i32::try_from(raw_dx).expect("K1 keeps a drag delta inside i32"),
            y: i32::try_from(raw_dy).expect("K1 keeps a drag delta inside i32"),
        };
        snap_to(raw, GRID_NM)
    }
}

/// The [`Isometry`] describing a drag preview and its matching commit (`Command::Move` or
/// `Command::Transform`).
#[inline]
pub fn drag_isometry(delta: Pt, pivot: Pt, by: Orient) -> Isometry {
    Isometry::Transform { delta, pivot, by }
}

/// Builds the single commit command for a drag (INV5, T5.5), or `None` when nothing changed.
///
/// - `by == Orient::IDENTITY && delta == Pt::ZERO`: `None` (a click, no edit).
/// - `by == Orient::IDENTITY`: `Command::Move`.
/// - `by != Orient::IDENTITY`: `Command::Transform` (even when `delta == Pt::ZERO`), planned and
///   normalised once against the original sheet so one undo reverts the whole gesture.
pub fn drag_command(
    items: &[ItemId],
    delta: Pt,
    pivot: Pt,
    by: Orient,
    follow: FollowPolicy,
) -> Option<Command> {
    if items.is_empty() || (by == Orient::IDENTITY && delta == Pt::ZERO) {
        return None;
    }
    if by == Orient::IDENTITY {
        Some(Command::Move {
            items: items.to_vec(),
            delta,
            follow,
        })
    } else {
        Some(Command::Transform {
            items: items.to_vec(),
            delta,
            pivot,
            by,
            follow,
        })
    }
}

/// WK7: live follow previews plan and draw up to this many affected runs. Above this cap the
/// overlay draws dragged ghosts only, and planning happens once at commit (`WD11`, `WK7`).
pub const FOLLOW_PREVIEW_CAP: usize = 256;

/// Live follow-preview verdict for an active [`DragGesture`] (`WD11`, `WK7`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowPreview {
    /// No affected runs touch the dragged selection (`affected_wires` is empty), or preview was
    /// cleared back to identity before re-planning.
    None,
    /// `plan_follow` accepted the current `(delta, by, alt)` (or the drag is at identity).
    Accepted,
    /// `plan_follow` refused the current `(delta, by, alt)` (`WD11`, `TC-move-13`).
    Refused(FollowError),
    /// Affected runs exceed [`FOLLOW_PREVIEW_CAP`]; live planning is skipped until commit (`WK7`).
    OverCap { run_count: usize },
}

/// Operation verb used to prefix commit-time and drag-time command refusal notices (`UD11`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefusalVerb {
    Move,
    Reorient,
}

impl RefusalVerb {
    fn prefix(self) -> &'static str {
        match self {
            Self::Move => "Move refused",
            Self::Reorient => "Reorient refused",
        }
    }
}

/// Formats a user-facing status notice for a [`FollowError`] under `verb` (`WD11`, `UD11`).
pub fn format_follow_refusal_for(verb: RefusalVerb, error: FollowError) -> SharedString {
    let prefix = verb.prefix();
    let suffix = match verb {
        RefusalVerb::Move => "; hold Alt to detach",
        RefusalVerb::Reorient => "",
    };
    match error {
        FollowError::NoRoute { blocker, .. } => match blocker {
            Blocker::Pin { .. } => {
                format!("{prefix}: no valid wire route (blocked by pin){suffix}").into()
            }
            Blocker::WireEnd(_) | Blocker::WireBody(_) | Blocker::Overlap(_) => {
                format!("{prefix}: no valid wire route (blocked by wire){suffix}").into()
            }
            Blocker::Sheet => {
                format!("{prefix}: route blocked at sheet limit or off lattice{suffix}").into()
            }
        },
    }
}

/// Formats a user-facing status notice for a live or commit-time move [`FollowError`] (`WD11`, `UD11`).
pub fn format_follow_refusal(error: FollowError) -> SharedString {
    format_follow_refusal_for(RefusalVerb::Move, error)
}

/// Formats a user-facing status notice when a move, transform, or reorient commit is refused by
/// [`Doc::apply`] (`WD11`, `WK7`, `UD11`).
pub fn format_command_refusal(verb: RefusalVerb, error: CommandError) -> SharedString {
    let prefix = verb.prefix();
    match error {
        CommandError::Follow(err) => format_follow_refusal_for(verb, err),
        CommandError::Capture { .. } => {
            format!("{prefix}: moved wire would connect across a foreign pin or wire").into()
        }
        CommandError::OffLattice(_) => {
            format!("{prefix}: moved geometry would land off the wire lattice").into()
        }
        CommandError::OutOfBounds => {
            format!("{prefix}: item or pin lies outside sheet limits").into()
        }
        CommandError::Segment(_) => "Segment drag refused: blocked by obstacle".into(),
        _ => error.to_string().into(),
    }
}

#[inline]
fn canonical_wire(w: Wire) -> Wire {
    if (w.a.x, w.a.y) <= (w.b.x, w.b.y) {
        w
    } else {
        Wire { a: w.b, b: w.a }
    }
}

/// Heap-boxed follow-preview caches owned by [`DragGesture`] so `ToolState::Dragging` stays compact
/// (`clippy::large_enum_variant`) while drag motion and painting allocate nothing warm (`B10`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct DragFollowCache {
    affected_wires: Vec<WireId>,
    /// Corner labels on rigid-eligible runs only (`Anchor::Boundary` at both ends), so labels on
    /// rerouted/detached non-rigid runs stay visible with their sheet text (`WINV11`, W2.1).
    affected_labels: Vec<LabelId>,
    hidden: Vec<ItemId>,
    plan: FollowPlan,
    plan_remove_sorted: Vec<WireId>,
    plan_labels_sorted: Vec<(LabelId, Pt)>,
    preview_junctions: Vec<Pt>,
    junction_candidates: Vec<Pt>,
    query_scratch: Vec<ItemId>,
}

/// A drag of the selection, held as preview state until release (`INV5`, `WD11`, `WINV5`, `WINV11`).
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
    /// Snapshot of wiring preferences captured at drag start so mid-drag preference edits apply to
    /// the next gesture rather than mutating a live preview (`WD12`, `WINV5`).
    prefs: WiringPrefs,
    /// Latest modifier state (`Alt`) tracked for live preview inversion (`WK3`, `WD11`).
    alt: bool,
    /// True once the drag has actually moved or reoriented off its starting position (T5.3, D7,
    /// INV11). Plain selection clicks never lift, keeping click-selection SheetView paints at 0.
    lifted: bool,
    /// `true` for static golden/programmatic previews that do not hold pointer capture.
    preview_only: bool,
    /// Cached affected-run sets, fixed lift-off hidden list, reusable planner output, and junction
    /// buffers (`WD11`, `WINV11`, `B10`).
    cache: Box<DragFollowCache>,
    /// Live follow-preview verdict.
    preview: FollowPreview,
    /// Cache key `(delta, by, alt)` for the last `plan_follow` invocation (`WD11`).
    last_plan_key: Option<(Pt, Orient, bool)>,
    /// Count of actual `omnia_doc::plan_follow` calls performed during this drag (`WD11`).
    plan_calls: u64,
}

impl DragGesture {
    /// Starts a drag gesture on `items` against `doc`, computing the boundary, affected runs, and
    /// fixed lift-off hidden set once (`WD11`, `WK7`, `WINV11`).
    pub fn start(
        doc: &Doc,
        items: Vec<ItemId>,
        pressed: ItemId,
        anchor: Pt,
        base: DocStamp,
        prefs: WiringPrefs,
        alt: bool,
    ) -> Self {
        let pivot = selection_pivot(doc, &items).unwrap_or(anchor);
        let anchor_pin = drag_anchor_pin(doc, pressed, &items);

        let mut boundary = Boundary::default();
        doc.boundary_points(&items, &mut boundary);
        let mut runs = Vec::new();
        doc.affected_runs(&boundary, &mut runs);

        let mut hidden = items.clone();
        hidden.sort_unstable_by_key(|&item| item_sort_key(item));
        hidden.dedup();

        let (affected_wires, affected_labels, preview) = if runs.len() > FOLLOW_PREVIEW_CAP {
            (
                Vec::new(),
                Vec::new(),
                FollowPreview::OverCap {
                    run_count: runs.len(),
                },
            )
        } else {
            let mut wires: Vec<WireId> = runs
                .iter()
                .flat_map(|run| run.wires.iter().copied())
                .collect();
            wires.sort_unstable_by_key(|id| id.0);
            wires.dedup();
            // Only runs with both ends on the moving boundary can translate rigidly and carry
            // corner labels (WD3); leaving non-rigid run labels unhidden preserves their sheet text.
            let mut labels: Vec<LabelId> = runs
                .iter()
                .filter(|run| run.a.1 == Anchor::Boundary && run.b.1 == Anchor::Boundary)
                .flat_map(|run| run.labels.iter().copied())
                .filter(|id| !items.contains(&ItemId::Label(*id)))
                .collect();
            labels.sort_unstable_by_key(|id| id.0);
            labels.dedup();
            hidden.extend(wires.iter().map(|&id| ItemId::Wire(id)));
            hidden.extend(labels.iter().map(|&id| ItemId::Label(id)));
            hidden.sort_unstable_by_key(|&item| item_sort_key(item));
            hidden.dedup();
            let verdict = if wires.is_empty() {
                FollowPreview::None
            } else {
                FollowPreview::Accepted
            };
            (wires, labels, verdict)
        };

        let remove_cap = affected_wires.len();
        let labels_cap = affected_labels.len();
        let mut gesture = Self {
            items,
            pressed,
            anchor,
            current: anchor,
            pivot,
            by: Orient::IDENTITY,
            base,
            anchor_pin,
            prefs,
            alt,
            lifted: false,
            preview_only: false,
            cache: Box::new(DragFollowCache {
                affected_wires,
                affected_labels,
                hidden,
                plan: FollowPlan::default(),
                plan_remove_sorted: Vec::with_capacity(remove_cap),
                plan_labels_sorted: Vec::with_capacity(labels_cap),
                preview_junctions: Vec::new(),
                junction_candidates: Vec::new(),
                query_scratch: Vec::new(),
            }),
            preview,
            last_plan_key: None,
            plan_calls: 0,
        };
        if !gesture.cache.affected_wires.is_empty() {
            gesture.recompute_preview_junctions(doc);
        }
        gesture
    }

    /// Creates a lifted programmatic drag preview without pointer capture (used for golden rendering
    /// and programmatic inspection).
    pub fn for_preview(
        doc: &Doc,
        items: Vec<ItemId>,
        delta: Pt,
        by: Orient,
        base: DocStamp,
        prefs: WiringPrefs,
        alt: bool,
    ) -> Option<Self> {
        let mut live_items: Vec<ItemId> = items
            .into_iter()
            .filter(|&item| item_exists(doc, item))
            .collect();
        if live_items.is_empty() {
            return None;
        }
        live_items.sort_unstable_by_key(|&item| item_sort_key(item));
        live_items.dedup();
        let pressed = live_items[0];
        let anchor = selection_pivot(doc, &live_items)?;
        let mut gesture = Self::start(doc, live_items, pressed, anchor, base, prefs, alt);
        gesture.preview_only = true;
        // Preserve exact grid-multiple `delta` even when the pressed part has half-grid pins.
        gesture.anchor_pin = None;
        gesture.current = Pt {
            x: anchor.x.saturating_add(delta.x),
            y: anchor.y.saturating_add(delta.y),
        };
        gesture.by = by;
        gesture.lifted = delta != Pt::ZERO || by != Orient::IDENTITY;
        gesture.update_preview(Some(doc));
        Some(gesture)
    }

    /// Whether this gesture is an active pointer drag (holds capture), as opposed to a static
    /// programmatic preview.
    pub fn is_pointer_drag(&self) -> bool {
        !self.preview_only
    }

    /// Whether the drag has visually lifted off the sheet (hidden on `SheetView`, drawn as ghost).
    pub fn is_lifted(&self) -> bool {
        self.lifted
    }

    /// Marks the drag as lifted off the sheet.
    pub(crate) fn set_lifted(&mut self) {
        self.lifted = true;
    }

    /// Current `Alt` modifier state tracked by this preview.
    pub fn alt(&self) -> bool {
        self.alt
    }

    /// Updates the stored `Alt` modifier flag without re-planning.
    pub(crate) fn set_alt_flag(&mut self, alt: bool) {
        self.alt = alt;
    }

    /// Current follow-preview verdict (`None`, `Accepted`, `Refused`, or `OverCap`).
    pub fn preview(&self) -> FollowPreview {
        self.preview
    }

    /// Follow refusal error when [`Self::preview`] is [`FollowPreview::Refused`].
    pub fn refusal(&self) -> Option<FollowError> {
        match self.preview {
            FollowPreview::Refused(err) => Some(err),
            FollowPreview::None | FollowPreview::Accepted | FollowPreview::OverCap { .. } => None,
        }
    }

    /// The blocking item when [`Self::preview`] is [`FollowPreview::Refused`] (`WD11`).
    pub fn blocker(&self) -> Option<Blocker> {
        self.refusal()
            .map(|FollowError::NoRoute { blocker, .. }| blocker)
    }

    /// Returns `true` when the live follow preview is currently refused (`WD11`).
    pub fn is_refused(&self) -> bool {
        matches!(self.preview, FollowPreview::Refused(_))
    }

    /// Number of times [`omnia_doc::plan_follow`] ran during this drag (`WD11`).
    pub fn plan_calls(&self) -> u64 {
        self.plan_calls
    }

    /// Canonically sorted items hidden from `SheetView`: empty before lift-off, then fixed for the
    /// remainder of the drag (`D7`, `INV11`, `WINV11`).
    pub fn hidden_items(&self) -> &[ItemId] {
        if self.lifted { &self.cache.hidden } else { &[] }
    }

    /// Sorted wire ids belonging to affected runs (empty when unwired or over [`FOLLOW_PREVIEW_CAP`]).
    pub fn affected_wires(&self) -> &[WireId] {
        &self.cache.affected_wires
    }

    /// Sorted non-selected corner label ids belonging to rigid-eligible runs (both ends
    /// `Anchor::Boundary`; empty when over [`FOLLOW_PREVIEW_CAP`]). Labels on non-rigid runs stay
    /// in the sheet with their text (`WINV11`, W2.1).
    pub fn affected_labels(&self) -> &[LabelId] {
        &self.cache.affected_labels
    }

    /// Latest follow plan computed for this drag.
    pub fn plan(&self) -> &FollowPlan {
        &self.cache.plan
    }

    /// New follow wire segments produced by the latest accepted plan.
    pub fn plan_add(&self) -> &[Wire] {
        &self.cache.plan.add
    }

    /// Sorted replaced wire ids (`WireId.0`) for zero-alloc lookup during overlay paint (`B10`).
    pub fn plan_remove_sorted(&self) -> &[WireId] {
        &self.cache.plan_remove_sorted
    }

    /// Sorted `(LabelId, Pt)` pairs from `plan.labels` (`LabelId.0`) for zero-alloc lookup during
    /// overlay paint (`B10`, `WINV5`).
    pub fn plan_labels_sorted(&self) -> &[(LabelId, Pt)] {
        &self.cache.plan_labels_sorted
    }

    /// Precomputed K6 junction dot coordinates hidden by `SheetView` that must appear in the
    /// overlay (`K6`, `B10`).
    pub fn preview_junctions(&self) -> &[Pt] {
        &self.cache.preview_junctions
    }

    /// Follow policy for this gesture given `alt` at commit or modifier change (`WD12`, `WK3`).
    pub fn follow_policy(&self, alt: bool) -> FollowPolicy {
        self.prefs.follow_policy(alt)
    }

    /// The pointer movement, snapped so a dragged part's anchor pin lands on `GRID_NM` (K2),
    /// or snapped directly to `GRID_NM` for wire/label-only drags.
    pub fn delta(&self) -> Pt {
        drag_delta(
            self.anchor,
            self.current,
            self.pivot,
            self.by,
            self.anchor_pin,
        )
    }

    /// The live preview and commit [`Isometry`] for this drag (T5.3, T5.5).
    pub fn isometry(&self) -> Isometry {
        drag_isometry(self.delta(), self.pivot, self.by)
    }

    /// The one command this drag commits: `Command::Move`, `Command::Transform` (when R, X or Y
    /// was applied mid-drag, T5.5), or `None` for a click that changed nothing (INV5).
    pub fn command(&self, follow: FollowPolicy) -> Option<Command> {
        drag_command(&self.items, self.delta(), self.pivot, self.by, follow)
    }

    /// Updates `alt` and refreshes the live follow preview if the modifier changed (`WD11`, `WK3`).
    pub fn set_alt(&mut self, doc: Option<&Doc>, alt: bool) -> bool {
        if self.alt == alt {
            return false;
        }
        self.alt = alt;
        self.update_preview(doc)
    }

    /// Updates the follow plan only when `(delta, by, alt)` changes (`WD11`).
    ///
    /// At identity (`delta == Pt::ZERO && by == Orient::IDENTITY`), skips `plan_follow`, clears any
    /// previous plan, and restores `Accepted` (or `None` when unwired) while keeping `OverCap`
    /// intact. Returns `true` when visible preview state changed.
    pub fn update_preview(&mut self, doc: Option<&Doc>) -> bool {
        if matches!(self.preview, FollowPreview::OverCap { .. }) {
            return false;
        }
        let delta = self.delta();
        if delta == Pt::ZERO && self.by == Orient::IDENTITY {
            if self.last_plan_key.is_some() {
                self.last_plan_key = None;
                self.clear_plan_buffers();
                self.preview = if self.cache.affected_wires.is_empty() {
                    FollowPreview::None
                } else {
                    FollowPreview::Accepted
                };
                if let Some(doc) = doc
                    && !self.cache.affected_wires.is_empty()
                {
                    self.recompute_preview_junctions(doc);
                } else {
                    self.cache.preview_junctions.clear();
                }
                return true;
            }
            return false;
        }
        let key = (delta, self.by, self.alt);
        if self.cache.affected_wires.is_empty() {
            self.last_plan_key = Some(key);
            self.preview = FollowPreview::None;
            return false;
        }
        if self.last_plan_key == Some(key) {
            return false;
        }
        let Some(doc) = doc else {
            return false;
        };
        self.last_plan_key = Some(key);
        self.plan_calls += 1;
        let policy = self.prefs.follow_policy(self.alt);
        match plan_follow(
            doc,
            &self.items,
            self.isometry(),
            policy,
            &mut self.cache.plan,
        ) {
            Ok(()) => {
                self.cache.plan_remove_sorted.clear();
                self.cache
                    .plan_remove_sorted
                    .extend_from_slice(&self.cache.plan.remove);
                self.cache
                    .plan_remove_sorted
                    .sort_unstable_by_key(|id| id.0);
                self.cache.plan_labels_sorted.clear();
                self.cache
                    .plan_labels_sorted
                    .extend_from_slice(&self.cache.plan.labels);
                self.cache
                    .plan_labels_sorted
                    .sort_unstable_by_key(|(id, _)| id.0);
                self.preview = FollowPreview::Accepted;
            }
            Err(err) => {
                self.clear_plan_buffers();
                self.preview = FollowPreview::Refused(err);
            }
        }
        self.recompute_preview_junctions(doc);
        true
    }

    fn clear_plan_buffers(&mut self) {
        self.cache.plan.remove.clear();
        self.cache.plan.add.clear();
        self.cache.plan.labels.clear();
        self.cache.plan.fates.clear();
        self.cache.plan_remove_sorted.clear();
        self.cache.plan_labels_sorted.clear();
    }

    fn recompute_preview_junctions(&mut self, doc: &Doc) {
        self.cache.junction_candidates.clear();
        self.cache.preview_junctions.clear();
        let refused = self.is_refused();
        let iso = self.isometry();

        for &id in &self.cache.affected_wires {
            if !refused
                && self
                    .cache
                    .plan_remove_sorted
                    .binary_search_by_key(&id.0, |w| w.0)
                    .is_ok()
            {
                continue;
            }
            if let Some(&wire) = doc.wire(id) {
                self.cache.junction_candidates.push(wire.a);
                self.cache.junction_candidates.push(wire.b);
            }
        }
        if !refused {
            for &wire in &self.cache.plan.add {
                self.cache.junction_candidates.push(wire.a);
                self.cache.junction_candidates.push(wire.b);
            }
            for &item in &self.items {
                match item {
                    ItemId::Part(id) => {
                        if let Some(pins) = doc.pins(id) {
                            for &pin in pins {
                                if let Ok(moved) = iso.apply(pin) {
                                    self.cache.junction_candidates.push(moved);
                                }
                            }
                        }
                    }
                    ItemId::Wire(id) => {
                        if let Some(&wire) = doc.wire(id) {
                            if let Ok(a) = iso.apply(wire.a) {
                                self.cache.junction_candidates.push(a);
                            }
                            if let Ok(b) = iso.apply(wire.b) {
                                self.cache.junction_candidates.push(b);
                            }
                        }
                    }
                    ItemId::Label(_) => {}
                }
            }
        }

        self.cache
            .junction_candidates
            .sort_unstable_by_key(|p| (p.x, p.y));
        self.cache.junction_candidates.dedup();

        for idx in 0..self.cache.junction_candidates.len() {
            let p = self.cache.junction_candidates[idx];
            let non_hidden_ends = self.non_hidden_ends_at(doc, p);
            if non_hidden_ends >= 3 {
                continue;
            }
            if non_hidden_ends + self.overlay_ends_at(doc, iso, refused, p) >= 3 {
                self.cache.preview_junctions.push(p);
            }
        }
    }

    fn non_hidden_ends_at(&mut self, doc: &Doc, p: Pt) -> usize {
        doc.query(Rect { min: p, max: p }, &mut self.cache.query_scratch);
        let mut ends = 0usize;
        for &item in &self.cache.query_scratch {
            if self
                .cache
                .hidden
                .binary_search_by_key(&item_sort_key(item), |&h| item_sort_key(h))
                .is_ok()
            {
                continue;
            }
            match item {
                ItemId::Part(id) => {
                    if let Some(pins) = doc.pins(id) {
                        ends += pins.iter().filter(|&&pin| pin == p).count();
                    }
                }
                ItemId::Wire(id) => {
                    if let Some(&wire) = doc.wire(id) {
                        ends += usize::from(wire.a == p) + usize::from(wire.b == p);
                    }
                }
                ItemId::Label(_) => {}
            }
        }
        ends
    }

    fn overlay_ends_at(&self, doc: &Doc, iso: Isometry, refused: bool, p: Pt) -> usize {
        let mut ends = 0usize;
        for &id in &self.cache.affected_wires {
            if !refused
                && self
                    .cache
                    .plan_remove_sorted
                    .binary_search_by_key(&id.0, |w| w.0)
                    .is_ok()
            {
                continue;
            }
            if let Some(&wire) = doc.wire(id) {
                ends += usize::from(wire.a == p) + usize::from(wire.b == p);
            }
        }
        if refused {
            return ends;
        }
        for &wire in &self.cache.plan.add {
            ends += usize::from(wire.a == p) + usize::from(wire.b == p);
        }
        for &item in &self.items {
            match item {
                ItemId::Part(id) => {
                    if let Some(pins) = doc.pins(id) {
                        ends += pins.iter().filter(|&&pin| iso.apply(pin) == Ok(p)).count();
                    }
                }
                ItemId::Wire(id) => {
                    if let Some(&wire) = doc.wire(id) {
                        ends += usize::from(iso.apply(wire.a) == Ok(p))
                            + usize::from(iso.apply(wire.b) == Ok(p));
                    }
                }
                ItemId::Label(_) => {}
            }
        }
        ends
    }

    /// Returns the exact canonical list of affected-run wires drawn by the overlay in this state:
    /// surviving unselected affected wires (`affected_wires \ plan_remove_sorted`) plus
    /// `plan.add` (or all original `affected_wires` when refused) (`WINV5`).
    pub fn preview_wires(&self, doc: &Doc) -> Vec<Wire> {
        let mut wires = Vec::new();
        let refused = self.is_refused();
        for &id in &self.cache.affected_wires {
            if !refused
                && self
                    .cache
                    .plan_remove_sorted
                    .binary_search_by_key(&id.0, |w| w.0)
                    .is_ok()
            {
                continue;
            }
            if let Some(&w) = doc.wire(id) {
                wires.push(canonical_wire(w));
            }
        }
        if !refused {
            wires.extend(self.cache.plan.add.iter().copied().map(canonical_wire));
        }
        wires.sort_unstable_by_key(|w| ((w.a.x, w.a.y), (w.b.x, w.b.y)));
        wires
    }

    /// Returns the canonical list of non-selected rigid-run corner labels (`affected_labels`) and
    /// their preview coordinates: translated coordinates from `plan.labels` when not refused, or
    /// their original coordinates on refusal/identity (`WINV5`).
    pub fn preview_labels(&self, doc: &Doc) -> Vec<(LabelId, Pt)> {
        let mut labels = Vec::with_capacity(self.cache.affected_labels.len());
        let refused = self.is_refused();
        for &id in &self.cache.affected_labels {
            if !refused
                && let Ok(idx) = self
                    .cache
                    .plan_labels_sorted
                    .binary_search_by_key(&id.0, |(lid, _)| lid.0)
            {
                labels.push(self.cache.plan_labels_sorted[idx]);
            } else if let Some(label) = doc.label(id) {
                labels.push((id, label.at));
            }
        }
        labels.sort_unstable_by_key(|(id, _)| id.0);
        labels
    }
}

/// Builds a direct `Command::Reorient` for idle R, X or Y on `selection` (WD3).
pub fn reorient_selection_command(
    doc: &Doc,
    selection: &[ItemId],
    by: Orient,
    follow: FollowPolicy,
) -> Option<Command> {
    if by == Orient::IDENTITY || selection.is_empty() {
        return None;
    }
    let items: Vec<ItemId> = selection
        .iter()
        .copied()
        .filter(|&item| item_exists(doc, item))
        .collect();
    if items.is_empty() {
        return None;
    }
    let pivot = selection_pivot(doc, &items)?;
    Some(Command::Reorient {
        items,
        pivot,
        by,
        follow,
    })
}

/// Builds `Command::Delete` for the live items in `selection` (WD10, WQ4).
pub fn delete_selection_command(
    doc: &Doc,
    selection: &[ItemId],
    with_wires: bool,
) -> Option<Command> {
    let items: Vec<ItemId> = selection
        .iter()
        .copied()
        .filter(|&item| item_exists(doc, item))
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(Command::Delete { items, with_wires })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{
        Label, LabelId, PIN_LATTICE_NM, Part, PartId, Wire, WireId, prefs::WiringPrefs,
    };
    use omnia_symbols::SymbolLibrary;

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    fn resistor_part(doc: &mut Doc, at: Pt, des: &str) -> PartId {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(res);
        doc.apply(Command::PlacePart {
            part: Part {
                symbol: res,
                at,
                orient: Orient::IDENTITY,
                designator: des.into(),
                value: "10k".into(),
            },
            local_pins: def.pins.iter().map(|p| p.at).collect(),
            local_bounds: def.bounds,
            def: omnia_doc::PlaceDef::Unmodelled,
            params: omnia_doc::Params::new(),
        })
        .unwrap()
        .added
        .into_iter()
        .find_map(|id| match id {
            ItemId::Part(p) => Some(p),
            _ => None,
        })
        .unwrap()
    }

    fn canonical_wires_in_doc(doc: &Doc) -> Vec<Wire> {
        let mut wires: Vec<Wire> = doc.wires().map(|(_, &w)| canonical_wire(w)).collect();
        wires.sort_unstable_by_key(|w| ((w.a.x, w.a.y), (w.b.x, w.b.y)));
        wires
    }

    #[test]
    fn t5_3_selection_pivot_rules_are_deterministic_and_lattice_safe() {
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        let opamp_at = Pt {
            x: -PIN_LATTICE_NM,
            y: -PIN_LATTICE_NM,
        };
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: opamp,
                    at: opamp_at,
                    orient: Orient::IDENTITY,
                    designator: "U1".into(),
                    value: "".into(),
                },
                def.pins.iter().map(|p| p.at).collect(),
                def.bounds,
            )],
            [Wire {
                a: Pt { x: 0, y: 0 },
                b: Pt { x: 3 * G, y: 0 },
            }],
            [Label {
                at: Pt {
                    x: PIN_LATTICE_NM,
                    y: 2 * G,
                },
                text: "N1".into(),
            }],
        )
        .unwrap();

        // Single part pivots on part.at (preserves half-grid parity for OpAmp).
        assert_eq!(
            selection_pivot(&doc, &[ItemId::Part(PartId(0))]),
            Some(opamp_at)
        );
        // Single label pivots on label.at.
        assert_eq!(
            selection_pivot(&doc, &[ItemId::Label(LabelId(0))]),
            Some(Pt {
                x: PIN_LATTICE_NM,
                y: 2 * G,
            })
        );
        // Single wire snaps midpoint (1.5G, 0) onto GRID_NM.
        let wire_pivot = selection_pivot(&doc, &[ItemId::Wire(WireId(0))]).unwrap();
        assert_eq!(wire_pivot.x % G, 0);
        assert_eq!(wire_pivot.y % G, 0);

        // Multi-item selection snaps bounding centre onto GRID_NM, independent of input order.
        let p_ab =
            selection_pivot(&doc, &[ItemId::Part(PartId(0)), ItemId::Wire(WireId(0))]).unwrap();
        let p_ba =
            selection_pivot(&doc, &[ItemId::Wire(WireId(0)), ItemId::Part(PartId(0))]).unwrap();
        assert_eq!(p_ab, p_ba);
        assert_eq!(p_ab.x % G, 0);
        assert_eq!(p_ab.y % G, 0);
    }

    #[test]
    fn t5_3_drag_delta_snaps_anchor_pin_to_grid_and_keeps_click_at_zero() {
        // Off-grid anchor pin: clicking without moving or with sub-half-grid jitter stays zero.
        let off_pin = Pt {
            x: PIN_LATTICE_NM,
            y: PIN_LATTICE_NM,
        };
        for jitter in [
            pt(0, 0),
            Pt { x: 1, y: 0 },
            Pt {
                x: PIN_LATTICE_NM - 1,
                y: -(PIN_LATTICE_NM - 1),
            },
        ] {
            assert_eq!(
                drag_delta(pt(0, 0), jitter, pt(0, 0), Orient::IDENTITY, Some(off_pin)),
                Pt::ZERO,
                "sub-half-grid jitter {jitter:?} on off-grid part must stay a click"
            );
        }

        // Moving an off-grid part snaps its anchor pin onto GRID_NM.
        let delta = drag_delta(
            pt(0, 0),
            Pt {
                x: 2 * G + 200_000,
                y: -G - 100_000,
            },
            pt(0, 0),
            Orient::IDENTITY,
            Some(off_pin),
        );
        let final_pin = off_pin + delta;
        assert_eq!(final_pin.x % G, 0);
        assert_eq!(final_pin.y % G, 0);

        // Rotating during drag keeps the transformed anchor pin on GRID_NM.
        let delta_rot = drag_delta(
            pt(0, 0),
            Pt {
                x: G + 300_000,
                y: -2 * G,
            },
            pt(0, 0),
            Orient::ROT_90,
            Some(off_pin),
        );
        let iso = drag_isometry(delta_rot, pt(0, 0), Orient::ROT_90);
        let final_rot_pin = iso.apply(off_pin).unwrap();
        assert_eq!(final_rot_pin.x % G, 0);
        assert_eq!(final_rot_pin.y % G, 0);

        // K1 boundary: dragging to ±SHEET_LIMIT_NM snaps anchor pin on GRID_NM inside K1.
        for edge in [
            Pt {
                x: SHEET_LIMIT_NM,
                y: SHEET_LIMIT_NM,
            },
            Pt {
                x: -SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
        ] {
            let d = drag_delta(pt(0, 0), edge, pt(0, 0), Orient::IDENTITY, Some(off_pin));
            let snapped_pin = off_pin + d;
            assert!(
                snapped_pin.is_in_bounds(),
                "edge {edge:?} -> pin {snapped_pin:?} out of K1"
            );
            assert_eq!(snapped_pin.x % G, 0);
            assert_eq!(snapped_pin.y % G, 0);
        }
    }

    #[test]
    fn t5_3_drag_command_uses_move_or_transform() {
        let follow = WiringPrefs::default().follow_policy(false);
        let items = [ItemId::Part(PartId(0))];
        assert_eq!(
            drag_command(&items, Pt::ZERO, Pt::ZERO, Orient::IDENTITY, follow),
            None
        );
        assert_eq!(
            drag_command(&items, pt(1, 0), Pt::ZERO, Orient::IDENTITY, follow),
            Some(Command::Move {
                items: items.to_vec(),
                delta: pt(1, 0),
                follow,
            })
        );
        assert_eq!(
            drag_command(&items, Pt::ZERO, pt(2, 3), Orient::ROT_90, follow),
            Some(Command::Transform {
                items: items.to_vec(),
                delta: Pt::ZERO,
                pivot: pt(2, 3),
                by: Orient::ROT_90,
                follow,
            })
        );
    }

    #[test]
    fn w2_3_wd11_subgrid_moves_and_identity_skip_plan_follow() {
        let mut doc = Doc::new();
        let r1 = resistor_part(&mut doc, pt(-6, 0), "R1");
        let r2 = resistor_part(&mut doc, pt(6, 0), "R2");
        doc.apply(Command::AddWires(vec![Wire {
            a: pt(-4, 0),
            b: pt(4, 0),
        }]))
        .unwrap();

        let stamp = DocStamp {
            key: 1,
            revision: doc.revision(),
        };
        let mut drag = DragGesture::start(
            &doc,
            vec![ItemId::Part(r1)],
            ItemId::Part(r1),
            pt(-6, 0),
            stamp,
            WiringPrefs::default(),
            false,
        );
        assert_eq!(drag.preview(), FollowPreview::Accepted);
        assert_eq!(drag.plan_calls(), 0);
        assert!(drag.hidden_items().is_empty());

        // Move across multiple sub-grid offsets that snap to (1G, 0): plan_follow runs once.
        drag.set_lifted();
        assert_eq!(
            drag.hidden_items(),
            &[ItemId::Part(r1), ItemId::Wire(WireId(0))]
        );
        for dx_sub in [0, 100, -100, G / 3, -G / 3, G / 2 - 1] {
            drag.current = Pt {
                x: -5 * G + dx_sub,
                y: dx_sub / 2,
            };
            let changed = drag.update_preview(Some(&doc));
            if dx_sub == 0 {
                assert!(changed, "first snapped move updates plan");
            } else {
                assert!(!changed, "sub-grid move with same delta must not re-plan");
            }
            assert_eq!(drag.delta(), pt(1, 0));
            assert_eq!(drag.plan_calls(), 1, "WD11: sub-grid offset {dx_sub}");
        }

        // Alt toggles trigger re-planning once per toggle; switching back to follow restores routes.
        assert!(drag.set_alt(Some(&doc), true));
        assert_eq!(drag.plan_calls(), 2);
        assert!(drag.plan_add().is_empty());
        assert_eq!(drag.preview_wires(&doc), canonical_wires_in_doc(&doc));
        assert!(!drag.set_alt(Some(&doc), true));
        assert_eq!(drag.plan_calls(), 2);

        assert!(drag.set_alt(Some(&doc), false));
        assert_eq!(drag.plan_calls(), 3);
        assert!(!drag.plan_add().is_empty());

        // Dragging back to identity clears the plan without calling plan_follow.
        drag.current = pt(-6, 0);
        assert!(drag.update_preview(Some(&doc)));
        assert_eq!(drag.plan_calls(), 3);
        assert_eq!(drag.preview(), FollowPreview::Accepted);
        assert!(drag.plan_add().is_empty());
        assert_eq!(drag.preview_wires(&doc), canonical_wires_in_doc(&doc));
        assert_eq!(drag.items, vec![ItemId::Part(r1)]);
        let _ = r2;
    }

    #[test]
    fn w2_3_wk7_over_cap_skips_live_plan_and_keeps_hidden_to_items() {
        let mut doc = Doc::new();
        let count = FOLLOW_PREVIEW_CAP / 2 + 2; // 130 resistors × 2 runs = 260 runs (> 256)
        let mut items = Vec::with_capacity(count);
        let mut wires = Vec::with_capacity(count * 2);
        for idx in 0..count {
            let col = i32::try_from(idx % 13).unwrap();
            let row = i32::try_from(idx / 13).unwrap();
            let x_grid = (col - 6) * 14;
            let y_grid = (row - 5) * 6;
            let p = resistor_part(&mut doc, pt(x_grid, y_grid), &format!("R{idx}"));
            items.push(ItemId::Part(p));
            wires.push(Wire {
                a: pt(x_grid - 6, y_grid),
                b: pt(x_grid - 2, y_grid),
            });
            wires.push(Wire {
                a: pt(x_grid + 2, y_grid),
                b: pt(x_grid + 6, y_grid),
            });
        }
        doc.apply(Command::AddWires(wires)).unwrap();

        let stamp = DocStamp {
            key: 7,
            revision: doc.revision(),
        };
        let mut drag = DragGesture::start(
            &doc,
            items.clone(),
            items[0],
            pt(0, 0),
            stamp,
            WiringPrefs::default(),
            false,
        );
        assert_eq!(
            drag.preview(),
            FollowPreview::OverCap {
                run_count: count * 2
            }
        );
        assert!(drag.affected_wires().is_empty());
        assert_eq!(drag.plan_calls(), 0);

        drag.set_lifted();
        let mut expected_hidden = items;
        expected_hidden.sort_unstable_by_key(|&item| item_sort_key(item));
        assert_eq!(
            drag.hidden_items(),
            expected_hidden.as_slice(),
            "WK7: over-cap hides dragged items only"
        );

        for step in 1..=5 {
            drag.current = pt(0, step);
            assert!(!drag.update_preview(Some(&doc)));
            assert!(!drag.set_alt(Some(&doc), step % 2 == 1));
            assert_eq!(drag.plan_calls(), 0, "WK7: plan_follow skipped over cap");
        }
    }

    #[test]
    fn w2_3_winv5_preview_wires_match_post_commit_across_scenarios_and_junctions() {
        // TC-move-15: R1 wired through Wire(0) to 3-way junction (0, 0).
        let mut doc = Doc::new();
        let r1 = resistor_part(&mut doc, pt(-6, 0), "R1");
        doc.apply(Command::AddWires(vec![
            Wire {
                a: pt(-4, 0),
                b: pt(0, 0),
            },
            Wire {
                a: pt(0, 0),
                b: pt(0, 4),
            },
            Wire {
                a: pt(0, 0),
                b: pt(6, 0),
            },
        ]))
        .unwrap();

        let stamp = DocStamp {
            key: 2,
            revision: doc.revision(),
        };
        for alt in [false, true] {
            let mut drag = DragGesture::start(
                &doc,
                vec![ItemId::Part(r1)],
                ItemId::Part(r1),
                pt(-6, 0),
                stamp,
                WiringPrefs::default(),
                alt,
            );
            drag.current = pt(-5, 2);
            drag.set_lifted();
            assert!(drag.update_preview(Some(&doc)));
            assert!(
                drag.preview_junctions().contains(&pt(0, 0)),
                "K6: hidden junction dot at (0, 0) must appear in preview_junctions (alt={alt})"
            );

            let mut visible_wires = drag.preview_wires(&doc);
            for (id, &w) in doc.wires() {
                if !drag.hidden_items().contains(&ItemId::Wire(id)) {
                    visible_wires.push(canonical_wire(w));
                }
            }
            visible_wires.sort_unstable_by_key(|w| ((w.a.x, w.a.y), (w.b.x, w.b.y)));

            let mut committed = doc.fork();
            let cmd = drag.command(drag.follow_policy(alt)).unwrap();
            committed.apply(cmd).unwrap();
            assert_eq!(
                visible_wires,
                canonical_wires_in_doc(&committed),
                "WINV5: preview wires must match committed sheet wires (alt={alt})"
            );
        }
    }

    #[test]
    fn w2_3_refusal_verb_formats_move_vs_reorient_notices() {
        let follow_err = FollowError::NoRoute {
            from: pt(0, 0),
            to: pt(4, 0),
            blocker: Blocker::Pin {
                part: PartId(0),
                pin: 0,
            },
        };
        assert_eq!(
            format_follow_refusal(follow_err),
            "Move refused: no valid wire route (blocked by pin); hold Alt to detach"
        );
        assert_eq!(
            format_follow_refusal_for(RefusalVerb::Reorient, follow_err),
            "Reorient refused: no valid wire route (blocked by pin)"
        );
        assert_eq!(
            format_command_refusal(RefusalVerb::Reorient, CommandError::OutOfBounds),
            "Reorient refused: item or pin lies outside sheet limits"
        );
        assert_eq!(
            format_command_refusal(
                RefusalVerb::Move,
                CommandError::Capture {
                    wire: WireId(0),
                    blocker: Blocker::WireBody(WireId(1)),
                }
            ),
            "Move refused: moved wire would connect across a foreign pin or wire"
        );
    }

    #[test]
    fn w2_3_rigid_corner_label_preview_and_tc_move_17_selected_wire_and_part() {
        // Two resistors joined by a 2-segment L run carrying a non-selected corner label (WD9).
        let mut doc = Doc::new();
        let r1 = resistor_part(&mut doc, pt(-6, 0), "R1");
        let r2 = resistor_part(&mut doc, pt(6, 4), "R2");
        doc.apply(Command::AddWires(vec![
            Wire {
                a: pt(-4, 0),
                b: pt(4, 0),
            },
            Wire {
                a: pt(4, 0),
                b: pt(4, 4),
            },
        ]))
        .unwrap();
        let label_added = doc
            .apply(Command::AddLabel(Label {
                at: pt(4, 0),
                text: "MID".into(),
            }))
            .unwrap()
            .added;
        let lid = label_added
            .into_iter()
            .find_map(|id| match id {
                ItemId::Label(l) => Some(l),
                _ => None,
            })
            .unwrap();

        let stamp = DocStamp {
            key: 3,
            revision: doc.revision(),
        };
        // Dragging only R1 makes the L-run non-rigid, so `lid` stays unhidden in `SheetView`.
        for alt in [false, true] {
            let mut single_drag = DragGesture::start(
                &doc,
                vec![ItemId::Part(r1)],
                ItemId::Part(r1),
                pt(-6, 0),
                stamp,
                WiringPrefs::default(),
                alt,
            );
            assert!(
                single_drag.affected_labels().is_empty(),
                "non-rigid run must not hide its corner label (alt={alt})"
            );
            single_drag.current = pt(-5, 2);
            single_drag.set_lifted();
            assert!(single_drag.update_preview(Some(&doc)));
            assert!(
                !single_drag.hidden_items().contains(&ItemId::Label(lid)),
                "non-rigid corner label keeps SheetView text during drag (alt={alt})"
            );
        }

        let mut drag = DragGesture::start(
            &doc,
            vec![ItemId::Part(r1), ItemId::Part(r2)],
            ItemId::Part(r1),
            pt(-6, 0),
            stamp,
            WiringPrefs::default(),
            false,
        );
        assert_eq!(drag.affected_labels(), &[lid]);
        drag.current = pt(-4, 3);
        drag.set_lifted();
        assert!(drag.update_preview(Some(&doc)));
        assert!(drag.hidden_items().contains(&ItemId::Label(lid)));
        assert_eq!(drag.preview_labels(&doc), vec![(lid, pt(6, 3))]);

        let mut committed = doc.fork();
        committed
            .apply(drag.command(drag.follow_policy(false)).unwrap())
            .unwrap();
        assert_eq!(committed.label(lid).unwrap().at, pt(6, 3));

        // TC-move-17: drag a part together with one of its attached wires.
        let mut mix_doc = Doc::new();
        let mr1 = resistor_part(&mut mix_doc, pt(-6, 0), "R1");
        let attached = mix_doc
            .apply(Command::AddWires(vec![
                Wire {
                    a: pt(-10, 0),
                    b: pt(-8, 0),
                },
                Wire {
                    a: pt(-4, 0),
                    b: pt(0, 0),
                },
            ]))
            .unwrap()
            .added[0];
        let mut mix_drag = DragGesture::start(
            &mix_doc,
            vec![ItemId::Part(mr1), attached],
            ItemId::Part(mr1),
            pt(-6, 0),
            DocStamp {
                key: 4,
                revision: mix_doc.revision(),
            },
            WiringPrefs::default(),
            false,
        );
        mix_drag.current = pt(-5, 2);
        mix_drag.set_lifted();
        assert!(mix_drag.update_preview(Some(&mix_doc)));

        let mut preview = mix_drag.preview_wires(&mix_doc);
        let iso = mix_drag.isometry();
        for (id, &w) in mix_doc.wires() {
            if !mix_drag.hidden_items().contains(&ItemId::Wire(id)) {
                preview.push(canonical_wire(w));
            } else if mix_drag.items.contains(&ItemId::Wire(id)) {
                preview.push(canonical_wire(Wire {
                    a: iso.apply(w.a).unwrap(),
                    b: iso.apply(w.b).unwrap(),
                }));
            }
        }
        preview.sort_unstable_by_key(|w| ((w.a.x, w.a.y), (w.b.x, w.b.y)));

        let mut mix_committed = mix_doc.fork();
        mix_committed
            .apply(mix_drag.command(mix_drag.follow_policy(false)).unwrap())
            .unwrap();
        assert_eq!(preview, canonical_wires_in_doc(&mix_committed));
    }
}
