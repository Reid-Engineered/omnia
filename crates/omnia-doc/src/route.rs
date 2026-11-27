//! Wire routing rules: the L-route and the split/merge normal form applied by
//! `Command::AddWires` (D2, INV2; wiring-plan WD4, WD9, M12, M18).
//!
//! Local normal form in the touched neighbourhood of `AddWires` (INV2):
//! no wire end lies in another wire's interior; no two wires share a stretch of
//! non-zero length; and no two collinear segments meet at a point that holds
//! nothing else. A meeting point holds something else when a pin lies there, a
//! label sits there (WD9: a labelled point is never merged away), or a third
//! wire end meets there (a junction). Crossing interiors never split and never
//! connect (M18). The planner is pure and deterministic: its result depends
//! only on the geometry and the existing ids, never on hash-map iteration order
//! or on the order of the added wires. Ties are broken by geometry, then by the
//! lowest existing id on a merge; the first geometry-ordered split piece keeps
//! its source id. Fresh ids follow geometry order. Unrelated pre-existing
//! non-normal geometry is preserved; see `plan_add_wires` for the boundary.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{Doc, ItemId, PartId, Pt, Rect, Wire, WireId, points::End};

/// Threshold above which local obstacle slices are sorted for binary search;
/// smaller slices are scanned linearly in L1 cache.
const LOCAL_SORT_THRESHOLD: usize = 24;

/// Cached part body lookup inside a [`LocalPart`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum BodyCache {
    Unknown,
    None,
    Some(Rect),
}

/// Cached obstacle part inside a local dogleg window.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalPart {
    pub id: PartId,
    pub envelope: Rect,
    pub body: BodyCache,
    /// `u32::MAX` until lazily populated on first envelope intersection.
    pub pins_start: u32,
    pub pins_len: u32,
}

/// Cached obstacle wire inside a local dogleg window, pre-split by orientation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalWire {
    pub id: WireId,
    pub wire: Wire,
    /// Fixed coordinate: `y` for horizontal, `x` for vertical.
    pub fixed: i32,
    /// Minimum coordinate along the wire's axis.
    pub lo: i32,
    /// Maximum coordinate along the wire's axis.
    pub hi: i32,
}

/// Reusable L1-hot snapshot of obstacles intersecting a dogleg reach box.
/// Kept inside [`RouteScratch`] so warm routes allocate nothing.
#[derive(Debug, Default)]
pub(crate) struct LocalObstacles {
    pub parts: Vec<LocalPart>,
    pub pins: Vec<Pt>,
    /// Horizontal wires; sorted by `(fixed = y, lo = min.x, id.0)` iff `wires_h_sorted`.
    pub wires_h: Vec<LocalWire>,
    /// Vertical wires; sorted by `(fixed = x, lo = min.y, id.0)` iff `wires_v_sorted`.
    pub wires_v: Vec<LocalWire>,
    pub wires_h_sorted: bool,
    pub wires_v_sorted: bool,
    /// Reservations; sorted by `(y, x, id.0)` iff `res_yx_sorted`.
    pub res_yx: Vec<(Pt, PartId)>,
    /// Reservations sorted by `(x, y, id.0)`, populated iff `res_xy_sorted`.
    pub res_xy: Vec<(Pt, PartId)>,
    pub res_yx_sorted: bool,
    pub res_xy_sorted: bool,
    /// Scratch deduplication buffers reused across calls.
    pub seen_parts: Vec<(PartId, Rect, Option<Rect>)>,
    pub seen_wires: Vec<(WireId, Wire)>,
}

impl LocalObstacles {
    pub(crate) fn clear(&mut self) {
        self.parts.clear();
        self.pins.clear();
        self.wires_h.clear();
        self.wires_v.clear();
        self.wires_h_sorted = false;
        self.wires_v_sorted = false;
        self.res_yx.clear();
        self.res_xy.clear();
        self.res_yx_sorted = false;
        self.res_xy_sorted = false;
        self.seen_parts.clear();
        self.seen_wires.clear();
    }

    pub(crate) fn finish(&mut self) {
        if self.seen_parts.len() > 1 {
            self.seen_parts.sort_unstable_by_key(|&(id, _, _)| id.0);
            self.seen_parts.dedup_by_key(|(id, _, _)| id.0);
        }
        for &(id, envelope, cached_body) in &self.seen_parts {
            let body = match cached_body {
                Some(r) => BodyCache::Some(r),
                None => BodyCache::Unknown,
            };
            self.parts.push(LocalPart {
                id,
                envelope,
                body,
                pins_start: u32::MAX,
                pins_len: 0,
            });
        }
        if self.seen_wires.len() > 1 {
            self.seen_wires.sort_unstable_by_key(|&(id, _)| id.0);
            self.seen_wires.dedup_by_key(|(id, _)| id.0);
        }
        for &(id, wire) in &self.seen_wires {
            if wire.a.y == wire.b.y {
                self.wires_h.push(LocalWire {
                    id,
                    wire,
                    fixed: wire.a.y,
                    lo: wire.a.x.min(wire.b.x),
                    hi: wire.a.x.max(wire.b.x),
                });
            } else {
                self.wires_v.push(LocalWire {
                    id,
                    wire,
                    fixed: wire.a.x,
                    lo: wire.a.y.min(wire.b.y),
                    hi: wire.a.y.max(wire.b.y),
                });
            }
        }
        if self.wires_h.len() > LOCAL_SORT_THRESHOLD {
            self.wires_h
                .sort_unstable_by_key(|w| (w.fixed, w.lo, w.id.0));
            self.wires_h_sorted = true;
        }
        if self.wires_v.len() > LOCAL_SORT_THRESHOLD {
            self.wires_v
                .sort_unstable_by_key(|w| (w.fixed, w.lo, w.id.0));
            self.wires_v_sorted = true;
        }
        if self.res_yx.len() > LOCAL_SORT_THRESHOLD {
            if !self.res_yx_sorted {
                self.res_yx
                    .sort_unstable_by_key(|&(p, id)| (p.y, p.x, id.0));
                self.res_yx_sorted = true;
            }
            self.res_xy.extend_from_slice(&self.res_yx);
            self.res_xy
                .sort_unstable_by_key(|&(p, id)| (p.x, p.y, id.0));
            self.res_xy_sorted = true;
        }
    }

    #[inline]
    fn part_pins<S: RouteSheet>(&mut self, idx: usize, sheet: &S) -> &[Pt] {
        let part = &mut self.parts[idx];
        if part.pins_start == u32::MAX {
            let start = self.pins.len() as u32;
            let pins = sheet.pins(part.id);
            self.pins.extend_from_slice(pins);
            part.pins_start = start;
            part.pins_len = pins.len() as u32;
        }
        let start = part.pins_start as usize;
        let end = start + part.pins_len as usize;
        &self.pins[start..end]
    }

    #[inline]
    fn part_body<S: RouteSheet>(&mut self, idx: usize, sheet: &S) -> Option<Rect> {
        let part = &mut self.parts[idx];
        match part.body {
            BodyCache::Some(r) => Some(r),
            BodyCache::None => None,
            BodyCache::Unknown => {
                let b = sheet.part_body(part.id);
                part.body = match b {
                    Some(r) => BodyCache::Some(r),
                    None => BodyCache::None,
                };
                b
            }
        }
    }
}

/// An obstacle emitted by [`RouteSheet::visit_segment`].
pub(crate) enum SegmentHit<'a> {
    Part {
        id: PartId,
        envelope: Rect,
        body: Option<Rect>,
        pins: &'a [Pt],
    },
    Wire {
        id: WireId,
        wire: Wire,
    },
    Res {
        id: PartId,
        at: Pt,
    },
}

/// What WD4's validator and scorer read from a sheet. [`Doc`] is one; the follow planner's
/// final-geometry view (W2.1) is another, layered over the document without copying it.
pub(crate) trait RouteSheet {
    /// As [`Doc::query`]: replaces `out` with the items whose indexed bounds meet `area`.
    /// Order is unspecified; callers must not depend on it.
    #[cfg(test)]
    fn query(&self, area: Rect, out: &mut Vec<ItemId>);
    /// World pins of a live part returned by [`RouteSheet::query`].
    fn pins(&self, id: PartId) -> &[Pt];
    /// A live wire returned by [`RouteSheet::query`].
    #[cfg(test)]
    fn wire(&self, id: WireId) -> Wire;
    /// As [`Doc::part_body`].
    fn part_body(&self, id: PartId) -> Option<Rect>;
    /// Streams every part, wire, and reservation whose indexed bounds meet a normalised 1D
    /// axis-aligned segment box `area` without allocating or mutating index stamps.
    ///
    /// Because `area` is 1D, every perpendicular wire shares at most one K12 cell with `area`
    /// and is visited at most once (making crossing counts exact without deduplication).
    /// Parts and parallel wires spanning multiple cells may be visited once per shared cell;
    /// callers apply only idempotent checks to them.
    fn visit_segment(&self, area: Rect, horizontal: bool, on_hit: impl FnMut(SegmentHit<'_>))
    where
        Self: Sized;
    /// Populates `out` with deduplicated obstacles intersecting normalised box `area`.
    fn collect_box(&self, area: Rect, out: &mut LocalObstacles)
    where
        Self: Sized;
}

impl RouteSheet for Doc {
    #[cfg(test)]
    fn query(&self, area: Rect, out: &mut Vec<ItemId>) {
        Doc::query(self, area, out);
    }
    fn pins(&self, id: PartId) -> &[Pt] {
        Doc::pins(self, id).expect("queried part is live")
    }
    #[cfg(test)]
    fn wire(&self, id: WireId) -> Wire {
        *Doc::wire(self, id).expect("queried wire is live")
    }
    fn part_body(&self, id: PartId) -> Option<Rect> {
        Doc::part_body(self, id)
    }
    fn visit_segment(&self, area: Rect, _horizontal: bool, mut on_hit: impl FnMut(SegmentHit<'_>)) {
        self.index
            .visit_parts_and_wires_unstamped(area, |item, bounds| match item {
                ItemId::Part(id) => on_hit(SegmentHit::Part {
                    id,
                    envelope: bounds,
                    body: None,
                    pins: Doc::pins(self, id).expect("queried part is live"),
                }),
                ItemId::Wire(id) => on_hit(SegmentHit::Wire {
                    id,
                    wire: Wire {
                        a: bounds.min,
                        b: bounds.max,
                    },
                }),
                ItemId::Label(_) => {}
            });
    }
    fn collect_box(&self, area: Rect, out: &mut LocalObstacles) {
        out.clear();
        self.index
            .visit_parts_and_wires_unstamped(area, |item, bounds| match item {
                ItemId::Part(id) => out.seen_parts.push((id, bounds, None)),
                ItemId::Wire(id) => out.seen_wires.push((
                    id,
                    Wire {
                        a: bounds.min,
                        b: bounds.max,
                    },
                )),
                ItemId::Label(_) => {}
            });
        out.finish();
    }
}

/// The L-route between `a` and `b`: zero, one, or two axis-aligned segments.
///
/// Equal points route to nothing and collinear points to a single segment.
/// Otherwise the route has one corner. `flip == false` leaves `a` horizontally
/// (corner at `(b.x, a.y)`); `flip == true` leaves `a` vertically (corner at
/// `(a.x, b.y)`). Segment direction follows `a` to corner to `b`.
pub fn l_route(a: Pt, b: Pt, flip: bool) -> [Option<Wire>; 2] {
    if a == b {
        return [None, None];
    }
    if a.x == b.x || a.y == b.y {
        return [Some(Wire { a, b }), None];
    }
    let corner = if flip {
        Pt { x: a.x, y: b.y }
    } else {
        Pt { x: b.x, y: a.y }
    };
    [Some(Wire { a, b: corner }), Some(Wire { a: corner, b })]
}

/// WD4 refusal. Pin indices match `Doc::pins` without narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    Pin {
        part: crate::PartId,
        pin: usize,
    },
    WireEnd(WireId),
    /// A corner of the new wires (a vertex that is not one of the two own ends)
    /// lies in this foreign wire's interior (W1.5 clarification of WD4 rule 2).
    WireBody(WireId),
    Overlap(WireId),
    /// Outside K1 or off the WK1 lattice (also malformed candidate segments).
    Sheet,
}

fn on_lattice(p: Pt) -> bool {
    p.is_in_bounds() && p.x % crate::PIN_LATTICE_NM == 0 && p.y % crate::PIN_LATTICE_NM == 0
}

fn interior_contains(w: Wire, p: Pt) -> bool {
    contains(w, p) && p != w.a && p != w.b
}

fn shared_stretch(a: Wire, b: Wire) -> bool {
    let (al, ah, horizontal) = normalised(a);
    let (bl, bh, other_horizontal) = normalised(b);
    horizontal == other_horizontal
        && if horizontal {
            al.y == bl.y && al.x.max(bl.x) < ah.x.min(bh.x)
        } else {
            al.x == bl.x && al.y.max(bl.y) < ah.y.min(bh.y)
        }
}

/// WD4: rule priority first, then segment order from `a`. Within a segment,
/// choose the first obstruction along its direction, then geometry, kind (pin
/// before end before body), and lowest id/index. Own ends exempt points, never
/// overlaps. Rule 2 covers the run's whole interior: a segment's interior and
/// its vertices that are not own ends (the run's corners), where a foreign pin,
/// a foreign wire end or a foreign wire's interior blocks (W1.5).
/// Query buffers contain only items intersecting one segment, not the sheet.
/// Callers remove the old run from the document view before validating it.
pub fn check_new_wires(doc: &Doc, wires: &[Wire], own_ends: [Pt; 2]) -> Result<(), Blocker> {
    check_new_wires_ignoring(doc, wires, &own_ends, &[])
}

/// [`check_new_wires`] with a caller-owned query buffer: allocates nothing once
/// `scratch` holds the largest single-segment query result.
pub fn check_new_wires_in(
    doc: &Doc,
    wires: &[Wire],
    own_ends: [Pt; 2],
    _scratch: &mut Vec<ItemId>,
) -> Result<(), Blocker> {
    check_new_wires_ignoring(doc, wires, &own_ends, &[])
}

/// [`check_new_wires`] on any [`RouteSheet`], streaming without allocating.
pub(crate) fn check_new_wires_on<S: RouteSheet>(
    doc: &S,
    wires: &[Wire],
    own_ends: [Pt; 2],
) -> Result<(), Blocker> {
    check_new_wires_ignoring(doc, wires, &own_ends, &[])
}

fn check_new_wires_ignoring<S: RouteSheet>(
    doc: &S,
    wires: &[Wire],
    own_ends: &[Pt],
    ignore_wires: &[WireId],
) -> Result<(), Blocker> {
    if !wires.iter().all(|&w| shape_valid(w)) {
        return Err(Blocker::Sheet);
    }
    let mut first_overlap = None;
    for &w in wires {
        let area = Rect::from_points(w.a, w.b).unwrap();
        let horizontal = w.a.y == w.b.y;
        let mut points = PointRule::new(w, own_ends);
        let mut overlaps = OverlapRule::new(w);
        doc.visit_segment(area, horizontal, |hit| match hit {
            SegmentHit::Part { id, pins, .. } => {
                points.part(id, pins);
            }
            SegmentHit::Wire { id, wire } => {
                if !ignore_wires.contains(&id) {
                    points.wire(id, wire);
                    if first_overlap.is_none() {
                        overlaps.wire(id, wire);
                    }
                }
            }
            SegmentHit::Res { id, at } => {
                points.consider(at, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
            }
        });
        if let Some(blocker) = points.blocker() {
            return Err(blocker);
        }
        if first_overlap.is_none() {
            first_overlap = overlaps.blocker();
        }
    }
    if let Some(blocker) = first_overlap {
        return Err(blocker);
    }
    Ok(())
}

/// WD4 rule 1 for one segment: lattice vertices inside the sheet, axis-aligned, not a point.
fn shape_valid(w: Wire) -> bool {
    on_lattice(w.a) && on_lattice(w.b) && w.a != w.b && (w.a.x == w.b.x || w.a.y == w.b.y)
}

type PointKey = (i64, (i32, i32), u8, u32, usize);
type OverlapKey = (i64, (i32, i32), (i32, i32), u32);

/// WD4 rule 2 accumulated over one segment's items, in any order.
struct PointRule<'a> {
    w: Wire,
    bounds: Rect,
    own_ends: &'a [Pt],
    best: Option<(PointKey, Blocker)>,
}

impl<'a> PointRule<'a> {
    fn new(w: Wire, own_ends: &'a [Pt]) -> Self {
        Self {
            w,
            bounds: Rect::from_points(w.a, w.b).unwrap(),
            own_ends,
            best: None,
        }
    }

    fn consider(&mut self, p: Pt, kind: u8, id: u32, pin: usize, blocker: Blocker) {
        if self.bounds.contains(p) && !self.own_ends.contains(&p) {
            let key = (self.w.a.manhattan_distance(p), pt_key(p), kind, id, pin);
            if self.best.as_ref().is_none_or(|&(old, _)| key < old) {
                self.best = Some((key, blocker));
            }
        }
    }

    fn part(&mut self, part: PartId, pins: &[Pt]) {
        for (pin, &p) in pins.iter().enumerate() {
            self.consider(p, 0, part.0, pin, Blocker::Pin { part, pin });
        }
    }

    fn wire(&mut self, id: WireId, other: Wire) {
        for p in [other.a, other.b] {
            self.consider(p, 1, id.0, 0, Blocker::WireEnd(id));
        }
        for v in [self.w.a, self.w.b] {
            if interior_contains(other, v) {
                self.consider(v, 2, id.0, 0, Blocker::WireBody(id));
            }
        }
    }

    fn blocker(&self) -> Option<Blocker> {
        self.best.map(|(_, blocker)| blocker)
    }
}

/// WD4 rule 3 accumulated over one segment's items, in any order.
struct OverlapRule {
    w: Wire,
    best: Option<(OverlapKey, WireId)>,
}

impl OverlapRule {
    fn new(w: Wire) -> Self {
        Self { w, best: None }
    }

    fn wire(&mut self, id: WireId, other: Wire) {
        let w = self.w;
        if shared_stretch(w, other) {
            let (lo, hi, _) = normalised(other);
            let first = Pt {
                x: w.a.x.clamp(lo.x, hi.x),
                y: w.a.y.clamp(lo.y, hi.y),
            };
            let distance = w.a.manhattan_distance(first);
            let key = (distance, pt_key(lo), pt_key(hi), id.0);
            if self.best.as_ref().is_none_or(|&(old, _)| key < old) {
                self.best = Some((key, id));
            }
        }
    }

    fn blocker(&self) -> Option<Blocker> {
        self.best.map(|(_, id)| Blocker::Overlap(id))
    }
}

/// The axis a route leaves `a` along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// WK5: dogleg mid-lines reach this far beyond the box spanned by the ends.
pub const DOGLEG_REACH_NM: i32 = 8 * crate::GRID_NM;
/// WK6: at most this many doglegs per route (after the two L-routes).
pub const DOGLEG_CAP: usize = 128;
/// WD4's comparison, lowest first; field order is the comparison order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Score {
    /// Segments whose interior passes through the interior of any part body.
    bodies: u8,
    bends: u8,
    /// Foreign wires crossed at a point interior to both.
    crossings: u32,
    length: i64,
    /// WD4's candidate number: 0 straight, 1 and 2 the L-routes, 3 on doglegs.
    number: u16,
}

impl Score {
    /// The comparison as one integer: each field in its own bits, most significant
    /// first, so `a.key() < b.key()` exactly when `a < b`. Length is a sum of
    /// distances, never negative.
    #[inline]
    fn key(self) -> u128 {
        debug_assert!(self.length >= 0);
        (u128::from(self.bodies) << 120)
            | (u128::from(self.bends) << 112)
            | (u128::from(self.crossings) << 80)
            | (u128::from(self.length as u64) << 16)
            | u128::from(self.number)
    }
}

