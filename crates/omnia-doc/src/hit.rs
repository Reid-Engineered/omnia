//! Indexed nearest-item picking in world nanometres (K14: caller converts 4 px).
//!
//! Symbol-local bounds are supplied by the caller to avoid a dependency cycle with
//! `omnia-symbols`. Keep the index and bounds lookup synchronized with the document.

use crate::{Doc, GeomError, ItemId, Part, Pt, Rect, SHEET_LIMIT_NM, SpatialIndex, SymbolId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub item: ItemId,
    /// Closest point on the part rectangle, wire segment, or label anchor.
    pub at: Pt,
}

/// Builds a read-only interaction index. Part entries include pins as well as body
/// bounds so snapping finds pins outside the art. Hit testing still uses only the body.
/// Transformed art bounds are clamped to K1; valid parts may straddle sheet edges.
/// Rebuild after edits until Doc owns and incrementally maintains this index (D3).
pub fn build_hit_index(
    doc: &Doc,
    bounds: impl Fn(SymbolId) -> Rect,
) -> Result<SpatialIndex, GeomError> {
    let mut index = SpatialIndex::new();
    for (id, part) in doc.parts() {
        let mut area = part_bounds(part, bounds(part.symbol))?;
        for &pin in doc.pins(id).unwrap() {
            area = area.union(Rect::new(pin, pin)?);
        }
        index.insert(ItemId::Part(id), area)?;
    }
    for (id, wire) in doc.wires() {
        index.insert(ItemId::Wire(id), Rect::from_points(wire.a, wire.b)?)?;
    }
    for (id, label) in doc.labels() {
        index.insert(ItemId::Label(id), Rect::new(label.at, label.at)?)?;
    }
    Ok(index)
}

pub(crate) fn part_bounds(part: &Part, local: Rect) -> Result<Rect, GeomError> {
    let oriented = local.checked_reorient_around(part.orient, Pt::ZERO)?;
    // Doc validates anchors and pins, not art extents. Translate in wide arithmetic
    // before clamping so edge-straddling art cannot invalidate the entire index.
    let limit = i64::from(SHEET_LIMIT_NM);
    let translate = |p: Pt| Pt {
        x: (i64::from(p.x) + i64::from(part.at.x)).clamp(-limit, limit) as i32,
        y: (i64::from(p.y) + i64::from(part.at.y)).clamp(-limit, limit) as i32,
    };
    Rect::new(translate(oriented.min), translate(oriented.max))
}

/// Nearest item in the inclusive Euclidean tolerance. Ties use part, wire, label,
/// then ascending ID. Labels use their anchor until text layout supplies geometry.
/// `scratch` is replaced and reusable; no allocation when its capacity suffices.
pub fn hit_test(
    doc: &Doc,
    index: &SpatialIndex,
    at: Pt,
    tolerance: i32,
    bounds: impl Fn(SymbolId) -> Rect,
    scratch: &mut Vec<ItemId>,
) -> Option<Hit> {
    assert!(at.is_in_bounds() && tolerance >= 0);
    index.query(search_area(at, tolerance), scratch);
    scratch
        .iter()
        .filter_map(|&item| {
            let nearest = match item {
                ItemId::Part(id) => {
                    let part = doc.part(id)?;
                    closest(at, part_bounds(part, bounds(part.symbol)).ok()?)
                }
                ItemId::Wire(id) => {
                    let wire = doc.wire(id)?;
                    closest(at, Rect::from_points(wire.a, wire.b).ok()?)
                }
                ItemId::Label(id) => doc.label(id)?.at,
            };
            let distance = at.distance_sq(nearest);
            (distance <= i64::from(tolerance).pow(2)).then_some((
                distance,
                item_key(item),
                Hit { item, at: nearest },
            ))
        })
        .min_by_key(|&(distance, key, _)| (distance, key))
        .map(|(_, _, hit)| hit)
}

pub(crate) fn item_key(item: ItemId) -> (u8, u32) {
    match item {
        ItemId::Part(id) => (0, id.0),
        ItemId::Wire(id) => (1, id.0),
        ItemId::Label(id) => (2, id.0),
    }
}

pub(crate) fn closest(at: Pt, area: Rect) -> Pt {
    Pt {
        x: at.x.clamp(area.min.x, area.max.x),
        y: at.y.clamp(area.min.y, area.max.y),
    }
}

