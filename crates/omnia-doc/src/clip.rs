//! Schematic clipboard snapshot and `Command::Paste` execution (T7.1, D2, INV2, INV4, INV10).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    Doc, EmbeddedPartDef, GRID_NM, ItemId, Label, LabelId, Orient, Params, Part, PartId, Pt, Rect,
    SHEET_LIMIT_NM, Wire, WireId,
    command::{ChangeAcc, CommandError, apply_wire_plan, insert_part_with_def_raw},
    part_def::{validate, validate_instance},
    snap_to,
    undo::UndoOp,
};

/// Largest multiple of [`GRID_NM`] that lies within K1 (`±SHEET_LIMIT_NM`).
const MAX_GRID_NM: i32 = (SHEET_LIMIT_NM / GRID_NM) * GRID_NM;

/// One copied part inside a [`Clip`], with `part.at` relative to the clip's reference origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipPart {
    pub part: Part,
    pub local_pins: Vec<Pt>,
    pub local_bounds: Rect,
    /// Self-contained definition snapshot from the source document (`P2.2`, `PD3`, `PD8`).
    pub def: Arc<EmbeddedPartDef>,
    /// Copied parameter overrides (`PD6`).
    pub params: Params,
}

/// Self-contained snapshot of copied schematic items (T7.1, D2).
///
/// Coordinates are displacements from a deterministic, [`GRID_NM`]-aligned reference origin
/// computed from the copied items' bounds at [`Doc::copy`] time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clip {
    parts: Vec<ClipPart>,
    wires: Vec<Wire>,
    labels: Vec<Label>,
}

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

#[inline]
fn apply_orient_pt(orient: Orient, p: Pt) -> Pt {
    let (rx, ry) = apply_orient_i64(orient, i64::from(p.x), i64::from(p.y));
    Pt {
        x: i32::try_from(rx).expect("clip displacement fits in i32"),
        y: i32::try_from(ry).expect("clip displacement fits in i32"),
    }
}

impl Clip {
    /// Returns `true` when the clip holds no parts, wires, or labels.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.wires.is_empty() && self.labels.is_empty()
    }

    /// Total number of copied parts, wires, and labels.
    pub fn item_count(&self) -> usize {
        self.parts.len() + self.wires.len() + self.labels.len()
    }

    /// Copied parts in ascending source-ID order, with `part.at` relative to the clip origin.
    pub fn parts(&self) -> &[ClipPart] {
        &self.parts
    }

    /// Copied wire segments in ascending source-ID order, relative to the clip origin.
    pub fn wires(&self) -> &[Wire] {
        &self.wires
    }

    /// Copied net labels in ascending source-ID order, relative to the clip origin.
    pub fn labels(&self) -> &[Label] {
        &self.labels
    }

    /// Clip-local position of the anchor pin (pin 0 of the first copied part that has pins),
    /// used for [`GRID_NM`] snapping during ghost placement (K2).
    pub fn anchor_pin(&self) -> Option<Pt> {
        self.parts.iter().find_map(|cp| {
            let local = *cp.local_pins.first()?;
            let off = cp.part.orient.apply(local);
            Some(Pt {
                x: i32::try_from(i64::from(cp.part.at.x) + i64::from(off.x))
                    .expect("clip anchor pin x fits in i32"),
                y: i32::try_from(i64::from(cp.part.at.y) + i64::from(off.y))
                    .expect("clip anchor pin y fits in i32"),
            })
        })
    }

    /// Returns a copy of this clip reoriented around its local origin `(0, 0)` by `by`.
    pub fn reoriented(&self, by: Orient) -> Self {
        if by == Orient::IDENTITY {
            return self.clone();
        }
        let parts = self
            .parts
            .iter()
            .map(|cp| {
                let mut part = cp.part.clone();
                part.at = apply_orient_pt(by, part.at);
                part.orient = part.orient.then(by);
                ClipPart {
                    part,
                    local_pins: cp.local_pins.clone(),
                    local_bounds: cp.local_bounds,
                    def: Arc::clone(&cp.def),
                    params: cp.params.clone(),
                }
            })
            .collect();
        let wires = self
            .wires
            .iter()
            .map(|w| Wire {
                a: apply_orient_pt(by, w.a),
                b: apply_orient_pt(by, w.b),
            })
            .collect();
        let labels = self
            .labels
            .iter()
            .map(|l| Label {
                at: apply_orient_pt(by, l.at),
                text: Arc::clone(&l.text),
            })
            .collect();
        Self {
            parts,
            wires,
            labels,
        }
    }
}

fn union_pt(bounds: &mut Option<Rect>, p: Pt) {
    let r = Rect { min: p, max: p };
    *bounds = Some(match *bounds {
        Some(existing) => existing.union(r),
        None => r,
    });
}

fn union_rect(bounds: &mut Option<Rect>, r: Rect) {
    *bounds = Some(match *bounds {
        Some(existing) => existing.union(r),
        None => r,
    });
}

