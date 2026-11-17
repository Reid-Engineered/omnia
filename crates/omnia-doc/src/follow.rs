//! Pure WD2/WD3 follow planning. Commands consume this plan in W2.2.
use crate::{
    Anchor, Boundary, Doc, ItemId, LabelId, Orient, PartId, Pt, Run, Wire, WireId,
    points::End,
    prefs::FollowPolicy,
    route::{Axis, Blocker, RouteScratch, check_new_wires, route},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isometry {
    Translate(Pt),
    Reorient { pivot: Pt, by: Orient },
}
impl Isometry {
    pub fn apply(self, p: Pt) -> Result<Pt, crate::GeomError> {
        match self {
            Self::Translate(d) => p.checked_add(d),
            Self::Reorient { pivot, by } => by.checked_apply_around(p, pivot),
        }
    }
    fn wire(self, w: Wire) -> Result<Wire, crate::GeomError> {
        Ok(Wire {
            a: self.apply(w.a)?,
            b: self.apply(w.b)?,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFate {
    Translated,
    Rerouted,
    Detached,
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FollowPlan {
    pub remove: Vec<WireId>,
    pub add: Vec<Wire>,
    pub labels: Vec<(LabelId, Pt)>,
    pub fates: Vec<(Pt, RunFate)>,
}
impl FollowPlan {
    fn clear(&mut self) {
        self.remove.clear();
        self.add.clear();
        self.labels.clear();
        self.fates.clear();
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowError {
    NoRoute { from: Pt, to: Pt, blocker: Blocker },
}
impl std::fmt::Display for FollowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "follow refused: {self:?}")
    }
}
impl std::error::Error for FollowError {}

// Scan only selected incidences before allocating Boundary's selected-wire list.
fn has_boundary(doc: &Doc, items: &[ItemId]) -> bool {
    let meets = |p| {
        doc.points
            .at(p)
            .iter()
            .any(|e| matches!(e, End::Wire(w) if !items.contains(&ItemId::Wire(*w))))
    };
    items.iter().any(|item| match *item {
        ItemId::Part(id) => doc
            .pins(id)
            .is_some_and(|pins| pins.iter().copied().any(meets)),
        ItemId::Wire(id) => doc.wire(id).is_some_and(|w| meets(w.a) || meets(w.b)),
        ItemId::Label(_) => false,
    })
}
fn allows(doc: &Doc, items: &[ItemId], run: &Run, policy: FollowPolicy) -> bool {
    [run.a, run.b]
        .into_iter()
        .filter(|(_, a)| *a == Anchor::Boundary)
        .all(|(p, _)| {
            let mut part_found = false;
            for e in doc.points.at(p) {
                if let End::Pin { part, .. } = *e
                    && items.contains(&ItemId::Part(part))
                {
                    part_found = true;
                    if !policy.part_follows(doc.connected_pins(part) as usize) {
                        return false;
                    }
                }
            }
            // WD3: the wire policy is the fallback for a boundary without a moved part.
            part_found || policy.wire_follows()
        })
}

// Preserve original identities in this private final-geometry view, including holes.
// Nothing in the caller's document (including history, revision and index stamps) changes.
fn view(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    runs: &[Run],
    detached: &[bool],
) -> Result<Doc, Blocker> {
    let mut v = Doc::new();
    v.parts.slots.resize(doc.parts.slots.len(), None);
    v.wires.slots.resize(doc.wires.slots.len(), None);
    for (i, s) in doc.parts.slots.iter().enumerate() {
        if let Some(s) = s {
            let mut s = s.clone();
            if items.contains(&ItemId::Part(PartId(i as u32))) {
                s.part.at = t.apply(s.part.at).map_err(|_| Blocker::Sheet)?;
                if let Isometry::Reorient { by, .. } = t {
                    s.part.orient = s.part.orient.then(by);
                }
                for p in &mut s.pins {
                    *p = t.apply(*p).map_err(|_| Blocker::Sheet)?;
                }
            }
            v.restore_stored_part_raw(PartId(i as u32), s)
                .map_err(|_| Blocker::Sheet)?;
        }
    }
    for (id, &w) in doc.wires() {
        if runs
            .iter()
            .enumerate()
            .any(|(i, r)| !detached[i] && r.wires.contains(&id))
        {
            continue;
        }
        let w = if items.contains(&ItemId::Wire(id)) {
            t.wire(w).map_err(|_| Blocker::Sheet)?
        } else {
            w
        };
        v.restore_wire_raw(id, w).map_err(|_| Blocker::Sheet)?;
    }
    Ok(v)
}

/// WD3: rigid first, unanimous boundary policy, reroute or detach. On refusal `plan` is empty.
/// Labels on rigid corners move once; rerouted label placement belongs to W3.6.
/// Nonempty plans currently build an isolated document view; no timing budget is certified.
pub fn plan_follow(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    policy: FollowPolicy,
    plan: &mut FollowPlan,
) -> Result<(), FollowError> {
    plan.clear();
    if !has_boundary(doc, items) {
        return Ok(());
    }
    let result = build(doc, items, t, policy, plan);
    if result.is_err() {
        plan.clear();
    }
    result
}
fn build(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    policy: FollowPolicy,
    plan: &mut FollowPlan,
) -> Result<(), FollowError> {
    let mut boundary = Boundary::default();
    doc.boundary_points(items, &mut boundary);
    let mut runs = Vec::new();
    doc.affected_runs(&boundary, &mut runs);
    let mut detached = vec![false; runs.len()];
    let mut scratch = RouteScratch::new();
    // A failed rigid attempt under detach policy introduces old geometry. Restart so every
    // earlier run sees it too. Each restart fixes at least one fate, so termination is bounded.
    'restart: loop {
        plan.clear();
        let mut v =
            view(doc, items, t, &runs, &detached).map_err(|blocker| FollowError::NoRoute {
                from: runs[0].a.0,
                to: runs[0].b.0,
                blocker,
            })?;
        let mut sources: Vec<WireId> = (0..v.wires.slots.len()).map(|i| WireId(i as u32)).collect();
        // Removing a run also removes its otherwise free far end. Reserve every final
        // endpoint before planning: an earlier corner must not capture a later run's end.
        // Zero-area marker parts affect validity only, not body/crossing scores.
        let mut end_markers = Vec::new();
        let mut reservations = Vec::new();
        for (i, r) in runs.iter().enumerate() {
            if detached[i] {
                continue;
            }
            for (end, source) in [(r.a, r.wires[0]), (r.b, *r.wires.last().unwrap())] {
                let at = if end.1 == Anchor::Boundary {
                    t.apply(end.0).map_err(|_| FollowError::NoRoute {
                        from: r.a.0,
                        to: r.b.0,
                        blocker: Blocker::Sheet,
                    })?
                } else {
                    end.0
                };
                let source = v
                    .points
                    .at(at)
                    .iter()
                    .filter_map(|end| match end {
                        End::Wire(id) => Some(*id),
                        End::Pin { .. } => None,
                    })
                    .chain(std::iter::once(source))
                    .min_by_key(|id| id.0)
                    .unwrap();
                reservations.push((at, source));
            }
        }
        // Marker pin ids must preserve the original lowest-wire-id tie at a shared end.
        reservations.sort_unstable_by_key(|(at, source)| (source.0, at.x, at.y));
        for (at, source) in reservations {
            let (id, _) = v
                .insert_part_raw(
                    crate::Part {
                        symbol: crate::SymbolId(0),
                        at,
                        orient: Orient::IDENTITY,
                        designator: "".into(),
                        value: "".into(),
                    },
                    &[Pt::ZERO],
                    crate::Rect::from_points(Pt::ZERO, Pt::ZERO).unwrap(),
                )
                .expect("bounded endpoint reservation");
            end_markers.push((id, source));
        }
        for (i, r) in runs.iter().enumerate() {
            if detached[i] {
                plan.fates.push((r.a.0, RunFate::Detached));
                continue;
            }
            let a = t.apply(r.a.0).map_err(|_| FollowError::NoRoute {
                from: r.a.0,
                to: r.b.0,
                blocker: Blocker::Sheet,
            })?;
            let b = if r.b.1 == Anchor::Boundary {
                t.apply(r.b.0).map_err(|_| FollowError::NoRoute {
                    from: r.a.0,
                    to: r.b.0,
                    blocker: Blocker::Sheet,
                })?
            } else {
                r.b.0
            };
            let mut wires = Vec::new();
            let rigid = r.b.1 == Anchor::Boundary
                && r.wires
                    .iter()
                    .try_for_each(|id| {
                        wires.push(
                            t.wire(*doc.wire(*id).unwrap())
                                .map_err(|_| Blocker::Sheet)?,
                        );
                        Ok::<_, Blocker>(())
                    })
                    .is_ok()
                && check_new_wires(&v, &wires, [a, b]).is_ok();
            let fate = if rigid {
                RunFate::Translated
            } else {
                wires.clear();
                if !allows(doc, items, r, policy) {
                    detached[i] = true;
                    continue 'restart;
                }
                let old = doc.wire(r.wires[0]).unwrap();
                let first_leg = if old.a.y == old.b.y {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                route(&v, a, b, first_leg, &mut scratch, &mut wires).map_err(|blocker| {
                    let remap = |w: WireId| sources[w.0 as usize];
                    let blocker = match blocker {
                        Blocker::Pin { part, .. }
                            if end_markers.iter().any(|(id, _)| *id == part) =>
                        {
                            Blocker::WireEnd(
                                end_markers.iter().find(|(id, _)| *id == part).unwrap().1,
                            )
                        }
                        Blocker::WireEnd(w) => Blocker::WireEnd(remap(w)),
                        Blocker::WireBody(w) => Blocker::WireBody(remap(w)),
                        Blocker::Overlap(w) => Blocker::Overlap(remap(w)),
                        b => b,
                    };
                    FollowError::NoRoute {
                        from: r.a.0,
                        to: r.b.0,
                        blocker,
                    }
                })?;
                RunFate::Rerouted
            };
            if rigid {
                for &id in &r.labels {
                    if !items.contains(&ItemId::Label(id)) {
                        let p = t.apply(doc.label(id).unwrap().at).map_err(|_| {
                            FollowError::NoRoute {
                                from: r.a.0,
                                to: r.b.0,
                                blocker: Blocker::Sheet,
                            }
                        })?;
                        plan.labels.push((id, p));
                    }
                }
            }
            // Insertion makes subsequent runs validate against every already planned segment.
            for &w in &wires {
                v.insert_wire_raw(w).expect("validated candidate");
                sources.push(r.wires[0]);
            }
            plan.remove.extend_from_slice(&r.wires);
            plan.add.extend(wires);
            plan.fates.push((r.a.0, fate));
        }
        plan.labels.sort_unstable_by_key(|(id, _)| id.0);
        plan.labels.dedup_by_key(|(id, _)| *id);
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GRID_NM, Label, Part, Rect, SymbolId};
    use proptest::prelude::*;
    fn p(x: i32, y: i32) -> Pt {
        Pt {
            x: x * GRID_NM,
            y: y * GRID_NM,
        }
    }
    fn part(d: &mut Doc, pins: &[Pt]) -> PartId {
        d.insert_part(
            Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R".into(),
                value: "".into(),
            },
            pins,
            Rect::from_points(Pt::ZERO, Pt::ZERO).unwrap(),
        )
        .unwrap()
    }
    fn wire(d: &mut Doc, a: Pt, b: Pt) -> WireId {
        d.insert_wire(Wire { a, b }).unwrap()
    }
    fn policy(invert: bool) -> FollowPolicy {
        FollowPolicy::new(true, 10, invert).unwrap()
    }
    fn plan(d: &Doc, items: &[ItemId], t: Isometry, pol: FollowPolicy) -> FollowPlan {
        let before = format!("{d:?}");
        let mut out = FollowPlan::default();
        plan_follow(d, items, t, pol, &mut out).unwrap();
        assert_eq!(before, format!("{d:?}"));
        out
    }
    fn fan(n: i32) -> (Doc, PartId) {
        let mut d = Doc::new();
        let pins: Vec<_> = (0..n).map(|y| p(0, y * 4)).collect();
        let id = part(&mut d, &pins);
        for a in pins {
            let b = Pt {
                x: a.x - 6 * GRID_NM,
                ..a
            };
            part(&mut d, &[b]);
            wire(&mut d, a, b);
        }
        (d, id)
    }
    #[test]
    fn tc_move_01_to_05_threshold_and_modifier() {
        for (n, invert, fate) in [
            (2, false, RunFate::Rerouted),
            (9, false, RunFate::Rerouted),
            (10, false, RunFate::Detached),
            (10, true, RunFate::Rerouted),
            (2, true, RunFate::Detached),
        ] {
            let (d, id) = fan(n);
            let out = plan(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(invert),
            );
            assert_eq!(out.fates.len(), n as usize);
            assert!(out.fates.iter().all(|(_, f)| *f == fate));
            if fate == RunFate::Detached {
                assert!(out.remove.is_empty() && out.add.is_empty());
            } else {
                for y in 0..n {
                    assert!(
                        out.add
                            .iter()
                            .any(|w| w.a == p(1, y * 4) || w.b == p(1, y * 4))
                    );
                    assert!(
                        out.add
                            .iter()
                            .any(|w| w.a == p(-6, y * 4) || w.b == p(-6, y * 4))
                    );
                }
            }
        }
    }
    fn bent() -> (Doc, Vec<ItemId>) {
        let mut d = Doc::new();
        let a = part(&mut d, &[p(0, 0)]);
        let b = part(&mut d, &[p(4, 0)]);
        wire(&mut d, p(0, 0), p(0, 2));
        wire(&mut d, p(0, 2), p(4, 2));
        wire(&mut d, p(4, 2), p(4, 0));
        (d, vec![ItemId::Part(a), ItemId::Part(b)])
    }
    #[test]
    fn tc_move_06_07_09_rigid_and_labels() {
        let (mut d, mut items) = bent();
        let label = d
            .insert_label(Label {
                at: p(0, 2),
                text: "net".into(),
            })
            .unwrap();
        let t = Isometry::Translate(p(1, 0));
        let out = plan(&d, &items, t, policy(false));
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Translated)]);
        assert_eq!(
            out.add,
            d.wires()
                .map(|(_, w)| t.wire(*w).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(out.labels, vec![(label, p(1, 2))]);
        items.reverse();
        assert_eq!(out, plan(&d, &items, t, policy(true)));
    }
    #[test]
    fn tc_move_08_rigid_pin_blocker_reroutes() {
        let (mut d, items) = bent();
        part(&mut d, &[p(3, 2)]);
        let out = plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false));
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Rerouted)]);
        assert_eq!(out.remove.len(), 3);
        assert!(d.wires().all(|(id, _)| out.remove.contains(&id)));
        assert_eq!(
            out.add,
            vec![Wire {
                a: p(1, 0),
                b: p(5, 0)
            }]
        );
    }
    #[test]
    fn tc_move_10_unanimous_policy_after_failed_rigid() {
        for reverse_thresholds in [false, true] {
            let (mut d, mut items) = bent();
            let selected: Vec<_> = items
                .iter()
                .map(|item| match *item {
                    ItemId::Part(id) => id,
                    _ => unreachable!(),
                })
                .collect();
            let shared: Vec<_> = d.wires().map(|(id, _)| id).collect();
            for (index, &id) in selected.iter().enumerate() {
                let n = if (index == 0) ^ reverse_thresholds {
                    8
                } else {
                    9
                };
                let (mut stored, _) = d.remove_stored_part_raw(id).unwrap();
                for k in 0..n {
                    let at = p(20 + id.0 as i32 * 5, k * 3);
                    stored.pins.push(at);
                    wire(&mut d, at, p(21 + id.0 as i32 * 5, k * 3));
                }
                d.restore_stored_part_raw(id, stored).unwrap();
            }
            part(&mut d, &[p(3, 2)]);
            let out = plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false));
            assert_eq!(
                out.fates.iter().find(|(at, _)| *at == p(0, 0)).unwrap().1,
                RunFate::Detached
            );
            assert!(shared.iter().all(|id| !out.remove.contains(id)));
            assert!(
                out.add
                    .iter()
                    .all(|w| w.a.x >= 20 * GRID_NM && w.b.x >= 20 * GRID_NM)
            );
            items.reverse();
            assert_eq!(
                out,
                plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false))
            );
        }
    }
    #[test]
    fn wd3_part_policy_at_boundary_shared_with_selected_wire() {
        let mut d = Doc::new();
        let a = part(&mut d, &[p(0, 0)]);
        let selected = wire(&mut d, p(0, 0), p(0, -2));
        let replaced = wire(&mut d, p(0, 0), p(4, 0));
        // WD3 applies the wire policy only to an end belonging to a moved wire,
        // not a part. Here the part allows follow and wire-only policy denies it.
        let pol = FollowPolicy::new(true, 1, true).unwrap();
        assert!(pol.part_follows(1));
        assert!(!pol.wire_follows());
        let out = plan(
            &d,
            &[ItemId::Part(a), ItemId::Wire(selected)],
            Isometry::Translate(p(1, 0)),
            pol,
        );
        assert_eq!(out.remove, vec![replaced]);
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Rerouted)]);
    }

    #[test]
    fn tc_move_11_rotation_and_mirrors() {
        let (d, id) = fan(2);
        for by in Orient::ALL {
            let t = Isometry::Reorient { pivot: p(1, 2), by };
            let out = plan(&d, &[ItemId::Part(id)], t, policy(false));
            for a in [p(0, 0), p(0, 4)] {
                let a = t.apply(a).unwrap();
                assert!(out.add.iter().any(|w| w.a == a || w.b == a));
            }
        }
    }
    #[test]
    fn tc_move_12_final_moved_body_selects_dogleg() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        let (mut s, _) = d.remove_stored_part_raw(id).unwrap();
        s.local_bounds = Rect::from_points(p(-4, -2), p(0, 2)).unwrap();
        d.restore_stored_part_raw(id, s).unwrap();
        wire(&mut d, p(0, 0), p(-7, 0));
        wire(&mut d, p(-7, 0), p(-7, 4));
        part(&mut d, &[p(-3, 4)]);
        let out = plan(
            &d,
            &[ItemId::Part(id)],
            Isometry::Translate(p(1, 0)),
            policy(false),
        );
        assert_eq!(out.add.len(), 3);
        assert_eq!(out.add.first().unwrap().a, p(1, 0));
        assert_eq!(out.add.last().unwrap().b, p(-7, 4));
    }
    #[test]
    fn tc_move_13_refusal_clears_plan_and_preserves_doc() {
        let (mut d, id) = fan(1);
        // Block every outgoing direction adjacent to the final pin.
        for at in [p(2, 0), p(0, 0), p(1, 1), p(1, -1)] {
            part(&mut d, &[at]);
        }
        let before = format!("{d:?}");
        let mut out = FollowPlan {
            add: vec![Wire {
                a: p(0, 0),
                b: p(1, 0),
            }],
            ..Default::default()
        };
        assert!(matches!(
            plan_follow(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(false),
                &mut out
            ),
            Err(FollowError::NoRoute {
                blocker: Blocker::Pin { .. },
                ..
            })
        ));
        assert_eq!(out, FollowPlan::default());
        assert_eq!(before, format!("{d:?}"));
    }
    #[test]
    fn tc_move_15_fixed_junction() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        let w = wire(&mut d, p(0, 0), p(4, 0));
        wire(&mut d, p(4, 0), p(4, 3));
        wire(&mut d, p(4, 0), p(6, 0));
        let out = plan(
            &d,
            &[ItemId::Part(id)],
            Isometry::Translate(p(0, 1)),
            policy(false),
        );
        assert_eq!(out.remove, vec![w]);
        assert_eq!(out.add.last().unwrap().b, p(4, 0));
    }
    #[test]
    fn tc_move_16_direct_pins_empty_and_17_selected_segment() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        part(&mut d, &[p(0, 0)]);
        assert_eq!(
            plan(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(false)
            ),
            FollowPlan::default()
        );
        let first = wire(&mut d, p(0, 0), p(3, 0));
        let rest = wire(&mut d, p(3, 0), p(3, 4));
        let out = plan(
            &d,
            &[ItemId::Part(id), ItemId::Wire(first)],
            Isometry::Translate(p(1, 0)),
            policy(false),
        );
        assert_eq!(out.remove, vec![rest]);
        assert_eq!(out.add.first().unwrap().a, p(4, 0));
        assert_eq!(out.add.last().unwrap().b, p(3, 4));
    }
    #[test]
    fn wd4_reserves_later_free_end_and_validates_detached_geometry() {
        for detach_second in [false, true] {
            let mut d = Doc::new();
            let a = part(&mut d, &[p(0, 0)]);
            let b = part(&mut d, &[p(0, 4), p(20, 20)]);
            wire(&mut d, p(0, 0), p(-4, 0));
            wire(&mut d, p(0, 4), p(-4, 4));
            wire(&mut d, p(20, 20), p(22, 20));
            let policy =
                FollowPolicy::new(true, if detach_second { 2 } else { 10 }, false).unwrap();
            let out = plan(
                &d,
                &[ItemId::Part(a), ItemId::Part(b)],
                Isometry::Translate(p(0, 4)),
                policy,
            );
            // Horizontal-first would put a corner at the later run's fixed free end.
            // With detach policy it would also overlap that run's unchanged wire.
            assert_eq!(
                out.add[0],
                Wire {
                    a: p(0, 4),
                    b: p(0, 0)
                }
            );
            assert_eq!(
                out.add[1],
                Wire {
                    a: p(0, 0),
                    b: p(-4, 0)
                }
            );
            assert_eq!(
                out.fates[1].1,
                if detach_second {
                    RunFate::Detached
                } else {
                    RunFate::Rerouted
                }
            );
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn winv6_winv8_rigid_reorientation_and_labels(
            orientation in 0usize..8, x in -8i32..9, y in -8i32..9,
            invert in any::<bool>()
        ) {
            let (mut d, mut items) = bent();
            let label = d.insert_label(Label { at: p(0, 2), text: "net".into() }).unwrap();
            let t = Isometry::Reorient { pivot: p(x, y), by: Orient::ALL[orientation] };
            let expected = plan(&d, &items, t, policy(invert));
            prop_assert_eq!(&expected.fates, &vec![(p(0, 0), RunFate::Translated)]);
            prop_assert_eq!(&expected.labels, &vec![(label, t.apply(p(0, 2)).unwrap())]);
            prop_assert_eq!(expected.remove.len(), 3);
            prop_assert_eq!(&expected.add, &d.wires().map(|(_, w)| t.wire(*w).unwrap()).collect::<Vec<_>>());
            items.reverse();
            let wires: Vec<_> = d.wires().map(|(id, w)| (id, *w)).collect();
            for &(id, _) in &wires { d.remove_wire_raw(id).unwrap(); }
            for &(id, w) in wires.iter().rev() { d.restore_wire_raw(id, w).unwrap(); }
            prop_assert_eq!(&expected, &plan(&d, &items, t, policy(!invert)));
        }
        #[test]
        fn winv6_winv8_different_ids_and_multiple_selected_parts(n in 2i32..9, dx in -2i32..3, dy in -2i32..3, invert in any::<bool>()) {
            let (d,id)=fan(n);
            let items=vec![ItemId::Part(id),ItemId::Part(PartId(1))];
            let t=Isometry::Translate(p(dx,dy));
            let expected=plan(&d,&items,t,policy(invert));
            let mut rebuilt=Doc::new();let mut selected=Vec::new();
            for (i,s) in d.parts.slots.iter().enumerate().rev() {
                if let Some(s)=s {
                    let local:Vec<_>=s.pins.iter().map(|&p|s.part.orient.inverse().apply(p-s.part.at)).collect();
                    let (new_id,_)=rebuilt.insert_part_raw(s.part.clone(),&local,s.local_bounds).unwrap();
                    if items.contains(&ItemId::Part(PartId(i as u32))) { selected.push(ItemId::Part(new_id)); }
                }
            }
            for (_,w) in d.wires().collect::<Vec<_>>().into_iter().rev() { rebuilt.insert_wire_raw(*w).unwrap(); }
            let actual=plan(&rebuilt,&selected,t,policy(invert));
            prop_assert_eq!(&expected.add,&actual.add);prop_assert_eq!(&expected.fates,&actual.fates);
            let removed:Vec<_>=expected.remove.iter().map(|id|*d.wire(*id).unwrap()).collect();
            let removed_rebuilt:Vec<_>=actual.remove.iter().map(|id|*rebuilt.wire(*id).unwrap()).collect();
            prop_assert_eq!(removed,removed_rebuilt);prop_assert_eq!(actual.fates.len(),n as usize);
            selected.reverse();prop_assert_eq!(&actual,&plan(&rebuilt,&selected,t,policy(invert)));
        }
        #[test]        fn winv6_winv8_selection_and_rebuilt_incidence(n in 1i32..10,dx in -2i32..3,dy in -2i32..3,invert in any::<bool>()) {
            let (mut d,id)=fan(n);let t=Isometry::Translate(p(dx,dy));let mut items=vec![ItemId::Part(id),ItemId::Part(id)];
            let expected=plan(&d,&items,t,policy(invert));items.reverse();
            prop_assert_eq!(&expected,&plan(&d,&items,t,policy(invert)));
            // Restore identical ids in reverse order, rebuilding index and incidence traversal.
            let wires:Vec<_>=d.wires().map(|(id,w)|(id,*w)).collect();
            for &(id,_) in &wires {d.remove_wire_raw(id).unwrap();}
            for &(id,w) in wires.iter().rev() {d.restore_wire_raw(id,w).unwrap();}
            prop_assert_eq!(&expected,&plan(&d,&items,t,policy(invert)));
            prop_assert_eq!(expected.fates.len(),n as usize);
            let mut removed=expected.remove.clone();removed.sort_unstable_by_key(|w|w.0);removed.dedup();prop_assert_eq!(removed.len(),expected.remove.len());
        }
    }
}