pub(crate) fn search_area(at: Pt, radius: i32) -> Rect {
    let limit = i64::from(SHEET_LIMIT_NM);
    let coord = |v: i32, delta: i64| (i64::from(v) + delta).clamp(-limit, limit) as i32;
    let r = i64::from(radius);
    Rect {
        min: Pt {
            x: coord(at.x, -r),
            y: coord(at.y, -r),
        },
        max: Pt {
            x: coord(at.x, r),
            y: coord(at.y, r),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Label, Orient, PartId, Wire};
    use proptest::prelude::*;

    fn bounds(_: SymbolId) -> Rect {
        Rect::from_points(Pt { x: -101, y: -203 }, Pt { x: 307, y: 409 }).unwrap()
    }

    proptest! {
        #[test]
        fn indexed_matches_independent_brute_force_and_snap_inv9(
            mut items in prop::collection::vec((-999_999_000i32..=999_999_000, -999_999_000i32..=999_999_000, 0u8..8), 1..80),
            overlaps in prop::collection::vec((-100i32..=100, -100i32..=100, 0u8..8), 1..30),
            x in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            y in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            tolerance in 0i32..=SHEET_LIMIT_NM,
            grid in 1i32..=i32::MAX,
            pick in any::<usize>(),
            dx in -500i32..=500,
            dy in -500i32..=500,
            boundary_step in 1i32..=100,
        ) {
            // Retain full-sheet coverage and also force cross-item overlaps.
            items.extend(overlaps);
            let mut doc = Doc::new();
            let mut oracle = Vec::new();
            for &(x, y, o) in &items {
                let orient = Orient::ALL[o as usize];
                let at = Pt { x, y };
                let part = Part { symbol: SymbolId(0), at, orient, designator: "R".into(), value: "".into() };
                let id = doc.insert_part(part, &[Pt { x: 700, y: 0 }, Pt::ZERO]).unwrap();
                // Independent transform of all four corners (not part_bounds).
                let corners: Vec<_> = [-101, 307].into_iter().flat_map(|cx| [-203, 409].into_iter().map(move |cy| {
                    let cx = if o >= 4 { -cx } else { cx };
                    let (tx, ty) = match o % 4 { 0 => (cx, cy), 1 => (-cy, cx), 2 => (-cx, -cy), _ => (cy, -cx) };
                    Pt { x: x + tx, y: y + ty }
                })).collect();
                oracle.push((ItemId::Part(id), Rect { min: Pt { x: corners.iter().map(|p| p.x).min().unwrap(), y: corners.iter().map(|p| p.y).min().unwrap() }, max: Pt { x: corners.iter().map(|p| p.x).max().unwrap(), y: corners.iter().map(|p| p.y).max().unwrap() } }));
                let b = if o % 2 == 0 { Pt { x: -x, y } } else { Pt { x, y: -y } };
                if at != b {
                    let id = doc.insert_wire(Wire { a: at, b }).unwrap();
                    oracle.push((ItemId::Wire(id), Rect::from_points(at, b).unwrap()));
                }
                let id = doc.insert_label(Label { at, text: "net".into() }).unwrap();
                oracle.push((ItemId::Label(id), Rect::new(at, at).unwrap()));
            }
            let index = build_hit_index(&doc, bounds).unwrap();
            let mut scratch = Vec::new();
            let chosen = items[pick % items.len()];
            // Probe just beyond the rightmost item. Nothing else can be closer,
            // so the nearest distance is exactly boundary_step (not merely the
            // distance to an item hidden behind a nearer overlapping body).
            let rightmost = oracle.iter().max_by_key(|(_, area)| area.max.x).unwrap().1;
            let boundary = Pt { x: rightmost.max.x + boundary_step, y: ((i64::from(rightmost.min.y) + i64::from(rightmost.max.y)) / 2) as i32 };
            for (at, tolerance) in [
                (Pt { x, y }, tolerance),
                (Pt { x: chosen.0 + dx, y: chosen.1 + dy }, tolerance),
                (boundary, boundary_step),
                (boundary, boundary_step - 1),
            ] {
                let expected = oracle.iter().filter_map(|&(item, area)| {
                    let q = Pt { x: at.x.max(area.min.x).min(area.max.x), y: at.y.max(area.min.y).min(area.max.y) };
                    let dx = i128::from(at.x) - i128::from(q.x);
                    let dy = i128::from(at.y) - i128::from(q.y);
                    let d = dx * dx + dy * dy;
                    (d <= i128::from(tolerance).pow(2)).then_some((d, item_key(item), Hit { item, at: q }))
                }).min_by_key(|&(d, key, _)| (d, key)).map(|(_, _, h)| h);
                prop_assert_eq!(hit_test(&doc, &index, at, tolerance, bounds, &mut scratch), expected);
                let first = crate::snap(&doc, &index, at, grid, tolerance, &mut scratch);
                prop_assert!(first.at().is_in_bounds());
                prop_assert_eq!(crate::snap(&doc, &index, first.at(), grid, tolerance, &mut scratch), first);
            }
            // Pin outside the symbol rectangle must still be indexed for snapping.
            let pin = doc.pins(PartId(0)).unwrap()[0];
            prop_assert!(matches!(crate::snap(&doc, &index, pin, grid, 0, &mut scratch), crate::Snap::Pin { .. }), "exact pin must snap to pin");
        }
    }

    #[test]
    fn art_crossing_sheet_edges_remains_indexable_pickable_and_snappable() {
        let mut scratch = Vec::new();
        for x in [-SHEET_LIMIT_NM, 0, SHEET_LIMIT_NM] {
            for y in [-SHEET_LIMIT_NM, 0, SHEET_LIMIT_NM] {
                if x == 0 && y == 0 {
                    continue;
                }
                for orient in Orient::ALL {
                    let at = Pt { x, y };
                    let doc = Doc::from_items(
                        [(
                            Part {
                                symbol: SymbolId(0),
                                at,
                                orient,
                                designator: "edge".into(),
                                value: "".into(),
                            },
                            vec![Pt::ZERO],
                        )],
                        [],
                        [Label {
                            at: Pt::ZERO,
                            text: "unrelated".into(),
                        }],
                    )
                    .unwrap();
                    // Independent corner-by-corner translation/clamping reference.
                    let corners: Vec<_> = [-101, 307]
                        .into_iter()
                        .flat_map(|cx| {
                            [-203, 409].into_iter().map(move |cy| {
                                let offset = orient.apply(Pt { x: cx, y: cy });
                                Pt {
                                    x: (x + offset.x).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                                    y: (y + offset.y).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                                }
                            })
                        })
                        .collect();
                    let expected = Rect {
                        min: Pt {
                            x: corners.iter().map(|p| p.x).min().unwrap(),
                            y: corners.iter().map(|p| p.y).min().unwrap(),
                        },
                        max: Pt {
                            x: corners.iter().map(|p| p.x).max().unwrap(),
                            y: corners.iter().map(|p| p.y).max().unwrap(),
                        },
                    };
                    let index = build_hit_index(&doc, bounds).unwrap();
                    assert_eq!(index.len(), 2);
                    assert_eq!(index.bounds(ItemId::Part(PartId(0))), Some(expected));
                    for point in [at, expected.center()] {
                        assert_eq!(
                            hit_test(&doc, &index, point, 0, bounds, &mut scratch),
                            Some(Hit {
                                item: ItemId::Part(PartId(0)),
                                at: point
                            })
                        );
                    }
                    let near_pin = Pt {
                        x: x - x.signum() * 3,
                        y: y - y.signum() * 4,
                    };
                    let snapped = crate::snap(&doc, &index, near_pin, 10, 5, &mut scratch);
                    assert_eq!(
                        snapped,
                        crate::Snap::Pin {
                            part: PartId(0),
                            pin: 0,
                            at
                        }
                    );
                    assert_eq!(
                        crate::snap(&doc, &index, snapped.at(), 10, 5, &mut scratch),
                        snapped
                    );
                    assert_eq!(
                        hit_test(&doc, &index, Pt::ZERO, 0, bounds, &mut scratch)
                            .unwrap()
                            .item,
                        ItemId::Label(crate::LabelId(0))
                    );
                }
            }
        }
    }

    #[test]
    fn inclusive_circle_body_distance_and_deterministic_ties() {
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "".into(),
                },
                vec![Pt { x: 100, y: 100 }],
            )],
            [Wire {
                a: Pt::ZERO,
                b: Pt { x: 10, y: 0 },
            }],
            [
                Label {
                    at: Pt::ZERO,
                    text: "A".into(),
                },
                Label {
                    at: Pt::ZERO,
                    text: "B".into(),
                },
            ],
        )
        .unwrap();
        let bounds = |_| Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap();
        let index = build_hit_index(&doc, bounds).unwrap();
        let mut scratch = Vec::new();
        assert_eq!(
            hit_test(&doc, &index, Pt::ZERO, 0, bounds, &mut scratch)
                .unwrap()
                .item,
            ItemId::Part(PartId(0))
        );
        assert_eq!(
            hit_test(&doc, &index, Pt { x: 5, y: 5 }, 0, bounds, &mut scratch)
                .unwrap()
                .at,
            Pt { x: 5, y: 5 }
        );
        let corner = Pt { x: 13, y: 14 };
        assert_eq!(
            hit_test(&doc, &index, corner, 5, bounds, &mut scratch),
            Some(Hit {
                item: ItemId::Part(PartId(0)),
                at: Pt { x: 10, y: 10 }
            })
        );
        assert_eq!(
            hit_test(&doc, &index, corner, 4, bounds, &mut scratch),
            None
        );
        // Indexed pin envelopes must not enlarge the part's pickable body.
        assert_eq!(
            hit_test(&doc, &index, Pt { x: 100, y: 100 }, 0, bounds, &mut scratch),
            None
        );
    }

    #[test]
    fn removed_slots_and_sheet_edges() {
        let mut doc = Doc::new();
        let gone = doc
            .insert_label(Label {
                at: Pt::ZERO,
                text: "gone".into(),
            })
            .unwrap();
        doc.remove_label(gone);
        let at = Pt {
            x: SHEET_LIMIT_NM,
            y: -SHEET_LIMIT_NM,
        };
        let live = doc
            .insert_label(Label {
                at,
                text: "live".into(),
            })
            .unwrap();
        let index = build_hit_index(&doc, bounds).unwrap();
        assert_eq!(doc.labels().map(|(id, _)| id).collect::<Vec<_>>(), [live]);
        assert_eq!(doc.pins(PartId(0)), None);
        let mut scratch = Vec::new();
        assert_eq!(
            hit_test(&doc, &index, at, i32::MAX, bounds, &mut scratch),
            Some(Hit {
                item: ItemId::Label(live),
                at
            })
        );
        assert_eq!(
            hit_test(&doc, &index, Pt::ZERO, 0, bounds, &mut scratch),
            None
        );
    }
}