impl Doc {
    /// Captures the live items named in `items` into a self-contained [`Clip`] (T7.1, D2).
    ///
    /// Missing or duplicate IDs are ignored/deduplicated, and item order never changes the clip:
    /// items are stored in ascending ID order per kind with coordinates relative to a
    /// [`GRID_NM`]-aligned reference origin so half-grid parts preserve pin-lattice parity on
    /// paste.
    pub fn copy(&self, items: &[ItemId]) -> Clip {
        let mut part_ids: Vec<PartId> = Vec::new();
        let mut wire_ids: Vec<WireId> = Vec::new();
        let mut label_ids: Vec<LabelId> = Vec::new();
        for &item in items {
            match item {
                ItemId::Part(id) if self.part(id).is_some() => part_ids.push(id),
                ItemId::Wire(id) if self.wire(id).is_some() => wire_ids.push(id),
                ItemId::Label(id) if self.label(id).is_some() => label_ids.push(id),
                _ => {}
            }
        }
        part_ids.sort_unstable_by_key(|id| id.0);
        part_ids.dedup();
        wire_ids.sort_unstable_by_key(|id| id.0);
        wire_ids.dedup();
        label_ids.sort_unstable_by_key(|id| id.0);
        label_ids.dedup();

        if part_ids.is_empty() && wire_ids.is_empty() && label_ids.is_empty() {
            return Clip::default();
        }

        let mut bounds: Option<Rect> = None;
        for &id in &part_ids {
            let stored = self.parts.get(id.0).expect("live copied part");
            union_pt(&mut bounds, stored.part.at);
            if let Some(body) = self.part_body(id) {
                union_rect(&mut bounds, body);
            }
            for &pin in &stored.pins {
                union_pt(&mut bounds, pin);
            }
        }
        for &id in &wire_ids {
            let wire = self.wire(id).expect("live copied wire");
            union_pt(&mut bounds, wire.a);
            union_pt(&mut bounds, wire.b);
        }
        for &id in &label_ids {
            let label = self.label(id).expect("live copied label");
            union_pt(&mut bounds, label.at);
        }

        let snapped = snap_to(bounds.expect("non-empty clip has bounds").center(), GRID_NM);
        let origin = Pt {
            x: snapped.x.clamp(-MAX_GRID_NM, MAX_GRID_NM),
            y: snapped.y.clamp(-MAX_GRID_NM, MAX_GRID_NM),
        };

        let parts = part_ids
            .into_iter()
            .map(|id| {
                let stored = self.parts.get(id.0).expect("live copied part");
                let def = self
                    .part_defs
                    .get_arc(stored.def)
                    .expect("live copied part must reference a definition");
                let local_pins = def.local_pins.clone();
                let mut part = stored.part.clone();
                part.at = part.at - origin;
                ClipPart {
                    part,
                    local_pins,
                    local_bounds: stored.local_bounds,
                    def,
                    params: stored.params.clone(),
                }
            })
            .collect();

        let wires = wire_ids
            .into_iter()
            .map(|id| {
                let wire = self.wire(id).expect("live copied wire");
                Wire {
                    a: wire.a - origin,
                    b: wire.b - origin,
                }
            })
            .collect();

        let labels = label_ids
            .into_iter()
            .map(|id| {
                let label = self.label(id).expect("live copied label");
                Label {
                    at: label.at - origin,
                    text: Arc::clone(&label.text),
                }
            })
            .collect();

        Clip {
            parts,
            wires,
            labels,
        }
    }
}

fn offset_pt(base: Pt, rel: Pt) -> Result<Pt, CommandError> {
    let x = i32::try_from(i64::from(base.x) + i64::from(rel.x))
        .map_err(|_| CommandError::OutOfBounds)?;
    let y = i32::try_from(i64::from(base.y) + i64::from(rel.y))
        .map_err(|_| CommandError::OutOfBounds)?;
    Pt::new(x, y).map_err(|_| CommandError::OutOfBounds)
}

/// Returns the non-numeric designator stem used when allocating sequential collision names.
///
/// Strips trailing ASCII digits deterministically: `"U1A"` yields prefix `"U1A"` (`"U1A1"`),
/// pure digits like `"10"` yield `""` (`"1"`), and `""` falls back to `"X"` (`"X1"`).
fn designator_prefix(designator: &str) -> &str {
    let trimmed = designator.trim_end_matches(|c: char| c.is_ascii_digit());
    if trimmed.is_empty() {
        if designator.is_empty() { "X" } else { "" }
    } else {
        trimmed
    }
}

fn next_unique_designator(
    raw: &Arc<str>,
    used: &mut FxHashSet<Arc<str>>,
    next_by_prefix: &mut FxHashMap<String, u32>,
) -> Arc<str> {
    let prefix = designator_prefix(raw);
    let next = next_by_prefix.entry(prefix.to_owned()).or_insert(1);
    loop {
        let candidate: Arc<str> = Arc::from(format!("{prefix}{next}"));
        *next = next.checked_add(1).expect("designator number space");
        if !used.contains(&candidate) {
            used.insert(Arc::clone(&candidate));
            return candidate;
        }
    }
}

pub(crate) fn point_in_wire_strict_interior(p: Pt, w: Wire) -> bool {
    if w.a.y == w.b.y {
        p.y == w.a.y && w.a.x.min(w.b.x) < p.x && p.x < w.a.x.max(w.b.x)
    } else if w.a.x == w.b.x {
        p.x == w.a.x && w.a.y.min(w.b.y) < p.y && p.y < w.a.y.max(w.b.y)
    } else {
        false
    }
}