/// Compact dogleg candidate (24 bytes) for fast enumeration and sorting by bound key.
#[derive(Debug, Clone, Copy)]
struct Dogleg {
    length: i64,
    m: i32,
    number: u16,
    horizontal_outer: bool,
}

impl Dogleg {
    #[inline]
    fn wires(self, a: Pt, b: Pt) -> [Wire; 3] {
        if self.horizontal_outer {
            let p1 = Pt { x: self.m, y: a.y };
            let p2 = Pt { x: self.m, y: b.y };
            [Wire { a, b: p1 }, Wire { a: p1, b: p2 }, Wire { a: p2, b }]
        } else {
            let p1 = Pt { x: a.x, y: self.m };
            let p2 = Pt { x: b.x, y: self.m };
            [Wire { a, b: p1 }, Wire { a: p1, b: p2 }, Wire { a: p2, b }]
        }
    }

    #[inline]
    fn bound_key(self, min_bodies: u8) -> u128 {
        Score {
            bodies: min_bodies,
            bends: 2,
            crossings: 0,
            length: self.length,
            number: self.number,
        }
        .key()
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
struct Candidate {
    wires: [Wire; 3],
    len: u8,
    score: Score,
}

#[cfg(test)]
impl Candidate {
    fn wires(&self) -> &[Wire] {
        &self.wires[..self.len as usize]
    }
}

/// Caller-owned buffers for [`route`]. A warm scratch makes `route` allocation
/// free: the dogleg buffers are reserved on construction and the local obstacle
/// snapshot keeps the capacity of the largest dogleg box it has served.
#[derive(Debug)]
pub struct RouteScratch {
    doglegs: Vec<Dogleg>,
    deferred_invalid: Vec<u16>,
    local: LocalObstacles,
}

impl RouteScratch {
    pub fn new() -> Self {
        Self {
            doglegs: Vec::with_capacity(DOGLEG_CAP),
            deferred_invalid: Vec::with_capacity(DOGLEG_CAP),
            local: LocalObstacles::default(),
        }
    }
}

impl Default for RouteScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// WD4: route a new run from `a` to `b`.
///
/// Enumerates the candidates in WD4's order (straight; the L leaving `a` along
/// `first_leg`, then the other L; then up to WK6 doglegs) and returns the
/// lowest-scoring valid one in `out`, segments directed from `a` to `b`.
/// Validity is [`check_new_wires_in`] with `[a, b]` as the own ends. With no
/// valid candidate, `out` is left empty and the error is the blocker of the
/// lowest-scoring invalid candidate (the same comparison ranks them). `a == b`
/// routes to nothing.
///
/// Candidates are evaluated best first: Stage 1 checks the straight or L-route
/// base candidates and exits immediately when one provably beats every dogleg
/// (`bends == 2`, `bodies >= min_bodies`). Stage 2 enumerates doglegs in WD4
/// order, sorts them once by `(length, number)` (exact bound key order), and
/// evaluates them validity-first against an L1-hot [`LocalObstacles`] snapshot,
/// stopping as soon as the next dogleg's lower bound reaches the best valid
/// score. Invalid doglegs are only fully scored on refusal to determine the
/// exact minimum-score blocker.
///
/// Doglegs: for `d = 1, 2, ...` grid steps (K2) from `a`'s coordinate, H-V-H
/// (vertical at `x = m`) then V-H-V (horizontal at `y = m`), each first on the
/// side towards `b` (the lesser coordinate when level), within the span of the
/// ends widened by WK5 on both sides; shapes with a zero-length segment are not
/// candidates and do not count towards WK6.
///
/// Cost is the candidates' local index queries; nothing scales with the
/// document beyond them, and a warm `scratch` and `out` allocate nothing. The
/// caller removes the run being replaced from the document first, as for
/// [`check_new_wires`].
pub fn route(
    doc: &Doc,
    a: Pt,
    b: Pt,
    first_leg: Axis,
    scratch: &mut RouteScratch,
    out: &mut Vec<Wire>,
) -> Result<(), Blocker> {
    route_on(doc, a, b, first_leg, scratch, out)
}

/// [`route`] on any [`RouteSheet`].
pub(crate) fn route_on<S: RouteSheet>(
    doc: &S,
    a: Pt,
    b: Pt,
    first_leg: Axis,
    scratch: &mut RouteScratch,
    out: &mut Vec<Wire>,
) -> Result<(), Blocker> {
    out.clear();
    if a == b {
        return Ok(());
    }
    let RouteScratch {
        doglegs,
        deferred_invalid,
        local,
    } = scratch;

    let ends_on_lattice = on_lattice(a) && on_lattice(b);
    let mut best_valid: Option<(u128, [Wire; 3], u8)> = None;
    let mut best_invalid: Option<(u128, Blocker)> = None;

    // Stage 1: evaluate base candidates (straight or the two L-routes) before enumerating
    // any doglegs. Every dogleg has `bodies >= min_bodies` and `bends == 2`, so any valid
    // base candidate with `bodies <= min_bodies` strictly beats every dogleg.
    let min_bodies = if a.x == b.x || a.y == b.y {
        let w0 = Wire { a, b };
        let (s0, v0, inside) = assess(doc, &[w0], [a, b], 0, ends_on_lattice);
        let min_bodies = u8::from(inside[0]) + u8::from(inside[1]);
        if v0.is_none() && s0.bodies <= min_bodies {
            out.push(w0);
            return Ok(());
        }
        match v0 {
            None => best_valid = Some((s0.key(), [w0, w0, w0], 1)),
            Some(blocker) => best_invalid = Some((s0.key(), blocker)),
        }
        min_bodies
    } else {
        let horizontal_first = Pt { x: b.x, y: a.y };
        let vertical_first = Pt { x: a.x, y: b.y };
        let (first, second) = match first_leg {
            Axis::Horizontal => (horizontal_first, vertical_first),
            Axis::Vertical => (vertical_first, horizontal_first),
        };
        let w1 = [Wire { a, b: first }, Wire { a: first, b }];
        let (s1, v1, inside) = assess(doc, &w1, [a, b], 1, ends_on_lattice);
        let min_bodies = u8::from(inside[0]) + u8::from(inside[1]);
        // L1 has lower bound `(min_bodies, 1, 0, manhattan, 2)`; if L0 is valid with
        // `min_bodies` and 0 crossings, L0 provably beats both L1 and every dogleg.
        if v1.is_none() && s1.bodies == min_bodies && s1.crossings == 0 {
            out.extend_from_slice(&w1);
            return Ok(());
        }
        let k1 = s1.key();
        match v1 {
            None => best_valid = Some((k1, [w1[0], w1[1], w1[0]], 2)),
            Some(blocker) => best_invalid = Some((k1, blocker)),
        }

        let w2 = [Wire { a, b: second }, Wire { a: second, b }];
        let (s2, v2, _) = assess(doc, &w2, [a, b], 2, ends_on_lattice);
        let k2 = s2.key();
        match v2 {
            None => {
                if best_valid.is_none_or(|(bk, _, _)| k2 < bk) {
                    best_valid = Some((k2, [w2[0], w2[1], w2[0]], 2));
                }
            }
            Some(blocker) => {
                if best_invalid.is_none_or(|(bk, _)| k2 < bk) {
                    best_invalid = Some((k2, blocker));
                }
            }
        }

        // If either L-route is valid with `bodies == min_bodies`, it strictly beats every
        // dogleg (`bends == 2`); return without enumerating doglegs.
        let dogleg_floor = Score {
            bodies: min_bodies,
            bends: 2,
            ..Score::default()
        }
        .key();
        if let Some((bk, wires, len)) = best_valid
            && bk < dogleg_floor
        {
            out.extend_from_slice(&wires[..usize::from(len)]);
            return Ok(());
        }
        min_bodies
    };

    // Stage 2: enumerate doglegs, sort once by exact lower-bound key `(length, number)`,
    // and walk them best-first.
    doglegs.clear();
    enumerate_doglegs(a, b, doglegs);
    doglegs.sort_unstable_by_key(|d| (d.length, d.number));

    let limit = crate::SHEET_LIMIT_NM;
    let limit64 = i64::from(limit);
    let reach = i64::from(DOGLEG_REACH_NM);
    let dx_reach = if a.y != b.y { reach } else { 0 };
    let dy_reach = if a.x != b.x { reach } else { 0 };
    let reach_box = Rect {
        min: Pt {
            x: (i64::from(a.x.min(b.x)) - dx_reach).clamp(-limit64, limit64) as i32,
            y: (i64::from(a.y.min(b.y)) - dy_reach).clamp(-limit64, limit64) as i32,
        },
        max: Pt {
            x: (i64::from(a.x.max(b.x)) + dx_reach).clamp(-limit64, limit64) as i32,
            y: (i64::from(a.y.max(b.y)) + dy_reach).clamp(-limit64, limit64) as i32,
        },
    };
    let use_local = crate::index::SpatialIndex::cell_count(reach_box) <= 64;
    if use_local {
        doc.collect_box(reach_box, local);
        deferred_invalid.clear();
        for (idx, &d) in doglegs.iter().enumerate() {
            let best_key = best_valid.map_or(u128::MAX, |(bk, _, _)| bk);
            if d.bound_key(min_bodies) >= best_key {
                break;
            }
            let shape_ok = ends_on_lattice && -limit <= d.m && d.m <= limit;
            if !shape_ok {
                if best_valid.is_none() {
                    deferred_invalid.push(idx as u16);
                }
                continue;
            }
            let wires = d.wires(a, b);
            match assess_local_for_valid(doc, local, &wires, [a, b], d.number, d.length, best_key) {
                FastEval::Valid(score) => {
                    let key = score.key();
                    if key < best_key {
                        best_valid = Some((key, wires, 3));
                    }
                }
                FastEval::Invalid => {
                    if best_valid.is_none() {
                        deferred_invalid.push(idx as u16);
                    }
                }
                FastEval::Pruned => {}
            }
        }
        if let Some((_, wires, len)) = best_valid {
            out.extend_from_slice(&wires[..usize::from(len)]);
            return Ok(());
        }
        // Refusal path: score deferred invalid doglegs in bound-key order to find the
        // exact minimum-score blocker per WD4.
        for &idx in deferred_invalid.iter() {
            let d = doglegs[usize::from(idx)];
            if best_invalid.is_some_and(|(bk, _)| d.bound_key(min_bodies) >= bk) {
                break;
            }
            let wires = d.wires(a, b);
            let judged = ends_on_lattice && -limit <= d.m && d.m <= limit;
            let (score, verdict, _) = assess_local(doc, local, &wires, [a, b], d.number, judged);
            let blocker = verdict.expect("deferred dogleg must be invalid");
            let key = score.key();
            if best_invalid.is_none_or(|(bk, _)| key < bk) {
                best_invalid = Some((key, blocker));
            }
        }
        return Err(best_invalid.map_or(Blocker::Sheet, |(_, b)| b));
    }

    // Large reach-box fallback (> 64 cells): stream segments directly against `doc`.
    for &d in doglegs.iter() {
        let bound_key = d.bound_key(min_bodies);
        if let Some((bk, _, _)) = best_valid
            && bound_key >= bk
        {
            break;
        }
        // With no valid candidate yet, keep going even past `best_invalid`'s bound: a later
        // dogleg could still be valid.
        let wires = d.wires(a, b);
        let judged = ends_on_lattice && -limit <= d.m && d.m <= limit;
        let (score, verdict, _) = assess(doc, &wires, [a, b], d.number, judged);
        let key = score.key();
        match verdict {
            None => {
                if best_valid.is_none_or(|(bk, _, _)| key < bk) {
                    best_valid = Some((key, wires, 3));
                }
            }
            Some(blocker) => {
                if best_invalid.is_none_or(|(bk, _)| key < bk) {
                    best_invalid = Some((key, blocker));
                }
            }
        }
    }
    if let Some((_, wires, len)) = best_valid {
        out.extend_from_slice(&wires[..usize::from(len)]);
        return Ok(());
    }
    Err(best_invalid.map_or(Blocker::Sheet, |(_, b)| b))
}

fn enumerate_doglegs(a: Pt, b: Pt, out: &mut Vec<Dogleg>) {
    let step = i64::from(crate::GRID_NM);
    let reach = i64::from(DOGLEG_REACH_NM);
    let (ax, ay, bx, by) = (
        i64::from(a.x),
        i64::from(a.y),
        i64::from(b.x),
        i64::from(b.y),
    );
    let dx_span = (ax - bx).abs();
    let dy_span = (ay - by).abs();
    // (a's coordinate, b's coordinate, low, high, orthogonal span): H-V-H (m is an x), then V-H-V.
    let families = [
        (ax, bx, ax.min(bx) - reach, ax.max(bx) + reach, dy_span),
        (ay, by, ay.min(by) - reach, ay.max(by) + reach, dx_span),
    ];
    let max_steps = families
        .iter()
        .map(|&(from, _, lo, hi, _)| (hi - from).max(from - lo) / step)
        .max()
        .unwrap_or(0);
    let mut number = 3u16;
    for d in 1..=max_steps {
        for (family, &(from, to, lo, hi, ortho_span)) in families.iter().enumerate() {
            if ortho_span == 0 {
                continue;
            }
            let toward = if to > from { 1 } else { -1 };
            for side in [toward, -toward] {
                let m = from + side * d * step;
                if m < lo || m > hi || m == to {
                    continue;
                }
                // Inside [lo, hi]; i32 for any ends within K1.
                let Ok(m_i32) = i32::try_from(m) else {
                    continue;
                };
                let length = (from - m).abs() + ortho_span + (m - to).abs();
                out.push(Dogleg {
                    length,
                    m: m_i32,
                    number,
                    horizontal_outer: family == 0,
                });
                number += 1;
                if usize::from(number - 3) == DOGLEG_CAP {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
fn make_candidate(a: Pt, points: &[Pt], number: u16) -> Candidate {
    let mut wires = [Wire { a, b: a }; 3];
    for (slot, pair) in wires.iter_mut().zip(points.windows(2)) {
        *slot = Wire {
            a: pair[0],
            b: pair[1],
        };
    }
    Candidate {
        wires,
        len: (points.len() - 1) as u8,
        score: Score {
            number,
            ..Score::default()
        },
    }
}

#[cfg(test)]
fn enumerate_candidates(a: Pt, b: Pt, first_leg: Axis, out: &mut Vec<Candidate>) {
    out.clear();
    if a.x == b.x || a.y == b.y {
        out.push(make_candidate(a, &[a, b], 0));
    } else {
        let horizontal_first = Pt { x: b.x, y: a.y };
        let vertical_first = Pt { x: a.x, y: b.y };
        let (first, second) = match first_leg {
            Axis::Horizontal => (horizontal_first, vertical_first),
            Axis::Vertical => (vertical_first, horizontal_first),
        };
        out.push(make_candidate(a, &[a, first, b], 1));
        out.push(make_candidate(a, &[a, second, b], 2));
    }
    let mut doglegs = Vec::with_capacity(DOGLEG_CAP);
    enumerate_doglegs(a, b, &mut doglegs);
    for d in doglegs {
        out.push(Candidate {
            wires: d.wires(a, b),
            len: 3,
            score: Score {
                number: d.number,
                ..Score::default()
            },
        });
    }
}

/// WD4's score of a candidate and its verdict under [`check_new_wires_in`] with
/// `own_ends`, streaming directly over the index cells per segment without
/// allocating. WD4 scores invalid candidates too: clip only the index query to
/// K1 and keep the original segment for body and crossing tests.
fn assess<S: RouteSheet>(
    doc: &S,
    wires: &[Wire],
    own_ends: [Pt; 2],
    number: u16,
    judged: bool,
) -> (Score, Option<Blocker>, [bool; 2]) {
    debug_assert_eq!(judged, wires.iter().all(|&w| shape_valid(w)));
    let mut score = Score {
        bends: (wires.len() - 1) as u8,
        number,
        ..Score::default()
    };
    let mut point = None;
    let mut overlap = None;
    let mut inside = [false; 2];
    let last = wires.len() - 1;
    let limit = crate::SHEET_LIMIT_NM;
    for (index, &w) in wires.iter().enumerate() {
        score.length += w.a.manhattan_distance(w.b);
        let horizontal = w.a.y == w.b.y;
        let area = Rect {
            min: Pt {
                x: w.a.x.min(w.b.x).clamp(-limit, limit),
                y: w.a.y.min(w.b.y).clamp(-limit, limit),
            },
            max: Pt {
                x: w.a.x.max(w.b.x).clamp(-limit, limit),
                y: w.a.y.max(w.b.y).clamp(-limit, limit),
            },
        };
        // Rules 2 and 3 matter only until a point blocker is known.
        let rules = judged && point.is_none();
        // Only a valid shape is judged; its bounds exist (inside K1).
        let mut points = rules.then(|| PointRule::new(w, &own_ends));
        let mut overlaps = OverlapRule::new(w);
        let mut through_body = false;
        doc.visit_segment(area, horizontal, |hit| match hit {
            SegmentHit::Part {
                id,
                envelope,
                body: cached_body,
                pins,
            } => {
                let need_body = (!through_body && passes_through(w, envelope))
                    || (index == 0 && !inside[0] && strictly_inside(envelope, own_ends[0]))
                    || (index == last && !inside[1] && strictly_inside(envelope, own_ends[1]));
                if need_body && let Some(body) = cached_body.or_else(|| doc.part_body(id)) {
                    through_body |= passes_through(w, body);
                    inside[0] |= index == 0 && strictly_inside(body, own_ends[0]);
                    inside[1] |= index == last && strictly_inside(body, own_ends[1]);
                }
                if let Some(points) = points.as_mut() {
                    points.part(id, pins);
                }
            }
            SegmentHit::Wire { id, wire } => {
                score.crossings += u32::from(crosses(w, wire));
                if let Some(points) = points.as_mut() {
                    points.wire(id, wire);
                    overlaps.wire(id, wire);
                }
            }
            SegmentHit::Res { id, at } => {
                if let Some(points) = points.as_mut() {
                    points.consider(at, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
                }
            }
        });
        score.bodies += u8::from(through_body);
        if let Some(points) = points {
            point = points.blocker();
            if point.is_none() && overlap.is_none() {
                overlap = overlaps.blocker();
            }
        }
    }
    let verdict = if judged {
        point.or(overlap)
    } else {
        Some(Blocker::Sheet)
    };
    (score, verdict, inside)
}

enum FastEval {
    Valid(Score),
    Invalid,
    Pruned,
}

#[inline]
fn parallel_wire_blocks(
    lo: i32,
    hi: i32,
    fixed: i32,
    horizontal: bool,
    lw: LocalWire,
    own_ends: [Pt; 2],
) -> bool {
    if lo.max(lw.lo) < hi.min(lw.hi) {
        return true;
    }
    if lo <= lw.lo && lw.lo <= hi {
        let p = if horizontal {
            Pt { x: lw.lo, y: fixed }
        } else {
            Pt { x: fixed, y: lw.lo }
        };
        if p != own_ends[0] && p != own_ends[1] {
            return true;
        }
    }
    if lo <= lw.hi && lw.hi <= hi {
        let p = if horizontal {
            Pt { x: lw.hi, y: fixed }
        } else {
            Pt { x: fixed, y: lw.hi }
        };
        if p != own_ends[0] && p != own_ends[1] {
            return true;
        }
    }
    false
}

#[inline]
fn perp_wire_check(
    lo: i32,
    hi: i32,
    fixed: i32,
    horizontal: bool,
    lw: LocalWire,
    own_ends: [Pt; 2],
) -> Result<bool, ()> {
    if lw.lo == fixed || lw.hi == fixed || lw.fixed == lo || lw.fixed == hi {
        let p = if horizontal {
            Pt {
                x: lw.fixed,
                y: fixed,
            }
        } else {
            Pt {
                x: fixed,
                y: lw.fixed,
            }
        };
        if p != own_ends[0] && p != own_ends[1] {
            return Err(());
        }
        return Ok(false);
    }
    Ok(true)
}

/// Validity-first, early-exit evaluation of a valid-shape dogleg against an L1-hot
/// [`LocalObstacles`] snapshot. Returns `FastEval::Valid(score)` iff the dogleg is valid
/// and beats `best_valid_key`, `FastEval::Invalid` on the first blocking obstacle, or
/// `FastEval::Pruned` when the partial score already reaches `best_valid_key`.
fn assess_local_for_valid<S: RouteSheet>(
    sheet: &S,
    local: &mut LocalObstacles,
    wires: &[Wire; 3],
    own_ends: [Pt; 2],
    number: u16,
    length: i64,
    best_valid_key: u128,
) -> FastEval {
    let mut score = Score {
        bodies: 0,
        bends: 2,
        crossings: 0,
        length,
        number,
    };
    for &w in wires {
        let horizontal = w.a.y == w.b.y;
        let area = Rect {
            min: Pt {
                x: w.a.x.min(w.b.x),
                y: w.a.y.min(w.b.y),
            },
            max: Pt {
                x: w.a.x.max(w.b.x),
                y: w.a.y.max(w.b.y),
            },
        };
        if horizontal {
            let y = area.min.y;
            let (x0, x1) = (area.min.x, area.max.x);
            // 1. Reservations along `w` (fastest early-exit).
            if local.res_yx_sorted && local.res_yx.len() > LOCAL_SORT_THRESHOLD {
                let lo = local.res_yx.partition_point(|&(p, _)| (p.y, p.x) < (y, x0));
                for &(p, _) in &local.res_yx[lo..] {
                    if p.y != y || p.x > x1 {
                        break;
                    }
                    if p != own_ends[0] && p != own_ends[1] {
                        return FastEval::Invalid;
                    }
                }
            } else {
                for &(p, _) in &local.res_yx {
                    if p.y == y && x0 <= p.x && p.x <= x1 && p != own_ends[0] && p != own_ends[1] {
                        return FastEval::Invalid;
                    }
                }
            }
            // 2. Parallel horizontal wires.
            if local.wires_h_sorted {
                let lo = local.wires_h.partition_point(|lw| lw.fixed < y);
                for &lw in &local.wires_h[lo..] {
                    if lw.fixed != y || lw.lo > x1 {
                        break;
                    }
                    if lw.hi >= x0 && parallel_wire_blocks(x0, x1, y, true, lw, own_ends) {
                        return FastEval::Invalid;
                    }
                }
            } else {
                for &lw in &local.wires_h {
                    if lw.fixed == y
                        && lw.lo <= x1
                        && lw.hi >= x0
                        && parallel_wire_blocks(x0, x1, y, true, lw, own_ends)
                    {
                        return FastEval::Invalid;
                    }
                }
            }
            // 3. Perpendicular vertical wires.
            if local.wires_v_sorted {
                let lo = local.wires_v.partition_point(|lw| lw.fixed < x0);
                for &lw in &local.wires_v[lo..] {
                    if lw.fixed > x1 {
                        break;
                    }
                    if lw.lo <= y && y <= lw.hi {
                        match perp_wire_check(x0, x1, y, true, lw, own_ends) {
                            Ok(crossed) => score.crossings += u32::from(crossed),
                            Err(()) => return FastEval::Invalid,
                        }
                    }
                }
            } else {
                for &lw in &local.wires_v {
                    if x0 <= lw.fixed && lw.fixed <= x1 && lw.lo <= y && y <= lw.hi {
                        match perp_wire_check(x0, x1, y, true, lw, own_ends) {
                            Ok(crossed) => score.crossings += u32::from(crossed),
                            Err(()) => return FastEval::Invalid,
                        }
                    }
                }
            }
        } else {
            let x = area.min.x;
            let (y0, y1) = (area.min.y, area.max.y);
            // 1. Reservations along `w`.
            if local.res_xy_sorted {
                let lo = local.res_xy.partition_point(|&(p, _)| (p.x, p.y) < (x, y0));
                for &(p, _) in &local.res_xy[lo..] {
                    if p.x != x || p.y > y1 {
                        break;
                    }
                    if p != own_ends[0] && p != own_ends[1] {
                        return FastEval::Invalid;
                    }
                }
            } else {
                for &(p, _) in &local.res_yx {
                    if p.x == x && y0 <= p.y && p.y <= y1 && p != own_ends[0] && p != own_ends[1] {
                        return FastEval::Invalid;
                    }
                }
            }
            // 2. Parallel vertical wires.
            if local.wires_v_sorted {
                let lo = local.wires_v.partition_point(|lw| lw.fixed < x);
                for &lw in &local.wires_v[lo..] {
                    if lw.fixed != x || lw.lo > y1 {
                        break;
                    }
                    if lw.hi >= y0 && parallel_wire_blocks(y0, y1, x, false, lw, own_ends) {
                        return FastEval::Invalid;
                    }
                }
            } else {
                for &lw in &local.wires_v {
                    if lw.fixed == x
                        && lw.lo <= y1
                        && lw.hi >= y0
                        && parallel_wire_blocks(y0, y1, x, false, lw, own_ends)
                    {
                        return FastEval::Invalid;
                    }
                }
            }
            // 3. Perpendicular horizontal wires.
            if local.wires_h_sorted {
                let lo = local.wires_h.partition_point(|lw| lw.fixed < y0);
                for &lw in &local.wires_h[lo..] {
                    if lw.fixed > y1 {
                        break;
                    }
                    if lw.lo <= x && x <= lw.hi {
                        match perp_wire_check(y0, y1, x, false, lw, own_ends) {
                            Ok(crossed) => score.crossings += u32::from(crossed),
                            Err(()) => return FastEval::Invalid,
                        }
                    }
                }
            } else {
                for &lw in &local.wires_h {
                    if y0 <= lw.fixed && lw.fixed <= y1 && lw.lo <= x && x <= lw.hi {
                        match perp_wire_check(y0, y1, x, false, lw, own_ends) {
                            Ok(crossed) => score.crossings += u32::from(crossed),
                            Err(()) => return FastEval::Invalid,
                        }
                    }
                }
            }
        }

        // 4. Parts intersecting `w`: lazy pins and lazy body only on envelope hit.
        let mut through_body = false;
        for part_idx in 0..local.parts.len() {
            let envelope = local.parts[part_idx].envelope;
            if !envelope.intersects(area) {
                continue;
            }
            for &p in local.part_pins(part_idx, sheet) {
                if area.contains(p) && p != own_ends[0] && p != own_ends[1] {
                    return FastEval::Invalid;
                }
            }
            if !through_body
                && passes_through(w, envelope)
                && local
                    .part_body(part_idx, sheet)
                    .is_some_and(|body| passes_through(w, body))
            {
                through_body = true;
            }
        }
        score.bodies += u8::from(through_body);
        if score.key() >= best_valid_key {
            return FastEval::Pruned;
        }
    }
    FastEval::Valid(score)
}

/// Same exact WD4 score and verdict as [`assess`], evaluated against an L1-hot
/// [`LocalObstacles`] snapshot (used on the Stage 2 refusal path).
fn assess_local<S: RouteSheet>(
    sheet: &S,
    local: &mut LocalObstacles,
    wires: &[Wire],
    own_ends: [Pt; 2],
    number: u16,
    judged: bool,
) -> (Score, Option<Blocker>, [bool; 2]) {
    debug_assert_eq!(judged, wires.iter().all(|&w| shape_valid(w)));
    let mut score = Score {
        bends: (wires.len() - 1) as u8,
        number,
        ..Score::default()
    };
    let mut point = None;
    let mut overlap = None;
    let mut inside = [false; 2];
    let last = wires.len() - 1;
    let limit = crate::SHEET_LIMIT_NM;
    for (index, &w) in wires.iter().enumerate() {
        score.length += w.a.manhattan_distance(w.b);
        let horizontal = w.a.y == w.b.y;
        let area = Rect {
            min: Pt {
                x: w.a.x.min(w.b.x).clamp(-limit, limit),
                y: w.a.y.min(w.b.y).clamp(-limit, limit),
            },
            max: Pt {
                x: w.a.x.max(w.b.x).clamp(-limit, limit),
                y: w.a.y.max(w.b.y).clamp(-limit, limit),
            },
        };
        let rules = judged && point.is_none();
        let mut points = rules.then(|| PointRule::new(w, &own_ends));
        let mut overlaps = OverlapRule::new(w);
        let mut through_body = false;

        for part_idx in 0..local.parts.len() {
            let envelope = local.parts[part_idx].envelope;
            if !envelope.intersects(area) {
                continue;
            }
            let need_body = (!through_body && passes_through(w, envelope))
                || (index == 0 && !inside[0] && strictly_inside(envelope, own_ends[0]))
                || (index == last && !inside[1] && strictly_inside(envelope, own_ends[1]));
            if need_body && let Some(body) = local.part_body(part_idx, sheet) {
                through_body |= passes_through(w, body);
                inside[0] |= index == 0 && strictly_inside(body, own_ends[0]);
                inside[1] |= index == last && strictly_inside(body, own_ends[1]);
            }
            if let Some(points) = points.as_mut() {
                let id = local.parts[part_idx].id;
                let pins = local.part_pins(part_idx, sheet);
                points.part(id, pins);
            }
        }

        if horizontal {
            let y = area.min.y;
            let (x0, x1) = (area.min.x, area.max.x);
            if let Some(points) = points.as_mut() {
                if local.wires_h_sorted {
                    let lo = local.wires_h.partition_point(|lw| lw.fixed < y);
                    for lw in &local.wires_h[lo..] {
                        if lw.fixed != y || lw.lo > x1 {
                            break;
                        }
                        if lw.hi >= x0 {
                            points.wire(lw.id, lw.wire);
                            overlaps.wire(lw.id, lw.wire);
                        }
                    }
                } else {
                    for lw in &local.wires_h {
                        if lw.fixed == y && lw.lo <= x1 && lw.hi >= x0 {
                            points.wire(lw.id, lw.wire);
                            overlaps.wire(lw.id, lw.wire);
                        }
                    }
                }
            }
            if local.wires_v_sorted {
                let lo = local.wires_v.partition_point(|lw| lw.fixed < x0);
                for lw in &local.wires_v[lo..] {
                    if lw.fixed > x1 {
                        break;
                    }
                    if lw.lo <= y && y <= lw.hi {
                        score.crossings += u32::from(crosses(w, lw.wire));
                        if let Some(points) = points.as_mut() {
                            points.wire(lw.id, lw.wire);
                        }
                    }
                }
            } else {
                for lw in &local.wires_v {
                    if x0 <= lw.fixed && lw.fixed <= x1 && lw.lo <= y && y <= lw.hi {
                        score.crossings += u32::from(crosses(w, lw.wire));
                        if let Some(points) = points.as_mut() {
                            points.wire(lw.id, lw.wire);
                        }
                    }
                }
            }
            if let Some(points) = points.as_mut() {
                if local.res_yx_sorted && local.res_yx.len() > LOCAL_SORT_THRESHOLD {
                    let lo = local.res_yx.partition_point(|&(p, _)| (p.y, p.x) < (y, x0));
                    for &(p, id) in &local.res_yx[lo..] {
                        if p.y != y || p.x > x1 {
                            break;
                        }
                        points.consider(p, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
                    }
                } else {
                    for &(p, id) in &local.res_yx {
                        if p.y == y && x0 <= p.x && p.x <= x1 {
                            points.consider(p, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
                        }
                    }
                }
            }
        } else {
            let x = area.min.x;
            let (y0, y1) = (area.min.y, area.max.y);
            if let Some(points) = points.as_mut() {
                if local.wires_v_sorted {
                    let lo = local.wires_v.partition_point(|lw| lw.fixed < x);
                    for lw in &local.wires_v[lo..] {
                        if lw.fixed != x || lw.lo > y1 {
                            break;
                        }
                        if lw.hi >= y0 {
                            points.wire(lw.id, lw.wire);
                            overlaps.wire(lw.id, lw.wire);
                        }
                    }
                } else {
                    for lw in &local.wires_v {
                        if lw.fixed == x && lw.lo <= y1 && lw.hi >= y0 {
                            points.wire(lw.id, lw.wire);
                            overlaps.wire(lw.id, lw.wire);
                        }
                    }
                }
            }
            if local.wires_h_sorted {
                let lo = local.wires_h.partition_point(|lw| lw.fixed < y0);
                for lw in &local.wires_h[lo..] {
                    if lw.fixed > y1 {
                        break;
                    }
                    if lw.lo <= x && x <= lw.hi {
                        score.crossings += u32::from(crosses(w, lw.wire));
                        if let Some(points) = points.as_mut() {
                            points.wire(lw.id, lw.wire);
                        }
                    }
                }
            } else {
                for lw in &local.wires_h {
                    if y0 <= lw.fixed && lw.fixed <= y1 && lw.lo <= x && x <= lw.hi {
                        score.crossings += u32::from(crosses(w, lw.wire));
                        if let Some(points) = points.as_mut() {
                            points.wire(lw.id, lw.wire);
                        }
                    }
                }
            }
            if let Some(points) = points.as_mut() {
                if local.res_xy_sorted {
                    let lo = local.res_xy.partition_point(|&(p, _)| (p.x, p.y) < (x, y0));
                    for &(p, id) in &local.res_xy[lo..] {
                        if p.x != x || p.y > y1 {
                            break;
                        }
                        points.consider(p, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
                    }
                } else {
                    for &(p, id) in &local.res_yx {
                        if p.x == x && y0 <= p.y && p.y <= y1 {
                            points.consider(p, 0, id.0, 0, Blocker::Pin { part: id, pin: 0 });
                        }
                    }
                }
            }
        }

        score.bodies += u8::from(through_body);
        if let Some(points) = points {
            point = points.blocker();
            if point.is_none() && overlap.is_none() {
                overlap = overlaps.blocker();
            }
        }
    }
    let verdict = if judged {
        point.or(overlap)
    } else {
        Some(Blocker::Sheet)
    };
    (score, verdict, inside)
}

/// `p` lies in the open interior of `body`: every segment leaving `p` passes through it.
fn strictly_inside(body: Rect, p: Pt) -> bool {
    body.min.x < p.x && p.x < body.max.x && body.min.y < p.y && p.y < body.max.y
}

/// The open segment meets the open rectangle (running along an edge or
/// touching a corner does not count).
fn passes_through(w: Wire, body: Rect) -> bool {
    let (lo, hi, horizontal) = normalised(w);
    if horizontal {
        body.min.y < lo.y && lo.y < body.max.y && lo.x.max(body.min.x) < hi.x.min(body.max.x)
    } else {
        body.min.x < lo.x && lo.x < body.max.x && lo.y.max(body.min.y) < hi.y.min(body.max.y)
    }
}

/// Perpendicular segments meeting at a point interior to both (INV2: not a
/// connection).
fn crosses(w: Wire, other: Wire) -> bool {
    let (lo, hi, horizontal) = normalised(w);
    let (olo, ohi, other_horizontal) = normalised(other);
    if horizontal == other_horizontal {
        return false;
    }
    if horizontal {
        lo.x < olo.x && olo.x < hi.x && olo.y < lo.y && lo.y < ohi.y
    } else {
        lo.y < olo.y && olo.y < hi.y && olo.x < lo.x && lo.x < ohi.x
    }
}

impl Doc {
    /// Debug oracle for INV1/INV2/WINV3/WD9. Uses local index queries and point
    /// incidence; does not compare every wire with every other wire.
    #[cfg(any(debug_assertions, test))]
    pub fn assert_normal_form(&self) {
        let mut scratch = Vec::new();
        for (id, &w) in self.wires() {
            assert!(
                w.a != w.b && (w.a.x == w.b.x || w.a.y == w.b.y),
                "INV1: {id:?}"
            );
            assert!(on_lattice(w.a) && on_lattice(w.b), "WINV3 lattice: {id:?}");
            self.query(Rect::from_points(w.a, w.b).unwrap(), &mut scratch);
            for &item in &scratch {
                match item {
                    ItemId::Wire(other_id) if other_id != id => {
                        let other = *self.wire(other_id).unwrap();
                        assert!(
                            !interior_contains(w, other.a) && !interior_contains(w, other.b),
                            "INV2: {id:?}, {other_id:?}"
                        );
                        assert!(
                            !shared_stretch(w, other),
                            "WINV3 overlap: {id:?}, {other_id:?}"
                        );
                    }
                    ItemId::Part(part) => {
                        for &p in self.pins(part).unwrap() {
                            assert!(!interior_contains(w, p), "WINV3 pin: {id:?}");
                        }
                    }
                    ItemId::Label(label) => assert!(
                        !interior_contains(w, self.label(label).unwrap().at),
                        "WD9 labelled point: {id:?}"
                    ),
                    _ => {}
                }
            }
            for p in [w.a, w.b] {
                if let Some((a, b)) = merge_pair(self, p) {
                    panic!("WINV3 mergeable at {p:?}: {a:?}, {b:?}");
                }
            }
        }
    }
}

// Shared local merge predicate: exactly two ends, no pin/label, opposite
// collinear segments. It never repairs pre-existing overlapping geometry.
fn merge_pair(doc: &Doc, at: Pt) -> Option<(WireId, WireId)> {
    let (a, b) = doc.points.at(at).two_wires()?;
    if !doc.connectivity.labels_at(at).is_empty() {
        return None;
    }
    let wa = *doc.wire(a).unwrap();
    let wb = *doc.wire(b).unwrap();
    if (wa.a.y == wa.b.y) != (wb.a.y == wb.b.y) || shared_stretch(wa, wb) {
        return None;
    }
    Some((a, b))
}

/// W1.4: merge only at the fixed ends vacated by Delete. Union the eligible
/// pairs before editing so multiple removed branches cannot make order matter.
pub(crate) fn plan_delete_merges(doc: &Doc, points: &mut Vec<Pt>) -> AddWiresPlan {
    points.sort_unstable_by_key(|&p| pt_key(p));
    points.dedup();
    let mut groups: FxHashMap<WireId, WireId> = FxHashMap::default();
    let mut originals = BTreeMap::new();
    fn root(groups: &mut FxHashMap<WireId, WireId>, id: WireId) -> WireId {
        let mut r = id;
        while groups[&r] != r {
            r = groups[&r];
        }
        let mut current = id;
        while current != r {
            let next = groups[&current];
            groups.insert(current, r);
            current = next;
        }
        r
    }
    for &p in points.iter() {
        if let Some((a, b)) = merge_pair(doc, p) {
            for id in [a, b] {
                groups.entry(id).or_insert(id);
                originals.insert(id.0, *doc.wire(id).unwrap());
            }
            let ra = root(&mut groups, a);
            let rb = root(&mut groups, b);
            groups.insert(WireId(ra.0.max(rb.0)), WireId(ra.0.min(rb.0)));
        }
    }
    if originals.is_empty() {
        return AddWiresPlan {
            remove: Vec::new(),
            insert: Vec::new(),
        };
    }
    let mut merged: BTreeMap<u32, Wire> = BTreeMap::new();
    for (&id, &w) in &originals {
        let r = root(&mut groups, WireId(id));
        let (lo, hi, _) = normalised(w);
        merged
            .entry(r.0)
            .and_modify(|m| {
                m.a = if pt_key(lo) < pt_key(m.a) { lo } else { m.a };
                m.b = if pt_key(hi) > pt_key(m.b) { hi } else { m.b };
            })
            .or_insert(Wire { a: lo, b: hi });
    }
    AddWiresPlan {
        remove: originals
            .into_iter()
            .map(|(id, w)| (WireId(id), w))
            .collect(),
        insert: merged
            .into_iter()
            .map(|(id, w)| (Some(WireId(id)), w))
            .collect(),
    }
}

fn pt_key(p: Pt) -> (i32, i32) {
    (p.x, p.y)
}

/// Normalised (low, high, horizontal) view of a valid wire.
fn normalised(wire: Wire) -> (Pt, Pt, bool) {
    if wire.a.y == wire.b.y {
        if wire.a.x <= wire.b.x {
            (wire.a, wire.b, true)
        } else {
            (wire.b, wire.a, true)
        }
    } else if wire.a.y <= wire.b.y {
        (wire.a, wire.b, false)
    } else {
        (wire.b, wire.a, false)
    }
}

struct SourceSeg {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Option<WireId>,
    descending: bool,
}

#[derive(Clone)]
struct Atomic {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Vec<WireId>,
    descending: Option<bool>,
}

struct FinalSeg {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Vec<WireId>,
    descending: bool,
}

/// The wire edits an `AddWires` command must perform, in deterministic order.
///
/// `remove` holds every existing wire that execution must take out (wires that
/// disappear and wires whose geometry changes), sorted by id. `insert` holds
/// the final wires to put in, in geometry order; `Some(id)` restores a kept
/// existing id with new geometry, `None` allocates a fresh id. Existing wires
/// absent from both are untouched and keep their ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddWiresPlan {
    pub remove: Vec<(WireId, Wire)>,
    pub insert: Vec<(Option<WireId>, Wire)>,
}

fn contains(wire: Wire, p: Pt) -> bool {
    Rect::from_points(wire.a, wire.b).unwrap().contains(p)
}

fn touches(a: Wire, b: Wire) -> bool {
    contains(a, b.a) || contains(a, b.b) || contains(b, a.a) || contains(b, a.b)
}

/// Plans only the added wires' local neighbourhood (D8, INV2).
/// Direct contacts have an endpoint on the other segment (inclusive); a pure
/// interior crossing is not a contact. Touched points are the ends of these
/// direct contacts and the added wires, plus pins/labels on added wires.
/// Collinear neighbours incident at those fixed points may join the plan, but
/// their far ends do not expand the touched points. Normalisation never walks
/// an unrelated chain. Ends outside the plan still count as merge blockers.
/// Pre-existing non-normal geometry elsewhere is left unchanged (W2.2 owns
/// normalising moved geometry; this is not a document-wide repair pass).
pub(crate) fn plan_add_wires(doc: &Doc, added: &[Wire]) -> AddWiresPlan {
    plan_wires_with_replacements(
        doc,
        &[],
        &added.iter().map(|&w| (None, w)).collect::<Vec<_>>(),
    )
}

/// Fast path for [`plan_add_wires`] when every wire in `added` was validated by
/// `plan_follow` against the final-geometry sheet (`selected_wires` empty, no moved
/// pin/label landing on any existing wire, and zero labels on the sheet).
///
/// Verifies that no endpoint merges and that no existing wire meeting an endpoint of
/// `added` splits, overlaps, or merges; falls back to [`plan_add_wires`] otherwise.
pub(crate) fn plan_follow_add_wires(doc: &Doc, added: &[Wire]) -> AddWiresPlan {
    if added.is_empty() {
        return AddWiresPlan {
            remove: Vec::new(),
            insert: Vec::new(),
        };
    }
    let mut ends: Vec<((i32, i32), bool)> = Vec::with_capacity(added.len() * 2);
    for &w in added {
        let horizontal = w.a.y == w.b.y;
        ends.push((pt_key(w.a), horizontal));
        ends.push((pt_key(w.b), horizontal));
    }
    ends.sort_unstable();

    let mut fallback = false;
    let mut unique_xy: Vec<(i32, i32)> = Vec::with_capacity(ends.len());
    let mut i = 0;
    while i < ends.len() && !fallback {
        let key = ends[i].0;
        unique_xy.push(key);
        let p = Pt { x: key.0, y: key.1 };
        let start = i;
        let mut h_count = 0usize;
        let mut v_count = 0usize;
        while i < ends.len() && ends[i].0 == key {
            if ends[i].1 {
                h_count += 1;
            } else {
                v_count += 1;
            }
            i += 1;
        }
        let add_count = i - start;
        let mut existing_wires = 0usize;
        let mut existing_h = 0usize;
        let mut existing_v = 0usize;
        let area = Rect { min: p, max: p };
        // Single-point query visits exactly one K12 cell, so each wire meeting `p` is
        // seen once; we only query `doc.points.at(p)` if a collinear 2-wire merge is possible.
        doc.index.visit_wires_unstamped(area, |id, bounds| {
            if fallback {
                return;
            }
            if bounds.min != p && bounds.max != p {
                fallback = true;
                return;
            }
            existing_wires += 1;
            if bounds.min.y == bounds.max.y {
                existing_h += 1;
            } else {
                existing_v += 1;
            }
            let e = Wire {
                a: bounds.min,
                b: bounds.max,
            };
            let far = if e.a == p { e.b } else { e.a };
            if merge_pair(doc, far).is_some() {
                fallback = true;
                return;
            }
            doc.index
                .visit_parts_and_wires_unstamped(bounds, |other_item, other_bounds| {
                    if fallback {
                        return;
                    }
                    match other_item {
                        ItemId::Part(pid) => {
                            if doc
                                .pins(pid)
                                .unwrap()
                                .iter()
                                .any(|&pin| interior_contains(e, pin))
                            {
                                fallback = true;
                            }
                        }
                        ItemId::Wire(oid) if oid != id => {
                            let other = Wire {
                                a: other_bounds.min,
                                b: other_bounds.max,
                            };
                            if interior_contains(e, other.a)
                                || interior_contains(e, other.b)
                                || interior_contains(other, e.a)
                                || interior_contains(other, e.b)
                                || shared_stretch(e, other)
                            {
                                fallback = true;
                            }
                        }
                        ItemId::Wire(_) | ItemId::Label(_) => {}
                    }
                });
        });
        if fallback {
            break;
        }
        if add_count + existing_wires == 2
            && (h_count + existing_h == 2 || v_count + existing_v == 2)
            && !doc
                .points
                .at(p)
                .iter()
                .any(|e| matches!(e, End::Pin { .. }))
        {
            fallback = true;
            break;
        }
    }

    if !fallback {
        let mut unique_yx: Vec<(i32, i32)> = unique_xy.iter().map(|&(x, y)| (y, x)).collect();
        unique_yx.sort_unstable();
        for &w in added {
            let (lo, hi, horizontal) = normalised(w);
            if horizontal {
                let idx = unique_yx.partition_point(|&(y, x)| (y, x) <= (lo.y, lo.x));
                if let Some(&(y, x)) = unique_yx.get(idx)
                    && y == lo.y
                    && x < hi.x
                {
                    fallback = true;
                    break;
                }
            } else {
                let idx = unique_xy.partition_point(|&(x, y)| (x, y) <= (lo.x, lo.y));
                if let Some(&(x, y)) = unique_xy.get(idx)
                    && x == lo.x
                    && y < hi.y
                {
                    fallback = true;
                    break;
                }
            }
        }
    }

    if fallback {
        return plan_add_wires(doc, added);
    }

    let mut sorted: Vec<Wire> = added.to_vec();
    sorted.sort_unstable_by(|&lhs, &rhs| {
        let (llo, lhi, lh) = normalised(lhs);
        let (rlo, rhi, rh) = normalised(rhs);
        (pt_key(llo), pt_key(lhi), lh).cmp(&(pt_key(rlo), pt_key(rhi), rh))
    });
    AddWiresPlan {
        remove: Vec::new(),
        insert: sorted.into_iter().map(|w| (None, w)).collect(),
    }
}

/// WD7 preview and execution plan for dragging one wire segment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentDragPlan {
    /// Existing wire ids removed or replaced by the drag, sorted by id.
    pub remove: Vec<WireId>,
    /// Final normalised wire segments after the drag, in geometry order.
    pub add: Vec<Wire>,
}

/// WD7: pure planner for dragging `wire` by `offset` nm perpendicular to its axis.
///
/// Positive `offset` moves along `+y` for a horizontal segment and `+x` for a
/// vertical segment. A corner neighbour (a perpendicular wire meeting at a
/// 2-wire corner with no pin and no label) stretches or shrinks to stay
/// attached; shrinking to or past zero length stops at zero and removes that
/// neighbour (TC-seg-04). A pin, junction, or labelled corner stays fixed and
/// inserts a perpendicular jog segment; a free end moves with the segment.
/// Every changed segment is validated with [`check_new_wires`] (ignoring the
/// replaced segments), and the result is locally normalised (WINV3).
///
/// Missing `wire` or `offset == 0` leaves `plan` empty and returns `Ok(())`.
pub fn plan_segment_drag(
    doc: &Doc,
    wire: WireId,
    offset: i32,
    plan: &mut SegmentDragPlan,
) -> Result<(), Blocker> {
    plan.remove.clear();
    plan.add.clear();
    let Some(exec) = plan_segment_drag_inner(doc, wire, offset)? else {
        return Ok(());
    };
    plan.remove.extend(exec.remove.iter().map(|&(id, _)| id));
    plan.add.extend(exec.insert.iter().map(|&(_, w)| w));
    Ok(())
}

pub(crate) fn plan_segment_drag_inner(
    doc: &Doc,
    wire: WireId,
    offset: i32,
) -> Result<Option<AddWiresPlan>, Blocker> {
    let Some(&seg) = doc.wire(wire) else {
        return Ok(None);
    };
    if offset == 0 {
        return Ok(None);
    }
    // Validate the requested displacement before zero-length clamping can change it.
    if offset % crate::PIN_LATTICE_NM != 0 {
        return Err(Blocker::Sheet);
    }
    let horizontal = seg.a.y == seg.b.y;
    let shift = |p: Pt, delta: i32| -> Result<Pt, Blocker> {
        let (x, y) = if horizontal {
            (i64::from(p.x), i64::from(p.y) + i64::from(delta))
        } else {
            (i64::from(p.x) + i64::from(delta), i64::from(p.y))
        };
        let x = i32::try_from(x).map_err(|_| Blocker::Sheet)?;
        let y = i32::try_from(y).map_err(|_| Blocker::Sheet)?;
        let shifted = Pt { x, y };
        if on_lattice(shifted) {
            Ok(shifted)
        } else {
            Err(Blocker::Sheet)
        }
    };

    #[derive(Clone, Copy)]
    enum EndKind {
        Free,
        Anchor,
        Corner {
            id: WireId,
            wire: Wire,
            far: Pt,
            has_label: bool,
        },
    }

    let classify_end = |at: Pt| -> EndKind {
        let ends = doc.points.at(at);
        if ends.iter().any(|e| matches!(e, End::Pin { .. })) || ends.len() >= 3 {
            return EndKind::Anchor;
        }
        let has_label = !doc.connectivity.labels_at(at).is_empty();
        if let Some((x, y)) = ends.two_wires() {
            let other_id = if x == wire { y } else { x };
            let other = *doc.wire(other_id).unwrap();
            let other_horiz = other.a.y == other.b.y;
            if other_horiz != horizontal {
                let far = if other.a == at { other.b } else { other.a };
                return EndKind::Corner {
                    id: other_id,
                    wire: other,
                    far,
                    has_label,
                };
            }
            return EndKind::Anchor;
        }
        if has_label {
            return EndKind::Anchor;
        }
        EndKind::Free
    };

    let end_a = classify_end(seg.a);
    let end_b = classify_end(seg.b);

    let signed_dist = |from: Pt, to: Pt| -> i64 {
        if horizontal {
            i64::from(to.y) - i64::from(from.y)
        } else {
            i64::from(to.x) - i64::from(from.x)
        }
    };

    // WD7: a stretch that would shrink a neighbour past zero length stops at zero.
    let mut effective = i64::from(offset);
    for end in [end_a, end_b] {
        if let EndKind::Corner { far, .. } = end {
            let d = signed_dist(seg.a, far);
            if (effective > 0 && d > 0 && effective > d)
                || (effective < 0 && d < 0 && effective < d)
            {
                effective = d;
            }
        }
    }
    let effective = i32::try_from(effective).map_err(|_| Blocker::Sheet)?;

    let new_a = shift(seg.a, effective)?;
    let new_b = shift(seg.b, effective)?;

    let mut replaced: Vec<(WireId, Wire)> = vec![(wire, seg)];
    let mut ignore_ids: Vec<WireId> = vec![wire];
    for end in [end_a, end_b] {
        if let EndKind::Corner { id, wire: w, .. } = end
            && !ignore_ids.contains(&id)
        {
            replaced.push((id, w));
            ignore_ids.push(id);
        }
    }

    // Build the changed segments in deterministic walk order from a's side to b's side.
    let mut check_wires: Vec<Wire> = Vec::with_capacity(3);
    let mut placed_wires: Vec<(Option<WireId>, Wire)> = Vec::with_capacity(3);
    let mut own_ends: Vec<Pt> = Vec::with_capacity(2);

    match end_a {
        EndKind::Free => {}
        EndKind::Anchor => {
            own_ends.push(seg.a);
            let jog = Wire { a: seg.a, b: new_a };
            check_wires.push(jog);
            placed_wires.push((None, jog));
        }
        EndKind::Corner {
            id,
            wire: w,
            far,
            has_label,
        } => {
            own_ends.push(far);
            if far != new_a {
                check_wires.push(Wire { a: far, b: new_a });
                let preserved = if w.a == seg.a {
                    Wire { a: new_a, b: far }
                } else {
                    Wire { a: far, b: new_a }
                };
                placed_wires.push((Some(id), preserved));
            }
            if has_label && !contains(Wire { a: far, b: new_a }, seg.a) {
                // Shrinking leaves the old label beyond the shortened neighbour.
                // Keep both the far leg and a new jog back to that label (WD7/WD9).
                let jog = Wire { a: new_a, b: seg.a };
                check_wires.push(jog);
                placed_wires.push((None, jog));
            }
        }
    }

    let moved_seg = Wire { a: new_a, b: new_b };
    check_wires.push(moved_seg);
    placed_wires.push((Some(wire), moved_seg));

    match end_b {
        EndKind::Free => {}
        EndKind::Anchor => {
            own_ends.push(seg.b);
            let jog = Wire { a: new_b, b: seg.b };
            check_wires.push(jog);
            placed_wires.push((None, jog));
        }
        EndKind::Corner {
            id,
            wire: w,
            far,
            has_label,
        } => {
            own_ends.push(far);
            if far != new_b {
                check_wires.push(Wire { a: new_b, b: far });
                let preserved = if w.a == seg.b {
                    Wire { a: new_b, b: far }
                } else {
                    Wire { a: far, b: new_b }
                };
                placed_wires.push((Some(id), preserved));
            }
            if has_label && !contains(Wire { a: far, b: new_b }, seg.b) {
                // Shrinking leaves the old label beyond the shortened neighbour.
                // Keep both the far leg and a new jog back to that label (WD7/WD9).
                let jog = Wire { a: new_b, b: seg.b };
                check_wires.push(jog);
                placed_wires.push((None, jog));
            }
        }
    }

    check_new_wires_ignoring(doc, &check_wires, &own_ends, &ignore_ids)?;

    Ok(Some(plan_wires_with_replacements(
        doc,
        &replaced,
        &placed_wires,
    )))
}

fn plan_wires_with_replacements(
    doc: &Doc,
    replaced: &[(WireId, Wire)],
    added: &[(Option<WireId>, Wire)],
) -> AddWiresPlan {
    let mut selected: FxHashMap<WireId, Wire> = replaced.iter().copied().collect();
    let mut pins = Vec::new();
    let mut labels = Vec::new();
    let mut scratch = Vec::new();
    let mut touched = FxHashSet::default();
    for &(_, wire) in replaced {
        touched.extend([wire.a, wire.b]);
    }
    for &(_, wire) in added {
        touched.extend([wire.a, wire.b]);
        doc.query(Rect::from_points(wire.a, wire.b).unwrap(), &mut scratch);
        for &item in &scratch {
            match item {
                ItemId::Wire(id) => {
                    if selected.contains_key(&id) {
                        continue;
                    }
                    let other = *doc.wire(id).unwrap();
                    if touches(wire, other) {
                        selected.insert(id, other);
                        touched.extend([other.a, other.b]);
                    }
                }
                ItemId::Part(id) => {
                    pins.extend(
                        doc.pins(id)
                            .unwrap()
                            .iter()
                            .copied()
                            .filter(|&p| contains(wire, p)),
                    );
                }
                ItemId::Label(id) => labels.push(doc.label(id).unwrap().at),
            }
        }
    }
    touched.extend(pins.iter().copied());
    touched.extend(labels.iter().copied());
    // One boundary expansion at fixed touched points, using the point store.
    // Only collinear neighbours can merge. Perpendicular untouched ends remain
    // blockers, without being copied or replanned.
    let mut endpoint_axes = FxHashSet::default();
    for (&id, &wire) in &selected {
        if replaced.iter().any(|&(rid, _)| rid == id) {
            continue;
        }
        let horizontal = wire.a.y == wire.b.y;
        endpoint_axes.extend([(wire.a, horizontal), (wire.b, horizontal)]);
    }
    for &(_, wire) in added {
        let horizontal = wire.a.y == wire.b.y;
        endpoint_axes.extend([(wire.a, horizontal), (wire.b, horizontal)]);
    }
    let mut neighbours = Vec::new();
    for &at in &touched {
        for end in doc.points.at(at) {
            if let End::Wire(id) = end {
                if selected.contains_key(&id) {
                    continue;
                }
                let wire = *doc.wire(id).unwrap();
                if endpoint_axes.contains(&(at, wire.a.y == wire.b.y)) {
                    neighbours.push((id, wire));
                }
            }
        }
        // Vacated points from removed/shrunk segments may leave two surviving
        // collinear wires meeting with no pin or label (analogous to Delete).
        if doc.connectivity.labels_at(at).is_empty()
            && !doc
                .points
                .at(at)
                .iter()
                .any(|e| matches!(e, End::Pin { .. }))
        {
            let mut surviving = doc.points.at(at).iter().filter_map(|e| match e {
                End::Wire(id) if !replaced.iter().any(|&(rid, _)| rid == id) => Some(id),
                _ => None,
            });
            if let (Some(a), Some(b), None) = (surviving.next(), surviving.next(), surviving.next())
            {
                let wa = *doc.wire(a).unwrap();
                let wb = *doc.wire(b).unwrap();
                if (wa.a.y == wa.b.y) == (wb.a.y == wb.b.y) && !shared_stretch(wa, wb) {
                    neighbours.push((a, wa));
                    neighbours.push((b, wb));
                }
            }
        }
    }
    selected.extend(neighbours);
    let mut existing_sorted: Vec<_> = selected.iter().map(|(&id, &wire)| (id, wire)).collect();
    existing_sorted.sort_by_key(|(id, _)| id.0);
    // Pins/labels at retained outer ends also prevent merging, even when those
    // points lie outside the new segments. Query points, never the whole sheet.
    for &at in &touched {
        if doc
            .points
            .at(at)
            .iter()
            .any(|end| matches!(end, End::Pin { .. }))
        {
            pins.push(at);
        }
        // Labels are indexed at their anchor point, which is where the
        // connectivity keeps them.
        if !doc.connectivity.labels_at(at).is_empty() {
            labels.push(at);
        }
    }
    // Canonical added order: geometry first, so fresh ids do not depend on the
    // caller's wire order.
    let mut added_sorted: Vec<(Option<WireId>, Wire)> = added.to_vec();
    added_sorted.sort_by(|&(lid, lhs), &(rid, rhs)| {
        let (llo, lhi, _) = normalised(lhs);
        let (rlo, rhi, _) = normalised(rhs);
        (
            pt_key(llo),
            pt_key(lhi),
            pt_key(lhs.a),
            pt_key(lhs.b),
            lid.map(|id| id.0),
        )
            .cmp(&(
                pt_key(rlo),
                pt_key(rhi),
                pt_key(rhs.a),
                pt_key(rhs.b),
                rid.map(|id| id.0),
            ))
    });

    let mut sources: Vec<SourceSeg> =
        Vec::with_capacity(existing_sorted.len() + added_sorted.len());
    for &(id, wire) in &existing_sorted {
        if replaced.iter().any(|&(rid, _)| rid == id) {
            continue;
        }
        let (lo, hi, horizontal) = normalised(wire);
        sources.push(SourceSeg {
            lo,
            hi,
            horizontal,
            existing: Some(id),
            descending: wire.a != lo,
        });
    }
    for &(keep, wire) in added_sorted.iter() {
        let (lo, hi, horizontal) = normalised(wire);
        sources.push(SourceSeg {
            lo,
            hi,
            horizontal,
            existing: keep,
            descending: wire.a != lo,
        });
    }

    // Feature points: every segment end, pin and label point. A segment is
    // split at every feature point in its interior (INV2, WD9).
    let mut feature: Vec<Pt> = Vec::new();
    for source in &sources {
        feature.push(source.lo);
        feature.push(source.hi);
    }
    feature.extend_from_slice(&pins);
    feature.extend_from_slice(&labels);
    feature.sort_unstable_by_key(|&p| pt_key(p));
    feature.dedup();

    // Feature points along each line, as (fixed, along) pairs sorted without
    // repeats: columns by (x, y), rows by (y, x).
    let cols: Vec<(i32, i32)> = feature.iter().map(|p| (p.x, p.y)).collect();
    let mut rows: Vec<(i32, i32)> = feature.iter().map(|p| (p.y, p.x)).collect();
    rows.sort_unstable();

    // Atomic pieces: maximal sub-segments free of interior feature points.
    // Identical pieces from overlapping wires are deduplicated here, so no two
    // final wires can share a stretch.
    let mut atomic_index: FxHashMap<(u8, i32, i32, i32), usize> = FxHashMap::default();
    let mut atomics: Vec<Atomic> = Vec::new();
    for source in &sources {
        let (line, fixed, low, high) = if source.horizontal {
            (&rows, source.lo.y, source.lo.x, source.hi.x)
        } else {
            (&cols, source.lo.x, source.lo.y, source.hi.y)
        };
        // Feature coordinates strictly between the source's ends, in order.
        let start = line.partition_point(|&c| c <= (fixed, low));
        let end = line.partition_point(|&c| c < (fixed, high));
        let mut lo = source.lo;
        for coordinate in line[start..end]
            .iter()
            .map(|&(_, c)| c)
            .chain(std::iter::once(high))
        {
            let hi = if source.horizontal {
                Pt {
                    x: coordinate,
                    y: source.lo.y,
                }
            } else {
                Pt {
                    x: source.lo.x,
                    y: coordinate,
                }
            };
            let key = (
                u8::from(!source.horizontal),
                if source.horizontal { lo.y } else { lo.x },
                if source.horizontal { lo.x } else { lo.y },
                if source.horizontal { hi.x } else { hi.y },
            );
            let idx = *atomic_index.entry(key).or_insert_with(|| {
                atomics.push(Atomic {
                    lo,
                    hi,
                    horizontal: source.horizontal,
                    existing: Vec::new(),
                    descending: Some(source.descending),
                });
                atomics.len() - 1
            });
            let atomic = &mut atomics[idx];
            if let Some(id) = source.existing {
                atomic.existing.push(id);
            }
            if atomic.descending != Some(source.descending) {
                atomic.descending = None;
            }
            lo = hi;
        }
    }

    // Ends per point over the deduplicated pieces: a merge point must hold
    // exactly the two ends being joined, and no pin or label.
    let mut incidence: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for atomic in &atomics {
        *incidence.entry(pt_key(atomic.lo)).or_default() += 1;
        *incidence.entry(pt_key(atomic.hi)).or_default() += 1;
    }
    let mut pin_keys: Vec<(i32, i32)> = pins.iter().map(|&p| pt_key(p)).collect();
    pin_keys.sort_unstable();
    pin_keys.dedup();
    let mut label_keys: Vec<(i32, i32)> = labels.iter().map(|&p| pt_key(p)).collect();
    label_keys.sort_unstable();
    label_keys.dedup();
    let mergeable = |at: Pt| {
        touched.contains(&at)
            && incidence.get(&pt_key(at)).copied().unwrap_or(0)
                + doc
                    .points
                    .at(at)
                    .iter()
                    .filter(|end| matches!(end, End::Wire(id) if !selected.contains_key(id)))
                    .count() as u32
                == 2
            && pin_keys.binary_search(&pt_key(at)).is_err()
            && label_keys.binary_search(&pt_key(at)).is_err()
    };

    // Merge collinear neighbours along each line, farthest first by geometry.
    let mut line_items: Vec<(u8, i32, i32, usize)> = Vec::with_capacity(atomics.len());
    for (idx, atomic) in atomics.iter().enumerate() {
        let (fixed, along) = if atomic.horizontal {
            (atomic.lo.y, atomic.lo.x)
        } else {
            (atomic.lo.x, atomic.lo.y)
        };
        line_items.push((u8::from(!atomic.horizontal), fixed, along, idx));
    }
    line_items.sort_unstable();
    let mut finals: Vec<FinalSeg> = Vec::with_capacity(atomics.len());
    let mut run: Vec<usize> = Vec::new();
    let finish_run = |run: &mut Vec<usize>, finals: &mut Vec<FinalSeg>| {
        if run.is_empty() {
            return;
        }
        let first = &atomics[run[0]];
        let last = &atomics[*run.last().unwrap()];
        let mut seg_existing: Vec<WireId> = Vec::new();
        let mut descending = atomics[run[0]].descending;
        for &idx in run.iter() {
            seg_existing.extend_from_slice(&atomics[idx].existing);
            if atomics[idx].descending != descending {
                descending = None;
            }
        }
        seg_existing.sort_unstable_by_key(|id| id.0);
        seg_existing.dedup();
        finals.push(FinalSeg {
            lo: first.lo,
            hi: last.hi,
            horizontal: first.horizontal,
            existing: seg_existing,
            descending: descending.unwrap_or(false),
        });
        run.clear();
    };
    let mut group_start = 0;
    while group_start < line_items.len() {
        let (orient, fixed, _, _) = line_items[group_start];
        let mut group_end = group_start + 1;
        while group_end < line_items.len()
            && line_items[group_end].0 == orient
            && line_items[group_end].1 == fixed
        {
            group_end += 1;
        }
        for &(_, _, _, idx) in &line_items[group_start..group_end] {
            if let Some(&last_idx) = run.last() {
                let junction = atomics[last_idx].hi;
                if atomics[idx].lo != junction || !mergeable(junction) {
                    finish_run(&mut run, &mut finals);
                }
            }
            run.push(idx);
        }
        finish_run(&mut run, &mut finals);
        group_start = group_end;
    }

    // Keep the lowest surviving existing id per final segment; the first final
    // segment (in geometry order) wins a split wire's id.
    finals.sort_unstable_by(|lhs, rhs| {
        (pt_key(lhs.lo), pt_key(lhs.hi), lhs.horizontal).cmp(&(
            pt_key(rhs.lo),
            pt_key(rhs.hi),
            rhs.horizontal,
        ))
    });
    let mut used = FxHashSet::default();
    let mut remove: Vec<(WireId, Wire)> = Vec::new();
    let mut insert: Vec<(Option<WireId>, Wire)> = Vec::new();
    for seg in &finals {
        let wire = if seg.descending {
            Wire {
                a: seg.hi,
                b: seg.lo,
            }
        } else {
            Wire {
                a: seg.lo,
                b: seg.hi,
            }
        };
        let keep = seg.existing.iter().copied().find(|id| !used.contains(id));
        match keep {
            Some(id) => {
                used.insert(id);
                let pos = existing_sorted
                    .binary_search_by_key(&id.0, |(eid, _)| eid.0)
                    .expect("kept existing wire is in existing_sorted");
                let old = existing_sorted[pos].1;
                let (old_lo, old_hi, _) = normalised(old);
                if old_lo != seg.lo || old_hi != seg.hi {
                    remove.push((id, old));
                    insert.push((Some(id), wire));
                }
            }
            None => insert.push((None, wire)),
        }
    }
    for &(id, wire) in &existing_sorted {
        if !used.contains(&id) {
            remove.push((id, wire));
        }
    }
    remove.sort_unstable_by_key(|(id, _)| id.0);
    AddWiresPlan { remove, insert }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Connectivity, Doc, Label, Orient, Part, Rect, SymbolId};
    use proptest::prelude::*;

    fn w(x1: i32, y1: i32, x2: i32, y2: i32) -> Wire {
        Wire {
            a: Pt { x: x1, y: y1 },
            b: Pt { x: x2, y: y2 },
        }
    }

    fn live_wires(doc: &Doc) -> Vec<(WireId, Wire)> {
        doc.wires().map(|(id, &wire)| (id, wire)).collect()
    }

    fn interior_contains(wire: Wire, p: Pt) -> bool {
        if wire.a.y == wire.b.y {
            p.y == wire.a.y && p.x > wire.a.x.min(wire.b.x) && p.x < wire.a.x.max(wire.b.x)
        } else {
            p.x == wire.a.x && p.y > wire.a.y.min(wire.b.y) && p.y < wire.a.y.max(wire.b.y)
        }
    }

    /// INV2: no wire end lies in another wire's interior.
    fn assert_inv2(doc: &Doc) {
        let wires = live_wires(doc);
        for &(id, wire) in &wires {
            for &(other_id, other) in &wires {
                if id != other_id {
                    assert!(
                        !interior_contains(other, wire.a) && !interior_contains(other, wire.b),
                        "INV2: end of {id:?} {wire:?} lies in interior of {other_id:?} {other:?}"
                    );
                }
            }
        }
    }

    /// WINV3 wire half: no shared stretch; no mergeable collinear meeting.
    fn assert_wire_normal_form(doc: &Doc) {
        let wires = live_wires(doc);
        for (i, &(_, a)) in wires.iter().enumerate() {
            for &(_, b) in &wires[i + 1..] {
                let (alo, ahi, ah) = normalised(a);
                let (blo, bhi, bh) = normalised(b);
                if ah == bh {
                    if ah && alo.y == blo.y {
                        assert!(
                            ahi.x <= blo.x || bhi.x <= alo.x,
                            "shared stretch: {a:?} {b:?}"
                        );
                    }
                    if !ah && alo.x == blo.x {
                        assert!(
                            ahi.y <= blo.y || bhi.y <= alo.y,
                            "shared stretch: {a:?} {b:?}"
                        );
                    }
                }
            }
        }
        // A point with exactly two wire ends, no pin and no label, must not
        // join two collinear segments (they should have merged).
        let mut ends: Vec<(Pt, WireId, Wire)> = Vec::new();
        for &(id, wire) in &wires {
            ends.push((wire.a, id, wire));
            ends.push((wire.b, id, wire));
        }
        let pins: Vec<Pt> = doc
            .parts()
            .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec())
            .collect();
        let labels: Vec<Pt> = doc.labels().map(|(_, label)| label.at).collect();
        for &(at, _, _) in &ends {
            let here: Vec<&(Pt, WireId, Wire)> = ends.iter().filter(|(p, _, _)| *p == at).collect();
            if here.len() == 2 && !pins.contains(&at) && !labels.contains(&at) {
                let first = here[0].2;
                let second = here[1].2;
                let collinear = (first.a.y == first.b.y && second.a.y == second.b.y)
                    || (first.a.x == first.b.x && second.a.x == second.b.x);
                assert!(
                    !collinear,
                    "mergeable collinear segments meet at {at:?}: {first:?} {second:?}"
                );
            }
        }
    }

    fn assert_inv10(doc: &Doc) {
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &wire)| (id, wire)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        crate::connectivity::tests::assert_partitions(&doc.connectivity, &oracle);
    }

    fn assert_all(doc: &Doc) {
        assert_inv2(doc);
        assert_wire_normal_form(doc);
        assert_inv10(doc);
    }

    #[test]
    fn l_route_corners_and_degenerate_cases() {
        let a = Pt { x: 0, y: 0 };
        let b = Pt { x: 10, y: 20 };
        assert_eq!(l_route(a, a, false), [None, None]);
        assert_eq!(
            l_route(a, Pt { x: 10, y: 0 }, true),
            [Some(w(0, 0, 10, 0)), None]
        );
        assert_eq!(
            l_route(a, b, false),
            [Some(w(0, 0, 10, 0)), Some(w(10, 0, 10, 20))],
            "flip=false leaves a horizontally"
        );
        assert_eq!(
            l_route(a, b, true),
            [Some(w(0, 0, 0, 20)), Some(w(0, 20, 10, 20))],
            "flip=true leaves a vertically"
        );
    }

    #[test]
    fn inv2_wire_ending_on_body_splits_and_joins_net() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        let before = doc
            .wires()
            .map(|(id, &wire)| (id, wire))
            .collect::<Vec<_>>();
        assert_eq!(before, vec![(WireId(0), w(0, 0, 10, 0))]);
        doc.apply(Command::AddWires(vec![w(5, 0, 5, 10)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);
        // The split keeps the original id on the first piece (INV4 helper).
        assert_eq!(doc.wire(WireId(0)), Some(&w(0, 0, 5, 0)));
        let net = doc.net_of(WireId(0)).unwrap();
        let mut net_wires = Vec::new();
        doc.net_wires(net, &mut net_wires);
        assert_eq!(net_wires.len(), 3, "a wire ending on a body joins its net");
        // INV4: undo restores the pre-split wire with its id; redo re-splits.
        doc.undo().unwrap();
        assert_eq!(live_wires(&doc), before);
        assert_all(&doc);
        doc.redo().unwrap();
        assert_eq!(doc.wires().count(), 3);
        assert_all(&doc);
    }

    #[test]
    fn inv2_crossing_interiors_never_connect() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, -5, 5, 5)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 2, "crossing interiors are not split");
        assert_ne!(
            doc.net_of(WireId(0)),
            doc.net_of(WireId(1)),
            "two wires crossing at interiors are in different nets (INV2)"
        );
    }

    #[test]
    fn added_wire_splits_over_existing_end_and_pin() {
        // Over an existing end.
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(5, 0, 5, 10)])).unwrap();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);
        assert_eq!(
            doc.net_of(WireId(0)),
            doc.net_of(WireId(1)),
            "passing over an existing end connects there"
        );

        // Over a pin.
        let mut doc = Doc::new();
        doc.apply(Command::PlacePart {
            part: Part {
                symbol: SymbolId(0),
                at: Pt { x: 5, y: 0 },
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "1k".into(),
            },
            local_pins: vec![Pt::ZERO],
            local_bounds: Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            def: crate::PlaceDef::Unmodelled,
            params: crate::Params::new(),
        })
        .unwrap();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 2, "the added wire splits at the pin");
        assert!(doc.net_at(Pt { x: 5, y: 0 }).is_some());
        let first = doc.wires().next().unwrap().0;
        assert_eq!(doc.net_of(first), doc.net_at(Pt { x: 5, y: 0 }));
    }

    #[test]
    fn collinear_segments_merge_and_undo_restores_ids() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 5, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(
            live_wires(&doc),
            vec![(WireId(0), w(0, 0, 10, 0))],
            "merged segment keeps the lowest id"
        );
        doc.undo().unwrap();
        assert_eq!(live_wires(&doc), vec![(WireId(0), w(0, 0, 5, 0))]);
        doc.undo().unwrap();
        assert!(doc.wires().next().is_none());

        // No merge across a junction (a third end at the meeting point).
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 0, 5, 10)]))
            .unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);

        // No merge across a labelled point (WD9; the W1.4 predicate).
        let mut doc = Doc::from_items(
            [],
            [w(0, 0, 5, 0), w(5, 0, 10, 0)],
            [Label {
                at: Pt { x: 5, y: 0 },
                text: "SIG".into(),
            }],
        )
        .unwrap();
        doc.apply(Command::AddWires(vec![w(20, 0, 25, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wire(WireId(0)), Some(&w(0, 0, 5, 0)));
        assert_eq!(doc.wire(WireId(1)), Some(&w(5, 0, 10, 0)));
    }

    #[test]
    fn overlapping_wires_unite_without_shared_stretch() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, 0, 15, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(live_wires(&doc), vec![(WireId(0), w(0, 0, 15, 0))]);
        // Re-adding an existing wire is a validated no-op.
        let revision = doc.revision();
        let change = doc.apply(Command::AddWires(vec![w(0, 0, 15, 0)])).unwrap();
        assert_eq!(change.dirty, None);
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn add_wires_result_does_not_depend_on_wire_order() {
        let wires = vec![w(0, 0, 10, 0), w(5, 0, 5, 10), w(5, 10, 15, 10)];
        let mut first = Doc::new();
        first.apply(Command::AddWires(wires.clone())).unwrap();
        let mut second = Doc::new();
        second
            .apply(Command::AddWires(vec![wires[2], wires[0], wires[1]]))
            .unwrap();
        assert!(first.content_eq(&second));
        assert_eq!(live_wires(&first), live_wires(&second));
        assert_all(&first);
    }

    fn snapshot(doc: &Doc) -> Doc {
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        Doc::load(bytes.as_slice()).unwrap()
    }

    fn net_members(doc: &Doc, id: WireId) -> Vec<WireId> {
        let mut wires = Vec::new();
        doc.net_wires(doc.net_of(id).unwrap(), &mut wires);
        wires.sort_by_key(|id| id.0);
        wires
    }

    // INV4 / INV10: unrelated AddWires must not repair legacy non-normal
    // geometry, change its exact ids/directions, or change its net partition.
    fn assert_unrelated_add_preserves(doc: &mut Doc) {
        let before = snapshot(doc);
        let wires = live_wires(doc);
        let memberships: Vec<_> = wires.iter().map(|&(id, _)| net_members(doc, id)).collect();
        let change = doc
            .apply(Command::AddWires(vec![w(1000, 1000, 1010, 1000)]))
            .unwrap();
        assert!(change.removed.is_empty());
        assert_eq!(change.added.len(), 1);
        for (&(id, wire), members) in wires.iter().zip(memberships) {
            assert_eq!(doc.wire(id), Some(&wire));
            assert_eq!(net_members(doc, id), members);
        }
        assert_inv10(doc);
        let after = snapshot(doc);
        doc.undo().unwrap();
        assert!(doc.content_eq(&before), "INV4: exact undo including ids");
        doc.redo().unwrap();
        assert!(doc.content_eq(&after), "INV4: exact redo including ids");
        doc.undo().unwrap();
    }

    #[test]
    fn unrelated_add_preserves_move_delete_and_duplicate_non_normal_states() {
        let mut moved = Doc::new();
        moved
            .apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 5, 5, 10)]))
            .unwrap();
        let id = moved.wires().find(|(_, wire)| wire.a.x == 5).unwrap().0;
        moved
            .apply(Command::Move {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![ItemId::Wire(id)],
                delta: Pt { x: 0, y: -5 },
            })
            .unwrap();
        assert_unrelated_add_preserves(&mut moved);

        let mut deleted = Doc::new();
        deleted
            .apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 0, 5, 10)]))
            .unwrap();
        let branch = deleted
            .wires()
            .find(|(_, wire)| wire.a.x == wire.b.x)
            .unwrap()
            .0;
        deleted
            .apply(Command::Delete {
                items: vec![ItemId::Wire(branch)],
                with_wires: false,
            })
            .unwrap();
        assert_eq!(deleted.wires().count(), 1);
        assert_unrelated_add_preserves(&mut deleted);

        let mut duplicate = Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 0, 0)], []).unwrap();
        assert_unrelated_add_preserves(&mut duplicate);
    }

    #[test]
    fn merge_expansion_stops_at_fixed_touched_points_and_keeps_external_junctions() {
        // Only the first wire directly touches the new segment. Its far end is
        // a touched point, so the second can merge; the third's far boundary
        // was never touched and the pre-existing extra chain is left alone.
        let mut doc =
            Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 20, 0), w(20, 0, 30, 0)], []).unwrap();
        doc.apply(Command::AddWires(vec![w(-10, 0, 0, 0)])).unwrap();
        assert_eq!(
            live_wires(&doc),
            vec![(WireId(0), w(-10, 0, 20, 0)), (WireId(2), w(20, 0, 30, 0))]
        );
        assert_inv10(&doc);
        let mut doc =
            Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 20, 0), w(10, 0, 10, 10)], []).unwrap();
        doc.apply(Command::AddWires(vec![w(-10, 0, 0, 0)])).unwrap();
        assert_eq!(doc.wire(WireId(0)), Some(&w(-10, 0, 10, 0)));
        assert_eq!(doc.wire(WireId(1)), Some(&w(10, 0, 20, 0)));
        assert_eq!(doc.wire(WireId(2)), Some(&w(10, 0, 10, 10)));
        assert_all(&doc);
    }

    fn with_features(pins: Vec<Pt>, labels: Vec<Pt>) -> Doc {
        Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "1k".into(),
                },
                pins,
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [],
            labels.into_iter().map(|at| Label {
                at,
                text: "SIG".into(),
            }),
        )
        .unwrap()
    }

    fn point_strategy() -> impl Strategy<Value = Pt> {
        (-4i32..5, -4i32..5).prop_map(|(x, y)| Pt { x, y })
    }

    fn wire_strategy() -> impl Strategy<Value = Wire> {
        (-4i32..5, -4i32..5, any::<bool>(), 1i32..5).prop_map(|(x, y, horiz, len)| {
            if horiz {
                w(x, y, x + len, y)
            } else {
                w(x, y, x, y + len)
            }
        })
    }

    fn lattice(p: Pt) -> Pt {
        Pt {
            x: p.x * crate::PIN_LATTICE_NM,
            y: p.y * crate::PIN_LATTICE_NM,
        }
    }

    fn lw(x1: i32, y1: i32, x2: i32, y2: i32) -> Wire {
        let w = w(x1, y1, x2, y2);
        Wire {
            a: lattice(w.a),
            b: lattice(w.b),
        }
    }

    #[test]
    fn wd4_blockers_rule_priority_direction_and_index_order() {
        let segment = lw(0, 0, 10, 0);
        let check = |doc: &Doc| check_new_wires(doc, &[segment], [segment.a, segment.b]);
        let pins = with_features(vec![lattice(Pt { x: 3, y: 0 })], vec![]);
        assert_eq!(
            check(&pins),
            Err(Blocker::Pin {
                part: crate::PartId(0),
                pin: 0
            })
        );
        let ends = Doc::from_items([], [lw(4, 0, 4, 2)], []).unwrap();
        assert_eq!(check(&ends), Err(Blocker::WireEnd(WireId(0))));
        let overlap = Doc::from_items([], [lw(-1, 0, 11, 0)], []).unwrap();
        assert_eq!(check(&overlap), Err(Blocker::Overlap(WireId(0))));
        let outside = Wire {
            a: Pt::ZERO,
            b: Pt {
                x: crate::SHEET_LIMIT_NM + 1,
                y: 0,
            },
        };
        assert_eq!(
            check_new_wires(&pins, &[segment, outside], [segment.a, outside.b]),
            Err(Blocker::Sheet)
        );
        assert_eq!(
            check_new_wires(&Doc::new(), &[w(0, 0, 1, 0)], [Pt::ZERO; 2]),
            Err(Blocker::Sheet)
        );
        // Second rule outranks third even on a later segment. The two wires
        // are not chained, so the first segment's own ends exempt every point
        // on it and only its overlap fails there.
        let doc = Doc::from_items([], [lw(-1, 0, 11, 0), lw(20, 4, 22, 4)], []).unwrap();
        assert_eq!(
            check_new_wires(&doc, &[segment, lw(20, 0, 20, 8)], [segment.a, segment.b]),
            Err(Blocker::WireEnd(WireId(1)))
        );
        // W1.5: a corner (a vertex that is not an own end) is in the run's
        // interior, so a foreign wire's interior there blocks under rule 2,
        // before the later segment's end and the first segment's overlap.
        let doc = Doc::from_items([], [lw(-1, 0, 11, 0), lw(10, 4, 12, 4)], []).unwrap();
        assert_eq!(
            check_new_wires(
                &doc,
                &[segment, lw(10, 0, 10, 8)],
                [segment.a, lattice(Pt { x: 10, y: 8 })]
            ),
            Err(Blocker::WireBody(WireId(0)))
        );
        // A corner on a foreign pin or a foreign wire end blocks too (TC-move-08).
        let corner = [segment, lw(10, 0, 10, 8)];
        let ends = [segment.a, lattice(Pt { x: 10, y: 8 })];
        let pin_at_corner = with_features(vec![lattice(Pt { x: 10, y: 0 })], vec![]);
        assert_eq!(
            check_new_wires(&pin_at_corner, &corner, ends),
            Err(Blocker::Pin {
                part: crate::PartId(0),
                pin: 0
            })
        );
        let end_at_corner = Doc::from_items([], [lw(10, 0, 14, 0)], []).unwrap();
        assert_eq!(
            check_new_wires(&end_at_corner, &corner, ends),
            Err(Blocker::WireEnd(WireId(0)))
        );
        let mut doc = Doc::from_items([], [lw(8, 0, 8, 2), lw(2, 0, 2, 2)], []).unwrap();
        assert_eq!(check(&doc), Err(Blocker::WireEnd(WireId(1))));
        let reversed = Wire {
            a: segment.b,
            b: segment.a,
        };
        assert_eq!(
            check_new_wires(&doc, &[reversed], [segment.a, segment.b]),
            Err(Blocker::WireEnd(WireId(0)))
        );
        // Undo restore changes index insertion order but not the selected blocker.
        doc.apply(Command::Delete {
            items: vec![ItemId::Wire(WireId(0))],
            with_wires: false,
        })
        .unwrap();
        doc.undo().unwrap();
        assert_eq!(check(&doc), Err(Blocker::WireEnd(WireId(1))));
        let own = lattice(Pt { x: 2, y: 0 });
        assert_eq!(
            check_new_wires(&doc, &[segment], [own, lattice(Pt { x: 8, y: 0 })]),
            Ok(())
        );
        let own_pin = lattice(Pt { x: 3, y: 0 });
        assert_eq!(
            check_new_wires(&pins, &[segment], [own_pin, segment.b]),
            Ok(())
        );
    }

    #[test]
    fn inv2_validated_crossing_stays_in_different_nets() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![lw(0, 0, 10, 0)])).unwrap();
        let candidate = lw(5, -5, 5, 5);
        assert_eq!(
            check_new_wires(&doc, &[candidate], [candidate.a, candidate.b]),
            Ok(())
        );
        doc.apply(Command::AddWires(vec![candidate])).unwrap();
        doc.assert_normal_form();
        assert_eq!(doc.wires().count(), 2);
        assert_ne!(doc.net_of(WireId(0)), doc.net_of(WireId(1)));
    }

    #[test]
    fn winv3_delete_merges_only_vacated_points_and_restores_exact_ids() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![
            lw(0, 0, 10, 0),
            lw(3, 0, 3, 3),
            lw(7, 0, 7, 3),
        ]))
        .unwrap();
        let before = snapshot(&doc);
        let branches: Vec<_> = doc
            .wires()
            .filter(|(_, w)| w.a.x == w.b.x)
            .map(|(id, _)| ItemId::Wire(id))
            .collect();
        let mut reversed = snapshot(&doc);
        reversed
            .apply(Command::Delete {
                items: branches.iter().copied().rev().collect(),
                with_wires: false,
            })
            .unwrap();
        doc.apply(Command::Delete {
            items: branches,
            with_wires: false,
        })
        .unwrap();
        doc.assert_normal_form();
        assert_inv10(&doc);
        assert_eq!(live_wires(&doc), vec![(WireId(0), lw(0, 0, 10, 0))]);
        assert!(doc.content_eq(&reversed));
        let after = snapshot(&doc);
        doc.undo().unwrap();
        assert!(doc.content_eq(&before));
        assert_inv10(&doc);
        doc.redo().unwrap();
        assert!(doc.content_eq(&after));
        assert_inv10(&doc);
        // WD9 was already implemented by T6.1: exercise a touched label,
        // not an unrelated AddWires which never visits its point.
        let mut labelled = with_features(vec![], vec![lattice(Pt { x: 5, y: 0 })]);
        labelled
            .apply(Command::AddWires(vec![lw(0, 0, 10, 0), lw(5, 0, 5, 5)]))
            .unwrap();
        let branch = labelled.wires().find(|(_, w)| w.a.x == w.b.x).unwrap().0;
        labelled
            .apply(Command::Delete {
                items: vec![ItemId::Wire(branch)],
                with_wires: false,
            })
            .unwrap();
        labelled.assert_normal_form();
        assert_eq!(labelled.wires().count(), 2);
        assert!(labelled.net_at(lattice(Pt { x: 5, y: 0 })).is_some());
        // A distant legacy pair is outside the deletion neighbourhood.
        let mut local =
            Doc::from_items([], [lw(0, 0, 5, 0), lw(5, 0, 10, 0), lw(20, 0, 21, 0)], []).unwrap();
        local
            .apply(Command::Delete {
                items: vec![ItemId::Wire(WireId(2))],
                with_wires: false,
            })
            .unwrap();
        assert_eq!(local.wire(WireId(0)), Some(&lw(0, 0, 5, 0)));
        assert_eq!(local.wire(WireId(1)), Some(&lw(5, 0, 10, 0)));
    }

    proptest! {
        #![proptest_config(crate::proptest_config(128))]
        #[test]
        fn winv3_inv4_inv10_add_delete_sequences(
            steps in prop::collection::vec((any::<bool>(), prop::collection::vec(wire_strategy(), 1..4), any::<usize>()), 1..40),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins.into_iter().map(lattice).collect(), labels.into_iter().map(lattice).collect());
            for (delete, wires, pick) in steps {
                let before = snapshot(&doc);
                let live = live_wires(&doc);
                let cmd = if delete && !live.is_empty() {
                    Command::Delete { items: vec![ItemId::Wire(live[pick % live.len()].0)], with_wires: false }
                } else {
                    Command::AddWires(wires.into_iter().map(|w| Wire { a: lattice(w.a), b: lattice(w.b) }).collect())
                };
                doc.apply(cmd).unwrap();
                doc.assert_normal_form();
                assert_inv10(&doc);
                let after = snapshot(&doc);
                if !before.content_eq(&after) {
                    doc.undo().unwrap();
                    prop_assert!(doc.content_eq(&before));
                    doc.assert_normal_form();
                    assert_inv10(&doc);
                    doc.redo().unwrap();
                    prop_assert!(doc.content_eq(&after));
                    doc.assert_normal_form();
                    assert_inv10(&doc);
                }
            }
        }
    }

    proptest! {
        #![proptest_config(crate::proptest_config(128))]
        /// INV2 + INV10 hold after every AddWires step, and undoing every step
        /// restores the empty document (INV4, ids included via content_eq).
        #[test]
        fn inv2_inv10_and_undo_over_random_add_wires(
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..4), 1..24),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins, labels);
            let empty = snapshot(&doc);
            let mut applied = 0usize;
            for wires in &batches {
                let before_bytes = {
                    let mut bytes = Vec::new();
                    doc.save(&mut bytes).unwrap();
                    bytes
                };
                let before = Doc::load(before_bytes.as_slice()).unwrap();
                let change = doc.apply(Command::AddWires(wires.clone())).unwrap();
                assert_all(&doc);
                let after = snapshot(&doc);
                if change.dirty.is_some() {
                    prop_assert!(doc.undo().is_some());
                    prop_assert!(doc.content_eq(&before), "INV4: undo restores pre-AddWires ids");
                    prop_assert!(doc.redo().is_some());
                    prop_assert!(doc.content_eq(&after), "INV4: redo equals apply, ids included");
                    assert_all(&doc);
                    applied += 1;
                } else {
                    prop_assert!(doc.content_eq(&before), "no-op AddWires changes nothing");
                }
            }
            for _ in 0..applied {
                if doc.undo().is_none() {
                    break;
                }
            }
            // Undo steps that were no-ops pushed nothing; drain any remainder.
            while doc.undo().is_some() {}
            prop_assert!(doc.content_eq(&empty), "undoing every AddWires restores the empty doc");
        }

        /// INV4 / INV10 through Move/Delete states; local AddWires must not
        /// silently repair any of those states on the rest of the sheet.
        #[test]
        fn local_add_with_features_and_mixed_edits(
            steps in prop::collection::vec((0u8..3, prop::collection::vec(wire_strategy(), 1..4), any::<usize>(), point_strategy()), 1..24),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins, labels);
            let initial = snapshot(&doc);
            for (kind, wires, pick, delta) in steps {
                let before = snapshot(&doc);
                let live = live_wires(&doc);
                let cmd = match (kind, live.is_empty()) {
                    (1, false) => Command::Move { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(), items: vec![ItemId::Wire(live[pick % live.len()].0)], delta },
                    (2, false) => Command::Delete { items: vec![ItemId::Wire(live[pick % live.len()].0)], with_wires: false },
                    _ => Command::AddWires(wires.clone()),
                };
                // Reversed input order must produce the same ids and geometry,
                // even from a non-normal starting state (D2).
                if matches!(&cmd, Command::AddWires(_)) {
                    let mut reversed = snapshot(&doc);
                    reversed.apply(Command::AddWires(wires.into_iter().rev().collect())).unwrap();
                    doc.apply(cmd).unwrap();
                    prop_assert!(doc.content_eq(&reversed));
                } else {
                    // W2.2 (WD3 capture rule): a moved wire is refused exactly
                    // when an independent brute-force oracle finds a foreign pin
                    // or end in its interior or a shared stretch; every other
                    // command must succeed, as before. Refusals change nothing.
                    // WK1/WINV3: an on-lattice end may not land off the lattice
                    // (checked after the capture rule). These sheets use raw nm.
                    let (expect_capture, expect_off_lattice) = match &cmd {
                        // A zero delta is an inert command (no validation, no edit).
                        Command::Move { items, delta, .. } if *delta != Pt::ZERO => {
                            let ItemId::Wire(id) = items[0] else { unreachable!() };
                            let w = *doc.wire(id).unwrap();
                            let lat = |p: Pt| p.x % crate::PIN_LATTICE_NM == 0 && p.y % crate::PIN_LATTICE_NM == 0;
                            let (a, b) = (Pt { x: w.a.x + delta.x, y: w.a.y + delta.y }, Pt { x: w.b.x + delta.x, y: w.b.y + delta.y });
                            let mut off = (lat(w.a) && !lat(a)) || (lat(w.b) && !lat(b));
                            // A lattice wire may not cover an off-lattice label (it would split there).
                            off |= lat(a) && lat(b) && doc.labels().any(|(_, l)| {
                                let p = l.at;
                                !lat(p) && p != a && p != b
                                    && (a.x.min(b.x)..=a.x.max(b.x)).contains(&p.x)
                                    && (a.y.min(b.y)..=a.y.max(b.y)).contains(&p.y)
                            });
                            (moved_wire_captures(&doc, id, *delta), off)
                        }
                        _ => (false, false),
                    };
                    let mut bytes = Vec::new();
                    doc.save(&mut bytes).unwrap();
                    match doc.apply(cmd) {
                        Err(crate::CommandError::Capture { .. }) => {
                            prop_assert!(expect_capture, "refused without a capture");
                            let mut refused = Vec::new();
                            doc.save(&mut refused).unwrap();
                            prop_assert_eq!(refused, bytes);
                        }
                        Err(crate::CommandError::OffLattice(_)) => {
                            prop_assert!(!expect_capture && expect_off_lattice, "refused off the lattice wrongly");
                            let mut refused = Vec::new();
                            doc.save(&mut refused).unwrap();
                            prop_assert_eq!(refused, bytes);
                        }
                        result => {
                            prop_assert!(!expect_capture, "capture accepted: {:?}", result);
                            prop_assert!(!expect_off_lattice, "off-lattice end accepted: {:?}", result);
                            result.unwrap();
                        }
                    }
                }
                assert_inv10(&doc);
                let after = snapshot(&doc);
                if !doc.content_eq(&before) {
                    doc.undo().unwrap();
                    prop_assert!(doc.content_eq(&before));
                    doc.redo().unwrap();
                    prop_assert!(doc.content_eq(&after));
                }
                assert_unrelated_add_preserves(&mut doc);
            }
            while doc.undo().is_some() {}
            prop_assert!(doc.content_eq(&initial));
        }
    }

    /// Brute-force WD3 capture oracle for a single selected wire translated by
    /// `delta`, independent of the command's indexed implementation.
    fn moved_wire_captures(doc: &Doc, id: WireId, delta: Pt) -> bool {
        let w = *doc.wire(id).unwrap();
        let a = Pt {
            x: w.a.x + delta.x,
            y: w.a.y + delta.y,
        };
        let b = Pt {
            x: w.b.x + delta.x,
            y: w.b.y + delta.y,
        };
        let (lo, hi) = (
            Pt {
                x: a.x.min(b.x),
                y: a.y.min(b.y),
            },
            Pt {
                x: a.x.max(b.x),
                y: a.y.max(b.y),
            },
        );
        let interior = |p: Pt| {
            p != a && p != b && (lo.x..=hi.x).contains(&p.x) && (lo.y..=hi.y).contains(&p.y)
        };
        let pins = doc
            .parts()
            .any(|(part, _)| doc.pins(part).unwrap().iter().any(|&p| interior(p)));
        let wires = doc.wires().filter(|&(other, _)| other != id).any(|(_, o)| {
            let (olo, ohi) = (
                Pt {
                    x: o.a.x.min(o.b.x),
                    y: o.a.y.min(o.b.y),
                },
                Pt {
                    x: o.a.x.max(o.b.x),
                    y: o.a.y.max(o.b.y),
                },
            );
            let stretch = if a.y == b.y && o.a.y == o.b.y && a.y == o.a.y {
                lo.x.max(olo.x) < hi.x.min(ohi.x)
            } else if a.x == b.x && o.a.x == o.b.x && a.x == o.a.x {
                lo.y.max(olo.y) < hi.y.min(ohi.y)
            } else {
                false
            };
            interior(o.a) || interior(o.b) || stretch
        });
        pins || wires
    }

    // ---- W1.5: route candidates (WD4, WK5, WK6, WINV10) ----

    fn lp(x: i32, y: i32) -> Pt {
        lattice(Pt { x, y })
    }

    fn chain(points: &[(i32, i32)]) -> Vec<Wire> {
        points
            .windows(2)
            .map(|p| lw(p[0].0, p[0].1, p[1].0, p[1].1))
            .collect()
    }

    /// One part per entry: world = local (at the origin, identity), body in
    /// lattice units `(x0, y0, x1, y1)`, pins in lattice units.
    fn sheet(parts: &[PartSpec], wires: &[Wire]) -> Doc {
        Doc::from_items(
            parts
                .iter()
                .enumerate()
                .map(|(i, &((x0, y0, x1, y1), ref pins))| {
                    (
                        Part {
                            symbol: SymbolId(0),
                            at: Pt::ZERO,
                            orient: Orient::IDENTITY,
                            designator: format!("U{i}").into(),
                            value: "1k".into(),
                        },
                        pins.iter().map(|&(x, y)| lp(x, y)).collect(),
                        Rect::from_points(lp(x0, y0), lp(x1, y1)).unwrap(),
                    )
                }),
            wires.iter().copied(),
            [],
        )
        .unwrap()
    }

    fn route_of(doc: &Doc, a: Pt, b: Pt, first_leg: Axis) -> Result<Vec<Wire>, Blocker> {
        let mut out = Vec::new();
        route(doc, a, b, first_leg, &mut RouteScratch::new(), &mut out).map(|()| out)
    }

    #[test]
    fn w15_wd4_off_sheet_candidates_keep_body_and_crossing_scores() {
        // Reflect the review reproduction across both axes, covering all K1 edges.
        for swap in [false, true] {
            for sign in [-1, 1] {
                let transform = |(x, y): (i32, i32)| {
                    if swap { (sign * y, x) } else { (x, sign * y) }
                };
                let lo = transform((-2, 784));
                let hi = transform((2, 787));
                let pins = [(1, 786), (-1, 786), (0, 785), (0, 787)]
                    .into_iter()
                    .map(transform)
                    .collect();
                let crossing = [transform((-3, 787)), transform((3, 787))];
                let doc = sheet(&[((lo.0, lo.1, hi.0, hi.1), pins)], &chain(&crossing));
                let a = transform((0, 786));
                let b = transform((6, 782));
                let axis = if swap {
                    Axis::Vertical
                } else {
                    Axis::Horizontal
                };
                for (number, wires) in oracle_candidates(lp(a.0, a.1), lp(b.0, b.1), axis) {
                    let ends = [lp(a.0, a.1), lp(b.0, b.1)];
                    let judged = wires.iter().all(|&w| shape_valid(w));
                    let (actual, _, _) = assess(&doc, &wires, ends, number, judged);
                    assert_eq!(
                        (
                            actual.bodies,
                            actual.bends,
                            actual.crossings,
                            actual.length,
                            actual.number
                        ),
                        oracle_score(&doc, &wires, number)
                    );
                }
                assert_eq!(
                    route_of(&doc, lp(a.0, a.1), lp(b.0, b.1), axis),
                    Err(Blocker::Pin {
                        part: crate::PartId(0),
                        pin: 0
                    })
                );
            }
        }
    }

    #[test]
    fn w15_straight_and_each_l() {
        let doc = Doc::new();
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 0), Axis::Vertical),
            Ok(chain(&[(0, 0), (10, 0)]))
        );
        assert_eq!(
            route_of(&doc, lp(0, 6), lp(0, -4), Axis::Horizontal),
            Ok(chain(&[(0, 6), (0, -4)]))
        );
        // The two Ls tie on everything but the candidate number: first_leg decides.
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Horizontal),
            Ok(chain(&[(0, 0), (10, 0), (10, 6)]))
        );
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Vertical),
            Ok(chain(&[(0, 0), (0, 6), (10, 6)]))
        );
        assert_eq!(
            route_of(&doc, lp(3, 3), lp(3, 3), Axis::Vertical),
            Ok(vec![])
        );
    }

    #[test]
    fn w15_l_blocked_by_pin_takes_the_other_l() {
        let doc = sheet(&[((20, 20, 22, 22), vec![(5, 0)])], &[]);
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Horizontal),
            Ok(chain(&[(0, 0), (0, 6), (10, 6)]))
        );
        // A pin on the L's corner blocks it too.
        let doc = sheet(&[((20, 20, 22, 22), vec![(10, 0)])], &[]);
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Horizontal),
            Ok(chain(&[(0, 0), (0, 6), (10, 6)]))
        );
    }

    #[test]
    fn w15_both_ls_blocked_dogleg_wins() {
        let doc = sheet(&[((20, 20, 22, 22), vec![(5, 0), (0, 3)])], &[]);
        // Nearest mid-line one grid step (two lattice units) from a towards b:
        // H-V-H at x = 2 ties V-H-V at y = 2 on length 16; H-V-H is numbered first.
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Horizontal),
            Ok(chain(&[(0, 0), (2, 0), (2, 6), (10, 6)]))
        );
        // Blocking the straight sends a level pair round a U-shaped dogleg; level
        // ends try the lesser coordinate first.
        let doc = sheet(&[((20, 20, 22, 22), vec![(4, 0)])], &[]);
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 0), Axis::Horizontal),
            Ok(chain(&[(0, 0), (0, -2), (10, -2), (10, 0)]))
        );
    }

    #[test]
    fn w15_tc_move_12_part_body_in_the_way() {
        // The moved part's body spans (-4, -2)..(0, 2); its pin a sits on the right
        // edge. A foreign pin at (-4, 4) blocks the vertical-first L, so the only L
        // left runs through the body. The V-H-V dogleg along the body's top edge
        // passes through no body interior and wins despite its extra bend.
        let doc = sheet(
            &[
                ((-4, -2, 0, 2), vec![(0, 0)]),
                ((20, 20, 22, 22), vec![(-4, 4)]),
            ],
            &[],
        );
        let routed = route_of(&doc, lp(0, 0), lp(-8, 4), Axis::Horizontal).unwrap();
        assert_eq!(routed, chain(&[(0, 0), (0, 2), (-8, 2), (-8, 4)]));
        // Without the dogleg rule the body L would be the only L.
        assert_eq!(
            check_new_wires(
                &doc,
                &chain(&[(0, 0), (-8, 0), (-8, 4)]),
                [lp(0, 0), lp(-8, 4)]
            ),
            Ok(())
        );
    }

    #[test]
    fn w15_crossings_rank_after_bends_and_corner_on_a_body_is_invalid() {
        // A foreign vertical wire crosses the horizontal-first L: the other L wins.
        let doc = sheet(&[], &[lw(5, -3, 5, 3)]);
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Horizontal),
            Ok(chain(&[(0, 0), (0, 6), (10, 6)]))
        );
        // The vertical-first L's corner (0, 6) lies on a foreign wire's interior: it
        // would connect that wire, so it is invalid and the crossing L wins.
        let doc = sheet(&[], &[lw(5, -3, 5, 3), lw(-3, 6, 3, 6)]);
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(10, 6), Axis::Vertical),
            Ok(chain(&[(0, 0), (10, 0), (10, 6)]))
        );
    }

    #[test]
    fn w15_refusal_reports_lowest_scoring_invalid_candidate() {
        // Pins one lattice unit from a on all four sides: every candidate's first
        // segment holds one. The best invalid candidate is the first L.
        let doc = sheet(
            &[((20, 20, 22, 22), vec![(1, 0), (-1, 0), (0, 1), (0, -1)])],
            &[],
        );
        let pin = |pin| Blocker::Pin {
            part: crate::PartId(0),
            pin,
        };
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(6, 4), Axis::Horizontal),
            Err(pin(0))
        );
        assert_eq!(
            route_of(&doc, lp(0, 0), lp(6, 4), Axis::Vertical),
            Err(pin(2))
        );
        // A body band crossed by both Ls ranks them below the V-H-V dogleg at
        // y = -2 (running along the band's bottom edge), so the refusal names that
        // dogleg's blocker (the pin below a), not the first L's.
        let doc = sheet(
            &[
                ((20, 20, 22, 22), vec![(1, 0), (-1, 0), (0, 1), (0, -1)]),
                ((2, -2, 4, 6), vec![]),
            ],
            &[],
        );
        let mut out = vec![lw(0, 0, 1, 0)];
        let got = route(
            &doc,
            lp(0, 0),
            lp(6, 4),
            Axis::Horizontal,
            &mut RouteScratch::new(),
            &mut out,
        );
        assert!(out.is_empty());
        assert_eq!(got, Err(pin(3)));
        let expected = oracle(&doc, lp(0, 0), lp(6, 4), Axis::Horizontal);
        assert!(expected.0.is_err() && expected.1 >= 3, "{expected:?}");
        assert_eq!(got, expected.0.map(|_| ()));
        // Off-lattice ends: every candidate fails the first rule.
        assert_eq!(
            route_of(&Doc::new(), Pt { x: 1, y: 0 }, lp(6, 4), Axis::Vertical),
            Err(Blocker::Sheet)
        );
    }

    #[test]
    fn w15_dogleg_order_reach_and_cap() {
        let mut candidates = Vec::new();
        enumerate_candidates(lp(0, 0), lp(10, 6), Axis::Horizontal, &mut candidates);
        let numbers: Vec<u16> = candidates.iter().map(|c| c.score.number).collect();
        assert_eq!(numbers, (1..=numbers.len() as u16).collect::<Vec<_>>());
        // The independent oracle lists the same candidates in the same order.
        let expected = oracle_candidates(lp(0, 0), lp(10, 6), Axis::Horizontal);
        assert_eq!(candidates.len(), expected.len());
        for (c, (number, wires)) in candidates.iter().zip(&expected) {
            assert_eq!((c.score.number, c.wires()), (*number, wires.as_slice()));
        }
        // A long span hits the WK6 cap.
        enumerate_candidates(lp(0, 0), lp(400, 300), Axis::Vertical, &mut candidates);
        assert_eq!(candidates.len(), 2 + DOGLEG_CAP);
        assert_eq!(
            candidates[2].wires(),
            chain(&[(0, 0), (2, 0), (2, 300), (400, 300)])
        );
        assert_eq!(
            candidates[3].wires(),
            chain(&[(0, 0), (-2, 0), (-2, 300), (400, 300)])
        );
        assert_eq!(
            candidates[4].wires(),
            chain(&[(0, 0), (0, 2), (400, 2), (400, 300)])
        );
    }

    /// Independent oracle for WD4: brute-force over every item (no index), the
    /// candidate list built by sorting every mid-line by (steps, family, side).
    fn oracle_candidates(a: Pt, b: Pt, first_leg: Axis) -> Vec<(u16, Vec<Wire>)> {
        let seg = |pts: &[Pt]| {
            pts.windows(2)
                .map(|p| Wire { a: p[0], b: p[1] })
                .collect::<Vec<_>>()
        };
        let mut out = Vec::new();
        if a.x == b.x || a.y == b.y {
            out.push((0, seg(&[a, b])));
        } else {
            let h = seg(&[a, Pt { x: b.x, y: a.y }, b]);
            let v = seg(&[a, Pt { x: a.x, y: b.y }, b]);
            let (first, second) = if first_leg == Axis::Horizontal {
                (h, v)
            } else {
                (v, h)
            };
            out.push((1, first));
            out.push((2, second));
        }
        let g = i64::from(crate::GRID_NM);
        let reach = 8 * g;
        let mut mids = Vec::new();
        for family in 0..2 {
            let (from, to) = if family == 0 {
                (i64::from(a.x), i64::from(b.x))
            } else {
                (i64::from(a.y), i64::from(b.y))
            };
            let degenerate = if family == 0 { a.y == b.y } else { a.x == b.x };
            let (lo, hi) = (from.min(to) - reach, from.max(to) + reach);
            for k in -1000i64..=1000 {
                let m = from + k * g;
                if k == 0 || m < lo || m > hi || m == to || degenerate {
                    continue;
                }
                let towards =
                    (m - from).signum() == (to - from).signum() || (to == from && m < from);
                mids.push((k.abs(), family, if towards { 0 } else { 1 }, m as i32));
            }
        }
        mids.sort();
        for (i, &(_, family, _, m)) in mids.iter().take(DOGLEG_CAP).enumerate() {
            let pts = if family == 0 {
                [a, Pt { x: m, y: a.y }, Pt { x: m, y: b.y }, b]
            } else {
                [a, Pt { x: a.x, y: m }, Pt { x: b.x, y: m }, b]
            };
            out.push((3 + i as u16, seg(&pts)));
        }
        out
    }

    fn oracle_score(doc: &Doc, wires: &[Wire], number: u16) -> (u8, u8, u32, i64, u16) {
        let open = |a: i32, b: i32| (a.min(b), a.max(b));
        let mut bodies = 0;
        let mut crossings = 0;
        let mut length = 0;
        for w in wires {
            length += i64::from((w.a.x - w.b.x).abs()) + i64::from((w.a.y - w.b.y).abs());
            let horizontal = w.a.y == w.b.y;
            let (s0, s1) = if horizontal {
                open(w.a.x, w.b.x)
            } else {
                open(w.a.y, w.b.y)
            };
            let fixed = if horizontal { w.a.y } else { w.a.x };
            if doc.parts().any(|(id, _)| {
                let r = doc.part_body(id).unwrap();
                let (along, across) = if horizontal {
                    ((r.min.x, r.max.x), (r.min.y, r.max.y))
                } else {
                    ((r.min.y, r.max.y), (r.min.x, r.max.x))
                };
                across.0 < fixed
                    && fixed < across.1
                    && s0 < along.1
                    && along.0 < s1
                    && along.0 < along.1
            }) {
                bodies += 1;
            }
            for (_, o) in doc.wires() {
                if (o.a.y == o.b.y) == horizontal {
                    continue;
                }
                let (o0, o1) = if horizontal {
                    open(o.a.y, o.b.y)
                } else {
                    open(o.a.x, o.b.x)
                };
                let ofixed = if horizontal { o.a.x } else { o.a.y };
                if s0 < ofixed && ofixed < s1 && o0 < fixed && fixed < o1 {
                    crossings += 1;
                }
            }
        }
        (bodies, (wires.len() - 1) as u8, crossings, length, number)
    }

    /// Independent enumeration/scoring oracle; validity and blocker selection reuse
    /// check_new_wires, so this does not independently prove the validator.
    /// Returns the answer and its candidate number.
    fn oracle(doc: &Doc, a: Pt, b: Pt, first_leg: Axis) -> (Result<Vec<Wire>, Blocker>, u16) {
        if a == b {
            return (Ok(vec![]), 0);
        }
        let mut ranked: Vec<_> = oracle_candidates(a, b, first_leg)
            .into_iter()
            .map(|(n, wires)| (oracle_score(doc, &wires, n), wires))
            .collect();
        ranked.sort_by_key(|(score, _)| *score);
        for (score, wires) in &ranked {
            if check_new_wires(doc, wires, [a, b]).is_ok() {
                return (Ok(wires.clone()), score.4);
            }
        }
        let (score, wires) = &ranked[0];
        (
            Err(check_new_wires(doc, wires, [a, b]).unwrap_err()),
            score.4,
        )
    }

    type PartSpec = ((i32, i32, i32, i32), Vec<(i32, i32)>);

    fn part_strategy() -> impl Strategy<Value = PartSpec> {
        (
            -8i32..8,
            -8i32..8,
            1i32..5,
            1i32..5,
            prop::collection::vec((0i32..5, 0i32..5, 0u8..4), 0..3),
        )
            .prop_map(|(x, y, w, h, pins)| {
                // Pins on the body's edges, as symbols place them.
                let pins = pins
                    .into_iter()
                    .map(|(u, v, side)| match side {
                        0 => (x + u.min(w), y),
                        1 => (x + u.min(w), y + h),
                        2 => (x, y + v.min(h)),
                        _ => (x + w, y + v.min(h)),
                    })
                    .collect();
                ((x, y, x + w, y + h), pins)
            })
    }

    fn random_sheet(parts: &[PartSpec], batches: Vec<Vec<Wire>>) -> Doc {
        let mut doc = sheet(parts, &[]);
        for batch in batches {
            let wires = batch
                .into_iter()
                .map(|w| Wire {
                    a: lattice(Pt {
                        x: w.a.x * 2,
                        y: w.a.y * 2,
                    }),
                    b: lattice(Pt {
                        x: w.b.x * 2,
                        y: w.b.y * 2,
                    }),
                })
                .collect();
            doc.apply(Command::AddWires(wires)).unwrap();
        }
        doc.assert_normal_form();
        doc
    }

    /// Union-find classes over probe points from `net_at`, optionally joining two.
    /// The net at `p`: its point's, or that of a wire whose body holds it (a
    /// merge removes a point but not its place on the net).
    fn net_here(doc: &Doc, p: Pt) -> Option<crate::NetId> {
        doc.net_at(p).or_else(|| {
            doc.wires()
                .find(|(_, w)| contains(**w, p))
                .and_then(|(id, _)| doc.net_of(id))
        })
    }

    /// Probes at the same point are one place, whatever `net_at` says.
    fn same_net_matrix(doc: &Doc, probes: &[Pt], join: &[(usize, usize)]) -> Vec<Vec<bool>> {
        let n = probes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], i: usize) -> usize {
            if p[i] == i {
                i
            } else {
                let r = find(p, p[i]);
                p[i] = r;
                r
            }
        }
        let union = |p: &mut Vec<usize>, i: usize, j: usize| {
            let (ri, rj) = (find(p, i), find(p, j));
            p[ri] = rj;
        };
        for i in 0..n {
            for j in 0..n {
                let (ni, nj) = (net_here(doc, probes[i]), net_here(doc, probes[j]));
                if probes[i] == probes[j] || (ni.is_some() && ni == nj) {
                    union(&mut parent, i, j);
                }
            }
        }
        for &(i, j) in join {
            union(&mut parent, i, j);
        }
        (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| find(&mut parent, i) == find(&mut parent, j))
                    .collect()
            })
            .collect()
    }

    /// Geometry of a blocker, for comparing across documents with different ids.
    fn blocker_shape(doc: &Doc, blocker: Blocker) -> (u8, Option<Pt>) {
        match blocker {
            Blocker::Pin { part, pin } => (0, Some(doc.pins(part).unwrap()[pin])),
            Blocker::WireEnd(_) => (1, None),
            Blocker::WireBody(_) => (2, None),
            Blocker::Overlap(_) => (3, None),
            Blocker::Sheet => (4, None),
        }
    }

    fn axis(vertical: bool) -> Axis {
        if vertical {
            Axis::Vertical
        } else {
            Axis::Horizontal
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        /// WINV10 and WD4: every returned route is orthogonal, on the lattice, a
        /// chain from a to b, passes check_new_wires, keeps normal form when added,
        /// and joins exactly the nets of a and b. Ok or Err, the answer is the
        /// enumeration/scoring oracle's (using the production validator): the lowest-scoring valid candidate, else the
        /// blocker of the lowest-scoring invalid one.
        #[test]
        fn w15_winv10_route_properties_and_oracle(
            parts in prop::collection::vec(part_strategy(), 0..5),
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..3), 0..5),
            a in (-10i32..11, -10i32..11),
            b in (-10i32..11, -10i32..11),
            vertical in any::<bool>(),
        ) {
            let mut doc = random_sheet(&parts, batches);
            let (a, b) = (lp(a.0, a.1), lp(b.0, b.1));
            let first_leg = axis(vertical);
            let mut scratch = RouteScratch::new();
            let mut out = vec![lw(0, 0, 1, 0)];
            let got = route(&doc, a, b, first_leg, &mut scratch, &mut out);
            let (expected, _) = oracle(&doc, a, b, first_leg);
            match (&got, &expected) {
                (Ok(()), Ok(wires)) => prop_assert_eq!(&out, wires),
                (Err(g), Err(e)) => {
                    prop_assert_eq!(g, e);
                    prop_assert!(out.is_empty());
                }
                _ => prop_assert!(false, "route {got:?} {out:?}, oracle {expected:?}"),
            }
            if got.is_err() || a == b {
                return Ok(());
            }
            prop_assert_eq!(out.first().unwrap().a, a);
            prop_assert_eq!(out.last().unwrap().b, b);
            for pair in out.windows(2) {
                prop_assert_eq!(pair[0].b, pair[1].a);
            }
            for w in &out {
                prop_assert!(w.a != w.b && (w.a.x == w.b.x || w.a.y == w.b.y));
                prop_assert!(on_lattice(w.a) && on_lattice(w.b));
            }
            prop_assert_eq!(check_new_wires(&doc, &out, [a, b]), Ok(()));
            // Probe every pin and wire end, then a and b.
            let mut probes: Vec<Pt> = doc
                .parts()
                .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec())
                .chain(doc.wires().flat_map(|(_, w)| [w.a, w.b]))
                .collect();
            probes.extend([a, b]);
            let n = probes.len();
            // a and b join. An end on a foreign wire's body is on that wire's net
            // (net_here), as D2's split makes it (TC-move-14).
            let expected_nets = same_net_matrix(&doc, &probes, &[(n - 2, n - 1)]);
            doc.apply(Command::AddWires(out.clone())).unwrap();
            doc.assert_normal_form();
            assert_inv10(&doc);
            prop_assert_eq!(same_net_matrix(&doc, &probes, &[]), expected_nets);
        }

        /// Determinism: the same inputs give the same route with a fresh or a warm
        /// scratch, and on the same sheet rebuilt with items in reverse id order.
        #[test]
        fn w15_route_is_deterministic(
            parts in prop::collection::vec(part_strategy(), 0..5),
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..3), 0..5),
            ends in prop::collection::vec(((-10i32..11, -10i32..11), (-10i32..11, -10i32..11), any::<bool>()), 1..6),
        ) {
            let doc = random_sheet(&parts, batches);
            let mut rev_parts = parts.clone();
            rev_parts.reverse();
            let rev_wires: Vec<Wire> = doc.wires().map(|(_, &w)| w).collect::<Vec<_>>().into_iter().rev().collect();
            let rebuilt = sheet(&rev_parts, &rev_wires);
            let mut warm = RouteScratch::new();
            let mut warm_out = Vec::new();
            for (a, b, vertical) in ends {
                let (a, b, leg) = (lp(a.0, a.1), lp(b.0, b.1), axis(vertical));
                let fresh = route_of(&doc, a, b, leg);
                let again = route(&doc, a, b, leg, &mut warm, &mut warm_out).map(|()| warm_out.clone());
                prop_assert_eq!(&fresh, &again);
                let other = route_of(&rebuilt, a, b, leg);
                match (&fresh, &other) {
                    (Ok(x), Ok(y)) => prop_assert_eq!(x, y),
                    (Err(x), Err(y)) => prop_assert_eq!(
                        blocker_shape(&doc, *x),
                        blocker_shape(&rebuilt, *y)
                    ),
                    _ => prop_assert!(false, "{fresh:?} vs rebuilt {other:?}"),
                }
            }
        }
    }

    // ---- W3.2: segment drag planner and command (WD7, WINV3, WINV4) ----

    fn pin_partition(doc: &Doc) -> Vec<Vec<bool>> {
        let pins: Vec<Pt> = doc
            .parts()
            .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec())
            .collect();
        same_net_matrix(doc, &pins, &[])
    }

    #[test]
    fn tc_seg_01_drag_middle_of_z_run_stretches_neighbours() {
        let mut doc = sheet(
            &[
                ((0, 0, 2, 2), vec![(2, 0)]),
                ((10, 6, 12, 8), vec![(10, 6)]),
            ],
            &[],
        );
        doc.apply(Command::AddWires(chain(&[(2, 0), (6, 0), (6, 6), (10, 6)])))
            .unwrap();
        let middle = doc
            .wires()
            .find(|(_, w)| w.a.x == w.b.x && w.a.x == lp(6, 0).x)
            .unwrap()
            .0;
        let before = snapshot(&doc);
        let pins_before = pin_partition(&doc);

        let mut plan = SegmentDragPlan::default();
        plan_segment_drag(&doc, middle, 2 * crate::PIN_LATTICE_NM, &mut plan).unwrap();
        assert_eq!(plan.remove.len(), 3);
        assert_eq!(plan.add.len(), 3);

        doc.apply(Command::DragSegment {
            wire: middle,
            offset: 2 * crate::PIN_LATTICE_NM,
        })
        .unwrap();
        doc.assert_normal_form();
        assert_inv10(&doc);
        assert_eq!(pin_partition(&doc), pins_before);

        let mut run = crate::Run::default();
        doc.run_through(middle, &mut run);
        assert_eq!(run.wires.len(), 3, "still one run");
        assert_eq!(
            doc.wire(middle),
            Some(&lw(8, 0, 8, 6)),
            "dragged segment moved perpendicular and kept its id"
        );

        let after = snapshot(&doc);
        doc.undo().unwrap();
        assert!(doc.content_eq(&before));
        doc.assert_normal_form();
        assert_inv10(&doc);
        doc.redo().unwrap();
        assert!(doc.content_eq(&after));
    }

    #[test]
    fn tc_seg_02_drag_segment_with_pin_end_inserts_jog() {
        let mut doc = sheet(&[((0, 0, 2, 2), vec![(2, 0)])], &[]);
        doc.apply(Command::AddWires(vec![lw(2, 0, 8, 0)])).unwrap();
        let wire = WireId(0);
        let before = snapshot(&doc);
        let pins_before = pin_partition(&doc);

        let mut plan = SegmentDragPlan::default();
        plan_segment_drag(&doc, wire, 2 * crate::PIN_LATTICE_NM, &mut plan).unwrap();
        assert_eq!(plan.remove, vec![wire]);
        assert_eq!(plan.add, vec![lw(2, 0, 2, 2), lw(2, 2, 8, 2)]);

        doc.apply(Command::DragSegment {
            wire,
            offset: 2 * crate::PIN_LATTICE_NM,
        })
        .unwrap();
        doc.assert_normal_form();
        assert_inv10(&doc);
        assert_eq!(pin_partition(&doc), pins_before);
        assert_eq!(doc.wires().count(), 2);
        assert_eq!(doc.wire(wire), Some(&lw(2, 2, 8, 2)));

        doc.undo().unwrap();
        assert!(doc.content_eq(&before));
    }

    #[test]
    fn tc_seg_03_drag_segment_across_foreign_pin_is_refused() {
        let mut doc = sheet(
            &[((0, 0, 2, 2), vec![(2, 0)]), ((4, 2, 6, 4), vec![(4, 2)])],
            &[],
        );
        doc.apply(Command::AddWires(vec![lw(2, 0, 8, 0)])).unwrap();
        let wire = WireId(0);
        let before = snapshot(&doc);
        let rev = doc.revision();

        let expected = Blocker::Pin {
            part: crate::PartId(1),
            pin: 0,
        };
        let mut plan = SegmentDragPlan::default();
        assert_eq!(
            plan_segment_drag(&doc, wire, 2 * crate::PIN_LATTICE_NM, &mut plan),
            Err(expected)
        );
        assert!(plan.remove.is_empty() && plan.add.is_empty());

        let err = doc.apply(Command::DragSegment {
            wire,
            offset: 2 * crate::PIN_LATTICE_NM,
        });
        assert_eq!(err, Err(crate::CommandError::Segment(expected)));
        assert_eq!(doc.revision(), rev);
        assert!(doc.content_eq(&before));
        doc.assert_normal_form();
        assert_inv10(&doc);
    }

    #[test]
    fn tc_seg_04_drag_segment_shrinks_neighbour_to_zero_and_normalises() {
        for steps in [4, 8] {
            let mut doc = sheet(
                &[
                    ((0, 0, 2, 2), vec![(2, 0)]),
                    ((12, 4, 14, 6), vec![(12, 4)]),
                ],
                &[],
            );
            doc.apply(Command::AddWires(chain(&[(2, 0), (6, 0), (6, 4), (12, 4)])))
                .unwrap();
            let top = doc
                .wires()
                .find(|(_, w)| w.a == lp(2, 0) || w.b == lp(2, 0))
                .unwrap()
                .0;
            let before = snapshot(&doc);
            let pins_before = pin_partition(&doc);

            doc.apply(Command::DragSegment {
                wire: top,
                offset: steps * crate::PIN_LATTICE_NM,
            })
            .unwrap();
            doc.assert_normal_form();
            assert_inv10(&doc);
            assert_eq!(pin_partition(&doc), pins_before);
            // Vertical neighbour shrank to zero (stopped at y = 4), horizontal legs merged,
            // and a jog connects the pin at (2, 0) to (2, 4).
            let mut wires: Vec<Wire> = doc.wires().map(|(_, &w)| w).collect();
            wires.sort_by_key(|w| (pt_key(w.a), pt_key(w.b)));
            assert_eq!(wires, vec![lw(2, 0, 2, 4), lw(2, 4, 12, 4)]);

            let after = snapshot(&doc);
            doc.undo().unwrap();
            assert!(doc.content_eq(&before));
            doc.redo().unwrap();
            assert!(doc.content_eq(&after));
        }
    }

    #[test]
    fn w32_junction_and_labelled_corner_insert_jogs_and_free_ends_move() {
        // Junction end at (0, 0) (three wire ends meeting) stays put and gets a jog;
        // free end at (6, 0) moves with the horizontal segment.
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![lw(-4, 0, 6, 0), lw(0, 0, 0, 4)]))
            .unwrap();
        let branch = doc
            .wires()
            .find(|(_, w)| w.a == lp(0, 0) && w.b == lp(6, 0))
            .unwrap()
            .0;
        let before = snapshot(&doc);
        // Dragging into the vertical stem (0, 0)-(0, 4) is refused by WD4.
        assert!(
            doc.apply(Command::DragSegment {
                wire: branch,
                offset: 2 * crate::PIN_LATTICE_NM,
            })
            .is_err()
        );
        assert!(doc.content_eq(&before));

        // Dragging away from the vertical stem (-2) inserts a jog (0, -2)-(0, 0) and moves free end (6, 0) to (6, -2).
        doc.apply(Command::DragSegment {
            wire: branch,
            offset: -2 * crate::PIN_LATTICE_NM,
        })
        .unwrap();
        doc.assert_normal_form();
        assert_inv10(&doc);
        assert_eq!(doc.wire(branch), Some(&lw(0, -2, 6, -2)));
        assert!(doc.wires().any(|(_, &w)| w == lw(0, 0, 0, -2)));
        doc.undo().unwrap();
        assert!(doc.content_eq(&before));

        // Labelled corner keeps the label at (4, 0) connected on both stretch (-2) and shrink (+2).
        for step in [-2, 2] {
            let mut labelled = with_features(vec![lp(4, 6)], vec![lp(4, 0)]);
            labelled
                .apply(Command::AddWires(vec![lw(0, 0, 4, 0), lw(4, 0, 4, 6)]))
                .unwrap();
            let horiz = labelled.wires().find(|(_, w)| w.a.y == w.b.y).unwrap().0;
            let before = snapshot(&labelled);
            labelled
                .apply(Command::DragSegment {
                    wire: horiz,
                    offset: step * crate::PIN_LATTICE_NM,
                })
                .unwrap();
            labelled.assert_normal_form();
            assert_inv10(&labelled);
            assert_eq!(labelled.wire(horiz), Some(&lw(0, step, 4, step)));
            assert_eq!(
                labelled.net_at(lp(4, 0)),
                labelled.net_of(horiz),
                "label at corner stays connected"
            );
            assert_eq!(
                labelled.net_at(lp(4, 6)),
                labelled.net_of(horiz),
                "WD7: far pin stays connected through the neighbour"
            );
            assert!(
                labelled
                    .wires()
                    .any(|(_, w)| w.a == lp(4, 6) || w.b == lp(4, 6))
            );
            let after = snapshot(&labelled);
            labelled.undo().unwrap();
            assert!(labelled.content_eq(&before));
            labelled.redo().unwrap();
            assert!(labelled.content_eq(&after));
        }
    }

    #[test]
    fn w32_labelled_shrink_both_endpoint_orders_and_axes() {
        for vertical in [false, true] {
            for reversed in [false, true] {
                let transform = |p: Pt| if vertical { Pt { x: p.y, y: p.x } } else { p };
                let old = transform(lp(4, 0));
                let far = transform(lp(4, 6));
                let mut doc = with_features(vec![far], vec![old]);
                let start = transform(lp(0, 0));
                doc.apply(Command::AddWires(vec![
                    Wire { a: start, b: old },
                    Wire { a: old, b: far },
                ]))
                .unwrap();
                let id = doc
                    .wires()
                    .find(|(_, w)| w.a == start && w.b == old)
                    .unwrap()
                    .0;
                if reversed {
                    // Same geometry, opposite stored endpoint orientation.
                    doc.remove_wire_raw(id).unwrap();
                    doc.restore_wire_raw(id, Wire { a: old, b: start }).unwrap();
                }
                let before = snapshot(&doc);
                doc.apply(Command::DragSegment {
                    wire: id,
                    offset: 2 * crate::PIN_LATTICE_NM,
                })
                .unwrap();
                doc.assert_normal_form();
                assert_inv10(&doc);
                assert_eq!(
                    doc.net_at(far),
                    doc.net_of(id),
                    "WD7: far anchor remains attached"
                );
                assert_eq!(
                    doc.net_at(old),
                    doc.net_of(id),
                    "WD9: label remains attached"
                );
                let after = snapshot(&doc);
                doc.undo().unwrap();
                assert!(doc.content_eq(&before));
                doc.redo().unwrap();
                assert!(doc.content_eq(&after));
            }
        }
    }

    #[test]
    fn w32_off_lattice_offset_is_refused_before_zero_clamp() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![lw(0, 0, 4, 0), lw(4, 0, 4, 2)]))
            .unwrap();
        let wire = doc.wires().find(|(_, w)| w.a.y == w.b.y).unwrap().0;
        let before = snapshot(&doc);
        let offset = 3 * crate::PIN_LATTICE_NM + 1;
        let mut plan = SegmentDragPlan::default();
        assert_eq!(
            plan_segment_drag(&doc, wire, offset, &mut plan),
            Err(Blocker::Sheet)
        );
        assert!(plan.remove.is_empty() && plan.add.is_empty());
        assert_eq!(
            doc.apply(Command::DragSegment { wire, offset }),
            Err(crate::CommandError::Segment(Blocker::Sheet))
        );
        assert!(doc.content_eq(&before));
        doc.assert_normal_form();
        assert_inv10(&doc);
    }

    proptest! {
        #![proptest_config(crate::proptest_config(128))]
        /// W3.2 (WD7, WINV3, WINV4, INV10): for random sheets and segment drags,
        /// plan_segment_drag matches Command::DragSegment; refused drags return
        /// Blocker and leave the document unchanged; accepted drags preserve the
        /// partition of pins, leave the document in normal form (WINV3) and INV10,
        /// and undo/redo restores exact ids and state (WINV4).
        #[test]
        fn w32_winv3_winv4_segment_drag_properties(
            parts in prop::collection::vec(part_strategy(), 0..5),
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..4), 1..6),
            drags in prop::collection::vec((any::<usize>(), -6i32..=6, any::<bool>()), 1..12),
        ) {
            let mut doc = random_sheet(&parts, batches);
            for (pick, steps, off_lattice_fuzz) in drags {
                let live: Vec<WireId> = doc.wires().map(|(id, _)| id).collect();
                if live.is_empty() {
                    continue;
                }
                let wire = live[pick % live.len()];
                let offset = if off_lattice_fuzz {
                    steps * crate::PIN_LATTICE_NM + 1
                } else {
                    steps * crate::PIN_LATTICE_NM
                };
                let before = snapshot(&doc);
                let rev_before = doc.revision();
                let pins_before = pin_partition(&doc);

                let mut preview = SegmentDragPlan::default();
                let planned = plan_segment_drag(&doc, wire, offset, &mut preview);
                let applied = doc.apply(Command::DragSegment { wire, offset });

                match (planned, applied) {
                    (Err(b1), Err(crate::CommandError::Segment(b2))) => {
                        prop_assert_eq!(b1, b2);
                        prop_assert!(preview.remove.is_empty() && preview.add.is_empty());
                        prop_assert_eq!(doc.revision(), rev_before);
                        prop_assert!(doc.content_eq(&before));
                        doc.assert_normal_form();
                        assert_inv10(&doc);
                    }
                    (Ok(()), Ok(change)) => {
                        doc.assert_normal_form();
                        assert_inv10(&doc);
                        prop_assert_eq!(pin_partition(&doc), pins_before);
                        let after = snapshot(&doc);
                        if change.dirty.is_none() {
                            prop_assert!(preview.remove.is_empty() && preview.add.is_empty());
                            prop_assert_eq!(doc.revision(), rev_before);
                            prop_assert!(doc.content_eq(&before));
                        } else {
                            // Preview matches the committed wire set.
                            let mut expected_wires: Vec<Wire> = before
                                .wires()
                                .filter(|(id, _)| !preview.remove.contains(id))
                                .map(|(_, &w)| w)
                                .chain(preview.add.iter().copied())
                                .collect();
                            let mut actual_wires: Vec<Wire> =
                                doc.wires().map(|(_, &w)| w).collect();
                            let norm_key = |w: &Wire| {
                                let (lo, hi, h) = normalised(*w);
                                (pt_key(lo), pt_key(hi), h)
                            };
                            expected_wires.sort_by_key(norm_key);
                            actual_wires.sort_by_key(norm_key);
                            prop_assert_eq!(
                                actual_wires.into_iter().map(|w| norm_key(&w)).collect::<Vec<_>>(),
                                expected_wires.into_iter().map(|w| norm_key(&w)).collect::<Vec<_>>(),
                            );

                            doc.undo().unwrap();
                            prop_assert!(doc.content_eq(&before));
                            doc.assert_normal_form();
                            assert_inv10(&doc);

                            doc.redo().unwrap();
                            prop_assert!(doc.content_eq(&after));
                            doc.assert_normal_form();
                            assert_inv10(&doc);
                        }
                    }
                    other => prop_assert!(false, "mismatch: {other:?}"),
                }
            }
        }

        /// W2.2: `plan_follow_add_wires` equals `plan_add_wires` on every label-free sheet
        /// for any batch of runs sequentially validated by `route`.
        #[test]
        fn w22_plan_follow_add_wires_matches_plan_add_wires(
            parts in prop::collection::vec(part_strategy(), 0..5),
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..4), 0..5),
            pairs in prop::collection::vec(
                (
                    (-6i32..=6, -6i32..=6),
                    (-6i32..=6, -6i32..=6),
                    any::<bool>(),
                ),
                1..6,
            ),
        ) {
            let doc = random_sheet(&parts, batches);
            let mut working = snapshot(&doc);
            let mut scratch = RouteScratch::new();
            let mut out = Vec::new();
            let mut added = Vec::new();
            for ((ax, ay), (bx, by), horiz) in pairs {
                let a = lp(ax, ay);
                let b = lp(bx, by);
                let axis = if horiz { Axis::Horizontal } else { Axis::Vertical };
                if route(&working, a, b, axis, &mut scratch, &mut out).is_ok() {
                    for &w in &out {
                        working.insert_wire(w).unwrap();
                    }
                    added.extend_from_slice(&out);
                }
            }
            prop_assert_eq!(
                plan_follow_add_wires(&doc, &added),
                plan_add_wires(&doc, &added),
            );
        }
    }
}