/// Executes [`crate::Command::Paste`] transactionally against `doc` (T7.1, D2, INV2, INV4, INV10).
///
/// Pre-validates every part origin, pin, local bounds, wire, and label before mutating anything;
/// assigns unique designators deterministically; inserts parts and labels; and normalises pasted
/// wires plus any existing wires touched by landed pins, wire ends, or labels through
/// [`crate::route::plan_add_wires`].
pub(crate) fn apply_paste(
    doc: &mut Doc,
    clip: Clip,
    at: Pt,
    acc: &mut ChangeAcc,
) -> Result<UndoOp, CommandError> {
    if clip.is_empty() {
        return Ok(UndoOp::Batch(Vec::new()));
    }

    // Phase 1: full transactional validation before any document or arena mutation.
    let mut resolved_parts = Vec::with_capacity(clip.parts.len());
    for cp in clip.parts {
        validate(&cp.def).map_err(CommandError::InvalidDef)?;
        validate_instance(
            &cp.def,
            cp.part.symbol,
            &cp.local_pins,
            cp.local_bounds,
            &cp.part.value,
            &cp.params,
        )
        .map_err(CommandError::InvalidInstance)?;
        Rect::new(cp.local_bounds.min, cp.local_bounds.max)
            .map_err(|_| CommandError::OutOfBounds)?;
        let new_at = offset_pt(at, cp.part.at)?;
        let mut world_pins = Vec::with_capacity(cp.local_pins.len());
        for &pin in &cp.local_pins {
            if !pin.is_in_bounds() {
                return Err(CommandError::OutOfBounds);
            }
            world_pins.push(new_at.checked_add(cp.part.orient.apply(pin))?);
        }
        let area = Doc::part_envelope_raw(new_at, cp.part.orient, cp.local_bounds, &world_pins)?;
        let mut part = cp.part;
        part.at = new_at;
        resolved_parts.push((part, world_pins, cp.local_bounds, area, cp.def, cp.params));
    }

    let mut resolved_wires = Vec::with_capacity(clip.wires.len());
    for w in clip.wires {
        let wire = Wire {
            a: offset_pt(at, w.a)?,
            b: offset_pt(at, w.b)?,
        };
        Doc::validate_wire(wire)?;
        resolved_wires.push(wire);
    }

    let mut resolved_labels = Vec::with_capacity(clip.labels.len());
    for l in clip.labels {
        let new_at = offset_pt(at, l.at)?;
        resolved_labels.push(Label {
            at: new_at,
            text: l.text,
        });
    }

    // Phase 2: ensure pasted part designators do not collide with existing parts or each other.
    // Pass 1 reserves every non-colliding original designator first so later parts keep free names;
    // Pass 2 assigns deterministic sequential fallback names only to colliding parts.
    if !resolved_parts.is_empty() {
        let mut used: FxHashSet<Arc<str>> = doc
            .parts()
            .map(|(_, part)| Arc::clone(&part.designator))
            .collect();
        let mut needs_rename = vec![false; resolved_parts.len()];
        for (i, (part, ..)) in resolved_parts.iter().enumerate() {
            if used.contains(&part.designator) {
                needs_rename[i] = true;
            } else {
                used.insert(Arc::clone(&part.designator));
            }
        }
        let mut next_by_prefix: FxHashMap<String, u32> = FxHashMap::default();
        for (i, (part, ..)) in resolved_parts.iter_mut().enumerate() {
            if needs_rename[i] {
                part.designator =
                    next_unique_designator(&part.designator, &mut used, &mut next_by_prefix);
            }
        }
    }

    // Phase 3: apply validated inserts and wire normalisation.
    let mut added_non_wires = Vec::with_capacity(resolved_parts.len() + resolved_labels.len());
    let mut landed = Vec::new();

    for (part, world_pins, local_bounds, bounds, def, params) in resolved_parts {
        let has_pins = !world_pins.is_empty();
        let def_id = doc.part_defs.intern(def);
        let id =
            insert_part_with_def_raw(doc, part, world_pins, local_bounds, bounds, def_id, params);
        acc.record_dirty(bounds);
        let item = ItemId::Part(id);
        acc.record_added(item);
        if has_pins {
            acc.nets_changed = true;
        }
        added_non_wires.push(item);
        landed.extend_from_slice(doc.pins(id).expect("inserted part exists"));
    }

    for label in resolved_labels {
        let label_at = label.at;
        let (id, bounds) = doc
            .insert_label_raw(label)
            .expect("pre-validated paste label must insert");
        acc.record_dirty(bounds);
        let item = ItemId::Label(id);
        acc.record_added(item);
        acc.nets_changed = true;
        added_non_wires.push(item);
        landed.push(label_at);
    }

    // Existing wires whose strict interior receives a pasted pin or label must split into normal
    // form (INV2, WD9); endpoint-only touches leave existing wires out so normalisation stays local
    // (D2), while pasted wires themselves still pass through `plan_add_wires`.
    let wire_undo = if !landed.is_empty() || !resolved_wires.is_empty() {
        if !landed.is_empty() {
            landed.sort_unstable_by_key(|p| (p.x, p.y));
            landed.dedup();
            let mut touched = Vec::new();
            for p in landed {
                doc.index
                    .visit_wires_unstamped(Rect { min: p, max: p }, |id, _| {
                        if doc
                            .wire(id)
                            .is_some_and(|w| point_in_wire_strict_interior(p, *w))
                        {
                            touched.push(id);
                        }
                    });
            }
            touched.sort_unstable_by_key(|id| id.0);
            touched.dedup();
            for id in touched {
                resolved_wires.push(*doc.wire(id).expect("touched wire exists"));
            }
        }
        if resolved_wires.is_empty() {
            UndoOp::Batch(Vec::new())
        } else {
            let plan = crate::route::plan_add_wires(doc, &resolved_wires);
            apply_wire_plan(doc, plan, acc)
        }
    } else {
        UndoOp::Batch(Vec::new())
    };

    // Undo batches execute in reverse order: undo wire additions/splits before removing pasted
    // parts and labels; redo restores parts and labels first so pins/labels are present when
    // wires restore under their exact IDs (INV4, INV10).
    let mut ops = Vec::with_capacity(2);
    if !added_non_wires.is_empty() {
        ops.push(UndoOp::DeleteRemove(added_non_wires));
    }
    if !matches!(&wire_undo, UndoOp::Batch(v) if v.is_empty()) {
        ops.push(wire_undo);
    }
    Ok(UndoOp::Batch(ops))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Connectivity, PIN_LATTICE_NM, SymbolId};
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect::from_points(pt(x0, y0), pt(x1, y1)).unwrap()
    }

    fn part(symbol: u16, at: Pt, orient: Orient, designator: &str, value: &str) -> Part {
        Part {
            symbol: SymbolId(symbol),
            at,
            orient,
            designator: designator.into(),
            value: value.into(),
        }
    }

    fn two_pin_pins() -> Vec<Pt> {
        vec![Pt { x: -2 * G, y: 0 }, Pt { x: 2 * G, y: 0 }]
    }

    fn two_pin_bounds() -> Rect {
        rect(-2, -1, 2, 1)
    }

    fn assert_inv10_and_normal_form(doc: &Doc) {
        doc.assert_normal_form();
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &w)| (id, w)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        crate::connectivity::tests::assert_partitions(&doc.connectivity, &oracle);
    }

    #[test]
    fn t7_1_copy_is_canonical_ignores_missing_and_empty_paste_is_noop() {
        let mut doc = Doc::new();
        let p0 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(-4, 0), Orient::IDENTITY, "R1", "10k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let l0 = doc
            .apply(Command::AddLabel(Label {
                at: pt(0, 0),
                text: "MID".into(),
            }))
            .unwrap()
            .added[0];
        let wires = doc
            .apply(Command::AddWires(vec![Wire {
                a: pt(-2, 0),
                b: pt(2, 0),
            }]))
            .unwrap()
            .added;
        assert_eq!(wires.len(), 2);

        let a = doc.copy(&[l0, p0, wires[0], wires[1], p0, ItemId::Part(PartId(999))]);
        let b = doc.copy(&[wires[1], l0, p0, wires[0]]);
        assert_eq!(a, b);
        assert_eq!(a.item_count(), 4); // 1 part, 2 split wires, 1 label
        assert_eq!(a.parts().len(), 1);
        assert_eq!(a.wires().len(), 2);
        assert_eq!(a.labels().len(), 1);

        let empty = doc.copy(&[ItemId::Part(PartId(999))]);
        assert!(empty.is_empty());
        let rev = doc.revision();
        let cursor = doc.history_cursor();
        let snap = doc.snapshot();
        let change = doc
            .apply(Command::Paste {
                clip: empty,
                at: Pt::ZERO,
            })
            .unwrap();
        assert_eq!(change.dirty, None);
        assert!(change.added.is_empty());
        assert_eq!(doc.revision(), rev);
        assert_eq!(doc.history_cursor(), cursor);
        assert_eq!(doc.snapshot(), snap);
    }

    #[test]
    fn t7_1_half_grid_part_preserves_pin_lattice_parity_across_orientations() {
        // An op-amp-like part at half-grid origin with half-grid local pins (all world pins on GRID_NM).
        let half = PIN_LATTICE_NM;
        let op_pins = vec![
            Pt {
                x: -3 * half,
                y: -half,
            },
            Pt {
                x: -3 * half,
                y: half,
            },
            Pt { x: 3 * half, y: 0 },
        ];
        let op_bounds = Rect::from_points(
            Pt {
                x: -3 * half,
                y: -3 * half,
            },
            Pt {
                x: 3 * half,
                y: 3 * half,
            },
        )
        .unwrap();
        let mut doc = Doc::new();
        let op = doc
            .apply(Command::PlacePart {
                part: part(4, Pt { x: half, y: half }, Orient::IDENTITY, "U1", "OPAMP"),
                local_pins: op_pins,
                local_bounds: op_bounds,
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let clip = doc.copy(&[op]);
        for &orient in &Orient::ALL {
            let mut probe = doc.fork();
            let rotated = clip.reoriented(orient);
            let change = probe
                .apply(Command::Paste {
                    clip: rotated,
                    at: pt(10, 10),
                })
                .unwrap();
            let [ItemId::Part(pid)] = change.added[..] else {
                panic!("expected pasted part");
            };
            // Every pin stays on PIN_LATTICE_NM (actually GRID_NM when origin & pins share half-grid).
            for &pin in probe.pins(pid).unwrap() {
                assert_eq!(pin.x % PIN_LATTICE_NM, 0);
                assert_eq!(pin.y % PIN_LATTICE_NM, 0);
            }
        }
    }

    #[test]
    fn t7_1_unique_designators_cover_collisions_gaps_cut_and_non_numeric_names() {
        let mut doc = Doc::new();
        let place_with_des = |doc: &mut Doc, x: i32, des: &str| {
            doc.apply(Command::PlacePart {
                part: part(0, pt(x, 0), Orient::IDENTITY, des, "1k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0]
        };

        let r1 = place_with_des(&mut doc, -20, "R1");
        let _r3 = place_with_des(&mut doc, -10, "R3");
        let gnd = place_with_des(&mut doc, 0, "GND");
        let u1a = place_with_des(&mut doc, 10, "U1A");
        let empty = place_with_des(&mut doc, 20, "");
        let num = place_with_des(&mut doc, 30, "10");

        let clip = doc.copy(&[r1, gnd, u1a, empty, num]);

        // Paste #1: R1 fills gap R2; GND -> GND1; U1A -> U1A1; "" -> X1; "10" -> "1".
        let c1 = doc
            .apply(Command::Paste {
                clip: clip.clone(),
                at: pt(0, 10),
            })
            .unwrap();
        let des1: Vec<&str> = c1
            .added
            .iter()
            .filter_map(|&id| match id {
                ItemId::Part(pid) => Some(&*doc.part(pid).unwrap().designator),
                _ => None,
            })
            .collect();
        assert_eq!(des1, vec!["R2", "GND1", "U1A1", "X1", "1"]);

        // Paste #2: R1 -> R4 (skipping existing R1, R2, R3); others increment deterministically.
        let c2 = doc
            .apply(Command::Paste {
                clip: clip.clone(),
                at: pt(0, 20),
            })
            .unwrap();
        let des2: Vec<&str> = c2
            .added
            .iter()
            .filter_map(|&id| match id {
                ItemId::Part(pid) => Some(&*doc.part(pid).unwrap().designator),
                _ => None,
            })
            .collect();
        assert_eq!(des2, vec!["R4", "GND2", "U1A2", "X2", "2"]);

        // Every live part has a strictly unique designator.
        let all_des: Vec<&str> = doc.parts().map(|(_, p)| &*p.designator).collect();
        let unique: BTreeSet<&str> = all_des.iter().copied().collect();
        assert_eq!(all_des.len(), unique.len());

        // Cut-and-paste (or paste into an empty doc) preserves free designators verbatim.
        let mut fresh = Doc::new();
        let cf = fresh.apply(Command::Paste { clip, at: Pt::ZERO }).unwrap();
        let des_fresh: Vec<&str> = cf
            .added
            .iter()
            .filter_map(|&id| match id {
                ItemId::Part(pid) => Some(&*fresh.part(pid).unwrap().designator),
                _ => None,
            })
            .collect();
        assert_eq!(des_fresh, vec!["R1", "GND", "U1A", "", "10"]);
    }

    #[test]
    fn t7_1_pasted_group_preserves_internal_connectivity_and_has_no_connection_to_source() {
        // Source circuit:
        // R1 at (-6, 0): pins (-8,0), (-4,0)
        // R2 at (6, 0):  pins (4,0),  (8,0)
        // R3 at (0, -6): pins (-2,-6),(2,-6)
        // Wire T-junction at (0, 0): (-4,0)-(0,0), (0,0)-(4,0), (0,0)-(0,-6) -> R1.p1, R2.p0, R3.p1?
        // Add horizontal wire (0,-6)-(2,-6) so R3.p1 is not needed; connect (0,0)-(0,-6) to R3 pin at (0,-6)
        // Use a single-pin part P_TOP at (0,-6) and abutting parts at (-8,0) and (8,0) + separate net (20,0)-(24,0).
        let mut doc = Doc::new();
        let r1 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(-6, 0), Orient::IDENTITY, "R1", "10k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let r2 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(6, 0), Orient::IDENTITY, "R2", "22k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let gnd = doc
            .apply(Command::PlacePart {
                part: part(5, pt(0, 6), Orient::IDENTITY, "GND1", ""),
                local_pins: vec![Pt::ZERO],
                local_bounds: rect(-1, 0, 1, 2),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let gnd2 = doc
            .apply(Command::PlacePart {
                part: part(5, pt(-8, 0), Orient::IDENTITY, "GND2", ""),
                local_pins: vec![Pt::ZERO],
                local_bounds: rect(-1, 0, 1, 2),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let w_res = doc
            .apply(Command::AddWires(vec![
                Wire {
                    a: pt(-4, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(0, 0),
                    b: pt(0, 6),
                },
            ]))
            .unwrap();
        let mut group = vec![r1, r2, gnd, gnd2];
        group.extend(w_res.added.iter().copied());

        // Add an external wire/part that is NOT copied.
        let _r_ext = doc
            .apply(Command::PlacePart {
                part: part(0, pt(20, 0), Orient::IDENTITY, "R9", "1k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap();

        let source_parts: Vec<PartId> = [r1, r2, gnd, gnd2]
            .into_iter()
            .map(|id| match id {
                ItemId::Part(p) => p,
                _ => unreachable!(),
            })
            .collect();

        // Capture source pin partition (list of (part_idx, pin_idx) equivalence classes).
        let net_classes = |doc: &Doc, parts: &[PartId]| -> Vec<Vec<(usize, usize)>> {
            let mut by_net: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
            for (pi, &pid) in parts.iter().enumerate() {
                for (pin_i, &p) in doc.pins(pid).unwrap().iter().enumerate() {
                    let net = doc.net_at(p).expect("every pin has a net");
                    by_net.entry(net.0).or_default().push((pi, pin_i));
                }
            }
            let mut classes: Vec<_> = by_net.into_values().collect();
            classes.sort();
            classes
        };
        let source_classes = net_classes(&doc, &source_parts);

        let clip = doc.copy(&group);
        for orient in [Orient::IDENTITY, Orient::ROT_90, Orient::FLIP_X] {
            let mut probe = doc.fork();
            let before = probe.fork();
            let change = probe
                .apply(Command::Paste {
                    clip: clip.reoriented(orient),
                    at: pt(0, 30),
                })
                .unwrap();
            assert_inv10_and_normal_form(&probe);

            let pasted_parts: Vec<PartId> = change
                .added
                .iter()
                .filter_map(|&id| match id {
                    ItemId::Part(p) => Some(p),
                    _ => None,
                })
                .collect();
            assert_eq!(pasted_parts.len(), source_parts.len());

            // Internal connectivity matches source pin-for-pin.
            assert_eq!(net_classes(&probe, &pasted_parts), source_classes);

            // Zero shared nets with any source pin or external part pin.
            let mut source_nets = BTreeSet::new();
            for &pid in &source_parts {
                for &p in probe.pins(pid).unwrap() {
                    source_nets.insert(probe.net_at(p).unwrap().0);
                }
            }
            for &pid in &pasted_parts {
                for &p in probe.pins(pid).unwrap() {
                    assert!(
                        !source_nets.contains(&probe.net_at(p).unwrap().0),
                        "pasted part must not share a net with source"
                    );
                }
            }

            // Paste then undo satisfies INV4 (exact content + IDs) and INV10.
            let after = probe.fork();
            assert!(probe.undo().is_some());
            assert!(probe.content_eq(&before));
            assert_inv10_and_normal_form(&probe);
            assert!(probe.redo().is_some());
            assert!(probe.content_eq(&after));
            assert_inv10_and_normal_form(&probe);
        }
    }

    #[test]
    fn t7_1_paste_onto_existing_wire_splits_normalises_and_undoes_exact_ids() {
        let mut doc = Doc::new();
        // Existing long horizontal wire from (0, 10) to (10, 10).
        doc.apply(Command::AddWires(vec![Wire {
            a: pt(0, 10),
            b: pt(10, 10),
        }]))
        .unwrap();

        // Copy a part with a pin + label from (0, 0), then paste so a pin and label land on
        // the interior of the existing wire at (3, 10) and (7, 10).
        let p0 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(0, 0), Orient::IDENTITY, "R1", "1k"),
                local_pins: vec![Pt { x: -2 * G, y: 0 }, Pt { x: 2 * G, y: 0 }],
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let l0 = doc
            .apply(Command::AddLabel(Label {
                at: pt(2, 0),
                text: "SIG".into(),
            }))
            .unwrap()
            .added[0];
        let clip = doc.copy(&[p0, l0]);

        let before = doc.fork();
        let change = doc
            .apply(Command::Paste {
                clip,
                at: pt(5, 10),
            })
            .unwrap();
        assert_eq!(change.added.len(), 4); // 1 part, 1 label, 2 extra split wire pieces
        assert_inv10_and_normal_form(&doc);
        assert_eq!(doc.wires().count(), 3);
        let after = doc.fork();

        assert!(doc.undo().is_some());
        assert!(doc.content_eq(&before), "INV4: undo restores exact IDs");
        assert_inv10_and_normal_form(&doc);

        assert!(doc.redo().is_some());
        assert!(doc.content_eq(&after), "INV4: redo restores exact IDs");
        assert_inv10_and_normal_form(&doc);
    }

    #[test]
    fn t7_1_out_of_bounds_paste_is_refused_without_mutation() {
        let mut doc = Doc::new();
        let p0 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(0, 0), Orient::IDENTITY, "R1", "1k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let w0 = doc
            .apply(Command::AddWires(vec![Wire {
                a: pt(-2, 0),
                b: pt(2, 0),
            }]))
            .unwrap()
            .added[0];
        let l0 = doc
            .apply(Command::AddLabel(Label {
                at: pt(0, 0),
                text: "NET".into(),
            }))
            .unwrap()
            .added[0];
        let clip = doc.copy(&[p0, w0, l0]);

        let snap = doc.snapshot();
        let rev = doc.revision();
        let cursor = doc.history_cursor();
        let err = doc
            .apply(Command::Paste {
                clip,
                at: Pt {
                    x: SHEET_LIMIT_NM - G,
                    y: 0,
                },
            })
            .unwrap_err();
        assert_eq!(err, CommandError::OutOfBounds);
        assert_eq!(doc.revision(), rev);
        assert_eq!(doc.history_cursor(), cursor);
        assert_eq!(doc.snapshot(), snap);
    }

    #[test]
    fn t7_1_two_pass_designators_preserve_free_names() {
        let mut src = Doc::new();
        let p1 = src
            .apply(Command::PlacePart {
                part: part(0, pt(0, 0), Orient::IDENTITY, "R1", "1k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let p2 = src
            .apply(Command::PlacePart {
                part: part(0, pt(0, 4), Orient::IDENTITY, "R2", "2k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let clip = src.copy(&[p1, p2]);

        let mut doc = Doc::new();
        doc.apply(Command::PlacePart {
            part: part(0, pt(-10, 0), Orient::IDENTITY, "R1", "10k"),
            local_pins: two_pin_pins(),
            local_bounds: two_pin_bounds(),
            def: crate::PlaceDef::Unmodelled,
            params: crate::Params::new(),
        })
        .unwrap();

        let change = doc
            .apply(Command::Paste {
                clip: clip.clone(),
                at: pt(10, 0),
            })
            .unwrap();
        let pasted_des: Vec<&str> = change
            .added
            .iter()
            .filter_map(|&id| match id {
                ItemId::Part(pid) => Some(&*doc.part(pid).unwrap().designator),
                _ => None,
            })
            .collect();
        assert_eq!(pasted_des, vec!["R3", "R2"]);

        let mut fresh = Doc::new();
        let cf = fresh.apply(Command::Paste { clip, at: Pt::ZERO }).unwrap();
        let fresh_des: Vec<&str> = cf
            .added
            .iter()
            .filter_map(|&id| match id {
                ItemId::Part(pid) => Some(&*fresh.part(pid).unwrap().designator),
                _ => None,
            })
            .collect();
        assert_eq!(fresh_des, vec!["R1", "R2"]);
    }

    #[test]
    fn t7_1_endpoint_touched_wire_is_not_refed_or_split() {
        let mut doc = Doc::new();
        let existing_wire = Wire {
            a: pt(0, 10),
            b: pt(6, 10),
        };
        let ItemId::Wire(wid) = doc
            .apply(Command::AddWires(vec![existing_wire]))
            .unwrap()
            .added[0]
        else {
            unreachable!();
        };

        let p0 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(0, 0), Orient::IDENTITY, "R1", "1k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let clip = doc.copy(&[p0]);

        let before = doc.fork();
        // Right pin of R1 lands cleanly at `pt(0, 10)` (the endpoint of `wid`, not its interior).
        let change = doc
            .apply(Command::Paste {
                clip,
                at: pt(-2, 10),
            })
            .unwrap();
        assert_eq!(change.added.len(), 1);
        assert!(matches!(change.added[0], ItemId::Part(_)));
        assert!(change.removed.is_empty());
        assert_eq!(doc.wire(wid).copied(), Some(existing_wire));
        assert_eq!(doc.wires().count(), 1);
        assert_inv10_and_normal_form(&doc);

        let after = doc.fork();
        assert!(doc.undo().is_some());
        assert!(doc.content_eq(&before));
        assert_inv10_and_normal_form(&doc);
        assert!(doc.redo().is_some());
        assert!(doc.content_eq(&after));
        assert_inv10_and_normal_form(&doc);
    }

    #[test]
    fn t7_1_pasted_labelled_group_preserves_label_text_and_isolates_unlabelled_nets() {
        let mut doc = Doc::new();
        let r1 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(-6, 0), Orient::IDENTITY, "R1", "10k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let r2 = doc
            .apply(Command::PlacePart {
                part: part(0, pt(6, 0), Orient::IDENTITY, "R2", "22k"),
                local_pins: two_pin_pins(),
                local_bounds: two_pin_bounds(),
                def: crate::PlaceDef::Unmodelled,
                params: crate::Params::new(),
            })
            .unwrap()
            .added[0];
        let wires = doc
            .apply(Command::AddWires(vec![
                Wire {
                    a: pt(-10, 0),
                    b: pt(-8, 0),
                },
                Wire {
                    a: pt(-4, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(8, 0),
                    b: pt(10, 0),
                },
            ]))
            .unwrap()
            .added;
        let l_vcc = doc
            .apply(Command::AddLabel(Label {
                at: pt(-10, 0),
                text: "VCC".into(),
            }))
            .unwrap()
            .added[0];
        let l_gnd = doc
            .apply(Command::AddLabel(Label {
                at: pt(10, 0),
                text: "GND".into(),
            }))
            .unwrap()
            .added[0];

        let mut group = vec![r1, r2, l_vcc, l_gnd];
        group.extend(wires);
        let clip = doc.copy(&group);

        let before = doc.fork();
        let paste = doc
            .apply(Command::Paste {
                clip,
                at: pt(0, 20),
            })
            .unwrap();
        assert_inv10_and_normal_form(&doc);

        // Unlabelled internal bridge (`(-4, 20)-(4, 20)`) is isolated from `(-4, 0)-(4, 0)`.
        let src_internal = doc.net_at(pt(-4, 0)).expect("source internal net");
        let pasted_internal = doc.net_at(pt(-4, 20)).expect("pasted internal net");
        assert_ne!(src_internal, pasted_internal);
        assert_eq!(doc.net_at(pt(4, 20)), Some(pasted_internal));

        // Pasted labels retain their text verbatim and share NetIds with source labels (D2 / INV10).
        let pasted_vcc_label = paste
            .added
            .iter()
            .copied()
            .find(|&id| match id {
                ItemId::Label(lid) => doc.label(lid).is_some_and(|l| l.text.as_ref() == "VCC"),
                _ => false,
            })
            .expect("pasted VCC label");
        let vcc_net = doc.net_at(pt(-10, 0)).expect("source VCC net");
        assert_eq!(doc.net_at(pt(-10, 20)), Some(vcc_net));
        let gnd_net = doc.net_at(pt(10, 0)).expect("source GND net");
        assert_eq!(doc.net_at(pt(10, 20)), Some(gnd_net));

        // Removing the pasted VCC label separates the pasted VCC net from source VCC (INV10).
        doc.apply(Command::Delete {
            items: vec![pasted_vcc_label],
            with_wires: false,
        })
        .unwrap();
        let separated_vcc = doc.net_at(pt(-10, 20)).expect("local wire net");
        assert_ne!(Some(vcc_net), Some(separated_vcc));
        assert_eq!(doc.net_at(pt(-10, 0)), Some(vcc_net));
        assert_inv10_and_normal_form(&doc);

        assert!(doc.undo().is_some());
        assert!(doc.undo().is_some());
        assert!(doc.content_eq(&before));
        assert_inv10_and_normal_form(&doc);
    }

    #[derive(Clone, Debug)]
    enum GenStep {
        Place(i32, i32),
        Wire(i32, i32, bool),
        Label(i32, i32, u8),
    }

    fn gen_setup() -> impl Strategy<Value = Vec<GenStep>> {
        prop::collection::vec(
            prop_oneof![
                3 => (-8i32..=8, -8i32..=8).prop_map(|(x, y)| GenStep::Place(x, y)),
                4 => (-8i32..=8, -8i32..=8, any::<bool>()).prop_map(|(x, y, h)| GenStep::Wire(x, y, h)),
                2 => (-8i32..=8, -8i32..=8, 0u8..3).prop_map(|(x, y, n)| GenStep::Label(x, y, n)),
            ],
            1..20,
        )
    }

    proptest! {
            #![proptest_config(crate::proptest_config(128))]
            #[test]
            fn t7_1_paste_undo_redo_satisfies_inv2_inv4_inv10(
                setup in gen_setup(),
                mask in prop::collection::vec(any::<bool>(), 1..30),
                tx in -12i32..=12,
                ty in -12i32..=12,
                orient_idx in 0usize..8,
            ) {
                let mut setup = setup;
                // Place parts and labels first so subsequent AddWires splits across all pin and
                // label interiors (`WINV3`, `WD9`).
                setup.sort_by_key(|step| match step {
                    GenStep::Place(..) => 0u8,
                    GenStep::Label(..) => 1u8,
                    GenStep::Wire(..) => 2u8,
                });
                let mut doc = Doc::new();
                let names = ["A", "B", "GND"];
                let mut des_n = 1u32;
                for step in setup {
                    match step {
                        GenStep::Place(x, y) => {
                            let _ = doc.apply(Command::PlacePart {
                                part: part(0, Pt { x: x * PIN_LATTICE_NM, y: y * PIN_LATTICE_NM }, Orient::IDENTITY, &format!("R{des_n}"), "1k"),
                                local_pins: vec![Pt::ZERO, Pt { x: PIN_LATTICE_NM, y: 0 }],
                                local_bounds: Rect::from_points(Pt { x: -PIN_LATTICE_NM, y: -PIN_LATTICE_NM }, Pt { x: 2 * PIN_LATTICE_NM, y: PIN_LATTICE_NM }).unwrap(),
        def: crate::PlaceDef::Unmodelled,
        params: crate::Params::new(),
    });
                            des_n += 1;
                        }
                        GenStep::Wire(x, y, h) => {
                            let a = Pt { x: x * PIN_LATTICE_NM, y: y * PIN_LATTICE_NM };
                            let b = if h {
                                Pt { x: (x + 2) * PIN_LATTICE_NM, y: y * PIN_LATTICE_NM }
                            } else {
                                Pt { x: x * PIN_LATTICE_NM, y: (y + 2) * PIN_LATTICE_NM }
                            };
                            let _ = doc.apply(Command::AddWires(vec![Wire { a, b }]));
                        }
                        GenStep::Label(x, y, n) => {
                            let _ = doc.apply(Command::AddLabel(Label {
                                at: Pt { x: x * PIN_LATTICE_NM, y: y * PIN_LATTICE_NM },
                                text: names[n as usize].into(),
                            }));
                        }
                    }
                }
                let live: Vec<ItemId> = doc
                    .parts()
                    .map(|(id, _)| ItemId::Part(id))
                    .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
                    .chain(doc.labels().map(|(id, _)| ItemId::Label(id)))
                    .collect();
                let selected: Vec<ItemId> = live
                    .iter()
                    .enumerate()
                    .filter_map(|(i, &id)| mask.get(i).copied().unwrap_or(i % 2 == 0).then_some(id))
                    .collect();
                let clip = doc.copy(&selected).reoriented(Orient::ALL[orient_idx]);
                let before = doc.fork();
                let before_snap = doc.snapshot();
                let pre_rev = doc.revision();
                match doc.apply(Command::Paste {
                    clip,
                    at: pt(tx, ty),
                }) {
                    Ok(change) if change.dirty.is_none() => {
                        prop_assert_eq!(doc.revision(), pre_rev);
                        prop_assert_eq!(doc.snapshot(), before_snap);
                        assert_inv10_and_normal_form(&doc);
                    }
                    Ok(_) => {
                        prop_assert_eq!(doc.revision(), pre_rev + 1);
                        let all_des: Vec<&str> = doc.parts().map(|(_, p)| &*p.designator).collect();
                        let uniq: BTreeSet<&str> = all_des.iter().copied().collect();
                        prop_assert_eq!(all_des.len(), uniq.len(), "designators must be unique");
                        assert_inv10_and_normal_form(&doc);

                        let after = doc.fork();
                        prop_assert!(doc.undo().is_some());
                        prop_assert!(doc.content_eq(&before));
                        assert_inv10_and_normal_form(&doc);

                        prop_assert!(doc.redo().is_some());
                        prop_assert!(doc.content_eq(&after));
                        assert_inv10_and_normal_form(&doc);
                    }
                    Err(err) => {
                        prop_assert!(
                            false,
                            "in-bounds paste of valid copied items must succeed, got {err:?}"
                        );
                    }
                }
            }
        }

    /// P2.2 (T7.1 handoff): `ClipPart` carries the embedded `EmbeddedPartDef` and `Params`
    /// across same-doc paste (deduplicating referenced definitions), cross-doc paste and
    /// cut-and-paste, with exact `INV4` undo/redo.
    #[test]
    fn p2_2_copy_paste_preserves_embedded_def_and_params() {
        let pins = two_pin_pins();
        let bounds = two_pin_bounds();
        let r_def = Arc::new(EmbeddedPartDef {
            source_id: "omnia:resistor".into(),
            content_hash: Some([0x33; 32]),
            name: "Resistor".into(),
            category: "Passives".into(),
            description: "Ideal resistor".into(),
            designator_prefix: "R".into(),
            symbol: SymbolId(0),
            local_pins: pins.clone(),
            local_bounds: bounds,
            pins: vec![
                crate::EmbeddedPin {
                    number: "1".into(),
                    name: "a".into(),
                    role: crate::PinRole::Passive,
                },
                crate::EmbeddedPin {
                    number: "2".into(),
                    name: "b".into(),
                    role: crate::PinRole::Passive,
                },
            ],
            params: vec![
                crate::EmbeddedParamSpec {
                    name: "resistance".into(),
                    label: "Resistance".into(),
                    kind: crate::ParamKind::Real {
                        unit: Some("ohm".into()),
                        min: Some(crate::FiniteF64::new(1e-6).unwrap()),
                        max: None,
                    },
                    default: crate::ParamValue::Real {
                        raw: "10k".into(),
                        value: crate::parse_value("10k", Some("ohm")).unwrap(),
                    },
                    optional: false,
                },
                crate::EmbeddedParamSpec {
                    name: "tc1".into(),
                    label: "Tempco".into(),
                    kind: crate::ParamKind::Real {
                        unit: Some("1/K".into()),
                        min: None,
                        max: None,
                    },
                    default: crate::ParamValue::Real {
                        raw: "0".into(),
                        value: crate::parse_value("0", Some("1/K")).unwrap(),
                    },
                    optional: true,
                },
            ],
            primary_param: Some("resistance".into()),
            sim: crate::EmbeddedSimBinding::Primitive {
                kind: crate::PrimitiveKind::Resistor,
                terminals: vec![0, 1],
            },
        });

        let mut params = Params::new();
        params.insert(
            "resistance".into(),
            crate::ParamValue::Real {
                raw: "4.7k".into(),
                value: crate::parse_value("4.7k", Some("ohm")).unwrap(),
            },
        );
        params.insert(
            "tc1".into(),
            crate::ParamValue::Real {
                raw: "1e-3".into(),
                value: crate::parse_value("1e-3", Some("1/K")).unwrap(),
            },
        );

        let mut doc = Doc::new();
        let r1_item = doc
            .apply(Command::PlacePart {
                part: part(0, pt(0, 0), Orient::IDENTITY, "R1", "4.7k"),
                local_pins: pins,
                local_bounds: bounds,
                def: crate::PlaceDef::Embed(Arc::clone(&r_def)),
                params: params.clone(),
            })
            .unwrap()
            .added[0];
        let ItemId::Part(r1) = r1_item else {
            unreachable!();
        };
        let orig_def_id = doc.part_def_id(r1).unwrap();

        let clip = doc.copy(&[r1_item]);
        assert_eq!(clip.parts().len(), 1);
        assert_eq!(clip.parts()[0].def, r_def);
        assert_eq!(clip.parts()[0].params, params);

        // Same-doc paste deduplicates onto `orig_def_id` and preserves overrides + value.
        let before = doc.fork();
        let c = doc
            .apply(Command::Paste {
                clip: clip.clone(),
                at: pt(0, 10),
            })
            .unwrap();
        let [ItemId::Part(r2)] = c.added[..] else {
            panic!("expected pasted part");
        };
        assert_eq!(doc.part_defs().count(), 1);
        assert_eq!(doc.part_def_id(r2), Some(orig_def_id));
        assert_eq!(doc.part_params(r2), Some(&params));
        assert_eq!(&*doc.part(r2).unwrap().value, "4.7k");

        let after = doc.fork();
        doc.undo().unwrap();
        assert!(doc.content_eq(&before));
        doc.redo().unwrap();
        assert!(doc.content_eq(&after));

        // Cross-doc paste into a brand-new Doc carries the full definition and overrides.
        let mut fresh = Doc::new();
        let cf = fresh
            .apply(Command::Paste {
                clip: clip.reoriented(Orient::ROT_90),
                at: pt(4, 4),
            })
            .unwrap();
        let [ItemId::Part(rf)] = cf.added[..] else {
            panic!("expected pasted part in fresh doc");
        };
        assert_eq!(fresh.part_defs().count(), 1);
        let fresh_def_id = fresh.part_def_id(rf).unwrap();
        assert_eq!(fresh.part_def(fresh_def_id), Some(&*r_def));
        assert_eq!(fresh.part_params(rf), Some(&params));
        assert_eq!(&*fresh.part(rf).unwrap().value, "4.7k");

        // Cut-and-paste: deleting all parts on `orig_def_id` makes it retained, so pasting
        // allocates a fresh definition id (`PD8`) while preserving the definition and params.
        doc.apply(Command::Delete {
            items: vec![ItemId::Part(r1), ItemId::Part(r2)],
            with_wires: false,
        })
        .unwrap();
        let cc = doc
            .apply(Command::Paste {
                clip,
                at: pt(0, 20),
            })
            .unwrap();
        let [ItemId::Part(r3)] = cc.added[..] else {
            panic!("expected pasted part after cut");
        };
        let cut_def_id = doc.part_def_id(r3).unwrap();
        assert_ne!(cut_def_id, orig_def_id);
        assert_eq!(doc.part_def(cut_def_id), Some(&*r_def));
        assert_eq!(doc.part_params(r3), Some(&params));
    }
}
