//! Priority snapping in world nanometres (WD5, WK4, K14: caller converts 10 px).

use smallvec::SmallVec;

use crate::hit::search_area;
use crate::{Doc, ItemId, PartId, Pt, SHEET_LIMIT_NM, Wire, WireId, snap_to};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Pin { part: PartId, pin: usize, at: Pt },
    WireEnd { wire: WireId, at: Pt },
    Junction { at: Pt },
    WireBody { wire: WireId, at: Pt },
    Grid(Pt),
}

impl Snap {
    pub fn at(self) -> Pt {
        match self {
            Self::Pin { at, .. }
            | Self::WireEnd { at, .. }
            | Self::Junction { at }
            | Self::WireBody { at, .. }
            | Self::Grid(at) => at,
        }
    }
}

/// Priority snapping per WD5: pin, then wire end or junction, then wire body
/// (projected onto the segment and moved along it to the nearest grid point still
/// on the segment), then grid fallback.
///
/// Acceptance uses inclusive Euclidean distance (`<= radius`). Ties inside a class
/// break by smaller `i64` squared distance, then lowest id (`PartId` then pin index;
/// `WireId`, using the lowest `WireId` at a junction), then coordinates.
///
/// Settles through higher priorities to enforce WINV7/INV9 even for off-grid
/// geometry: rounding or projection can enter another target's radius. Consequently
/// the final point can be farther than `radius` from the input. Grid points are
/// clipped to the outermost in-sheet grid line (K1).
pub fn snap(doc: &Doc, at: Pt, grid: i32, radius: i32, scratch: &mut Vec<ItemId>) -> Snap {
    assert!(at.is_in_bounds() && grid > 0 && radius >= 0);
    let mut current = at;
    // Each move after the first strictly improves the priority tier. A zero-distance
    // target wins within its tier, so four tiers need at most five evaluations.
    for _ in 0..5 {
        let result = once(doc, current, grid, radius, scratch);
        if result.at() == current {
            return result;
        }
        current = result.at();
    }
    unreachable!("snap priority must converge")
}

fn once(doc: &Doc, at: Pt, grid: i32, radius: i32, scratch: &mut Vec<ItemId>) -> Snap {
    doc.query(search_area(at, radius), scratch);
    let r_sq = i64::from(radius) * i64::from(radius);

    // Tier 0: Pin (strictly beats wire end / junction, wire body, and grid).
    let mut best_pin = None;
    for &item in scratch.iter() {
        if let ItemId::Part(part) = item
            && let Some(pins) = doc.pins(part)
        {
            for (pin, &p) in pins.iter().enumerate() {
                let distance = at.distance_sq(p);
                if distance <= r_sq {
                    let key = (distance, part.0, pin, p.x, p.y);
                    if best_pin.as_ref().is_none_or(|(old, _)| key < *old) {
                        best_pin = Some((key, Snap::Pin { part, pin, at: p }));
                    }
                }
            }
        }
    }
    if let Some((_, result)) = best_pin {
        return result;
    }

    // Tier 1: WireEnd or Junction (same priority class; no pin is within radius here,
    // so >= 3 wire ends at a point within radius is a Junction, 1..=2 is a WireEnd).
    // Up to 32 distinct wire endpoints within radius stay stack-allocated.
    let mut end_counts: SmallVec<[(Pt, u32, u8, i64); 32]> = SmallVec::new();
    for &item in scratch.iter() {
        if let ItemId::Wire(wire) = item
            && let Some(w) = doc.wire(wire)
        {
            for p in [w.a, w.b] {
                let distance = at.distance_sq(p);
                if distance <= r_sq {
                    if let Some(entry) = end_counts.iter_mut().find(|(pt, ..)| *pt == p) {
                        entry.1 = entry.1.min(wire.0);
                        entry.2 = entry.2.saturating_add(1);
                    } else {
                        end_counts.push((p, wire.0, 1, distance));
                    }
                }
            }
        }
    }
    if let Some(&(p, min_wire_id, count, _)) = end_counts
        .iter()
        .min_by_key(|&&(p, min_wire_id, _, distance)| (distance, min_wire_id, p.x, p.y))
    {
        return if count >= 3 {
            Snap::Junction { at: p }
        } else {
            Snap::WireEnd {
                wire: WireId(min_wire_id),
                at: p,
            }
        };
    }

    // Tier 2: WireBody (projected onto segment and moved along it to nearest grid point on segment).
    let mut best_body = None;
    for &item in scratch.iter() {
        if let ItemId::Wire(wire) = item
            && let Some(w) = doc.wire(wire)
            && let Some(p) = wire_body_target(at, w, grid)
        {
            let distance = at.distance_sq(p);
            if distance <= r_sq {
                let key = (distance, wire.0, p.x, p.y);
                if best_body.as_ref().is_none_or(|(old, _)| key < *old) {
                    best_body = Some((key, Snap::WireBody { wire, at: p }));
                }
            }
        }
    }
    if let Some((_, result)) = best_body {
        return result;
    }

    // Tier 3: Grid fallback.
    grid_snap(at, grid)
}

fn grid_snap(at: Pt, grid: i32) -> Snap {
    let limit = SHEET_LIMIT_NM / grid * grid;
    let p = snap_to(at, grid);
    Snap::Grid(Pt {
        x: p.x.clamp(-limit, limit),
        y: p.y.clamp(-limit, limit),
    })
}

/// Projects `at` onto `w`, clamps to `w`, then moves along `w` to the nearest
/// multiple of `grid` that still lies on `w`. Returns `None` if no grid step lies on `w`.
fn wire_body_target(at: Pt, w: &Wire, grid: i32) -> Option<Pt> {
    if w.a.y == w.b.y {
        let (lo, hi) = (w.a.x.min(w.b.x), w.a.x.max(w.b.x));
        let proj = at.x.clamp(lo, hi);
        let x = nearest_grid_in_range(proj, lo, hi, grid)?;
        Some(Pt { x, y: w.a.y })
    } else {
        let (lo, hi) = (w.a.y.min(w.b.y), w.a.y.max(w.b.y));
        let proj = at.y.clamp(lo, hi);
        let y = nearest_grid_in_range(proj, lo, hi, grid)?;
        Some(Pt { x: w.a.x, y })
    }
}

#[inline]
fn div_ceil_i64(a: i64, b: i64) -> i64 {
    debug_assert!(b > 0);
    -((-a).div_euclid(b))
}

fn nearest_grid_in_range(v: i32, lo: i32, hi: i32, grid: i32) -> Option<i32> {
    let step = i64::from(grid);
    let k_min = div_ceil_i64(i64::from(lo), step);
    let k_max = i64::from(hi).div_euclid(step);
    if k_min > k_max {
        return None;
    }
    let v64 = i64::from(v);
    let half = step / 2;
    let k = if v64 >= 0 {
        (v64 + half) / step
    } else {
        (v64 - half) / step
    };
    let k = k.clamp(k_min, k_max);
    Some((k * step) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GRID_NM, Label, Orient, PIN_LATTICE_NM, Part, Rect, SymbolId};
    use proptest::prelude::*;
    use std::collections::HashMap;

    const PX_NM_100: i32 = GRID_NM / 20; // K3: 20 px = GRID_NM => 1 px = 127_000 nm
    const RADIUS_100: i32 = 10 * PX_NM_100; // WK4/K14: 10 px = 1_270_000 nm

    fn part_at(at: Pt) -> Part {
        Part {
            symbol: SymbolId(0),
            at,
            orient: Orient::IDENTITY,
            designator: "R1".into(),
            value: "".into(),
        }
    }

    fn zero_bounds() -> Rect {
        Rect::new(Pt::ZERO, Pt::ZERO).unwrap()
    }

    /// Independent brute-force WD5 oracle over all items in `doc` (no spatial index).
    fn brute_once(doc: &Doc, at: Pt, grid: i32, radius: i32) -> Snap {
        let r_sq = i64::from(radius) * i64::from(radius);
        let mut best_pin = None;
        for (part, _) in doc.parts() {
            if let Some(pins) = doc.pins(part) {
                for (pin, &p) in pins.iter().enumerate() {
                    let distance = at.distance_sq(p);
                    if distance <= r_sq {
                        let key = (distance, part.0, pin, p.x, p.y);
                        if best_pin.as_ref().is_none_or(|(old, _)| key < *old) {
                            best_pin = Some((key, Snap::Pin { part, pin, at: p }));
                        }
                    }
                }
            }
        }
        if let Some((_, res)) = best_pin {
            return res;
        }

        let mut ends: HashMap<Pt, (u32, usize, i64)> = HashMap::new();
        for (wire, w) in doc.wires() {
            for p in [w.a, w.b] {
                let distance = at.distance_sq(p);
                if distance <= r_sq {
                    ends.entry(p)
                        .and_modify(|(min_id, count, _)| {
                            *min_id = (*min_id).min(wire.0);
                            *count += 1;
                        })
                        .or_insert((wire.0, 1, distance));
                }
            }
        }
        if let Some((&p, &(min_id, count, _))) = ends
            .iter()
            .min_by_key(|&(&p, &(min_id, _, dist))| (dist, min_id, p.x, p.y))
        {
            return if count >= 3 {
                Snap::Junction { at: p }
            } else {
                Snap::WireEnd {
                    wire: WireId(min_id),
                    at: p,
                }
            };
        }

        let mut best_body = None;
        for (wire, w) in doc.wires() {
            let (lo, hi, is_h) = if w.a.y == w.b.y {
                (w.a.x.min(w.b.x), w.a.x.max(w.b.x), true)
            } else {
                (w.a.y.min(w.b.y), w.a.y.max(w.b.y), false)
            };
            let proj = if is_h {
                at.x.clamp(lo, hi)
            } else {
                at.y.clamp(lo, hi)
            };
            // Brute-force search over valid grid multiples on [lo, hi] around proj.
            let step = i64::from(grid);
            let k_min = div_ceil_i64(i64::from(lo), step);
            let k_max = i64::from(hi).div_euclid(step);
            if k_min <= k_max {
                let target_coord = {
                    let approx = (i64::from(proj) / step).clamp(k_min, k_max);
                    let candidates = [
                        (approx - 1).clamp(k_min, k_max),
                        approx,
                        (approx + 1).clamp(k_min, k_max),
                    ];
                    // Pick strictly closest to proj; on tie, pick larger |coord| (away from zero).
                    let best_k = candidates
                        .into_iter()
                        .min_by_key(|&k| {
                            let c = k * step;
                            let d = (c - i64::from(proj)).abs();
                            (d, -c.abs())
                        })
                        .unwrap();
                    (best_k * step) as i32
                };
                let p = if is_h {
                    Pt {
                        x: target_coord,
                        y: w.a.y,
                    }
                } else {
                    Pt {
                        x: w.a.x,
                        y: target_coord,
                    }
                };
                let distance = at.distance_sq(p);
                if distance <= r_sq {
                    let key = (distance, wire.0, p.x, p.y);
                    if best_body.as_ref().is_none_or(|(old, _)| key < *old) {
                        best_body = Some((key, Snap::WireBody { wire, at: p }));
                    }
                }
            }
        }
        if let Some((_, res)) = best_body {
            return res;
        }
        grid_snap(at, grid)
    }

    fn brute_snap(doc: &Doc, at: Pt, grid: i32, radius: i32) -> Snap {
        let mut current = at;
        for _ in 0..5 {
            let res = brute_once(doc, current, grid, radius);
            if res.at() == current {
                return res;
            }
            current = res.at();
        }
        unreachable!("brute_snap must converge")
    }

    proptest! {
        #[test]
        fn inv9_dense_off_grid_geometry(
            pins in prop::collection::vec((-100i32..=100, -100i32..=100), 0..30),
            wires in prop::collection::vec((-100i32..=100, -100i32..=100, 1i32..=100, any::<bool>()), 0..30),
            x in -200i32..=200, y in -200i32..=200,
            grid in 1i32..=1000, radius in 0i32..=200,
        ) {
            let doc = Doc::from_items([(
                part_at(Pt::ZERO),
                pins.into_iter().map(|(x, y)| Pt { x, y }).collect(),
                zero_bounds(),
            )], wires.into_iter().map(|(x, y, length, vertical)| Wire {
                a: Pt { x, y },
                b: if vertical { Pt { x, y: y + length } } else { Pt { x: x + length, y } },
            }), []).unwrap();
            let mut scratch = Vec::new();
            let first = snap(&doc, Pt { x, y }, grid, radius, &mut scratch);
            prop_assert_eq!(snap(&doc, first.at(), grid, radius, &mut scratch), first);
        }

        #[test]
        fn winv7_matches_brute_force_and_reverse_insertion(
            part_specs in prop::collection::vec((-40i32..=40, -40i32..=40, 0u8..8), 0..20),
            wire_specs in prop::collection::vec((-40i32..=40, -40i32..=40, 1i32..=20, any::<bool>()), 0..40),
            cx in -50i32..=50,
            cy in -50i32..=50,
            grid in 1i32..=20,
            radius in 0i32..=30,
        ) {
            let parts_data: Vec<_> = part_specs
                .iter()
                .map(|&(x, y, o)| {
                    (
                        Part {
                            symbol: SymbolId(0),
                            at: Pt { x, y },
                            orient: Orient::ALL[o as usize],
                            designator: "R".into(),
                            value: "".into(),
                        },
                        vec![Pt::ZERO, Pt { x: 4, y: 0 }],
                        Rect::from_points(Pt { x: -2, y: -2 }, Pt { x: 6, y: 2 }).unwrap(),
                    )
                })
                .collect();
            let wires_data: Vec<_> = wire_specs
                .iter()
                .map(|&(x, y, len, vert)| Wire {
                    a: Pt { x, y },
                    b: if vert { Pt { x, y: y + len } } else { Pt { x: x + len, y } },
                })
                .collect();

            let doc = Doc::from_items(parts_data.clone(), wires_data.clone(), []).unwrap();
            let at = Pt { x: cx, y: cy };
            let mut scratch = Vec::new();

            let actual = snap(&doc, at, grid, radius, &mut scratch);
            let expected = brute_snap(&doc, at, grid, radius);
            prop_assert_eq!(actual, expected);
            // INV9 idempotence.
            prop_assert_eq!(snap(&doc, actual.at(), grid, radius, &mut scratch), actual);
            // Fixed point obeys WD5 once-step class/distance/id ranking.
            prop_assert_eq!(brute_once(&doc, actual.at(), grid, radius), actual);

            // Rebuild the exact same sheet with items inserted into the index in reverse
            // order (same IDs, reversed spatial-index cell lists) and verify identical snap.
            let mut doc_rev_index = Doc::new();
            let mut pids = Vec::new();
            for (p, pins, bounds) in &parts_data {
                pids.push((doc_rev_index.insert_part(p.clone(), pins, *bounds).unwrap(), p.clone(), pins.clone(), *bounds));
            }
            let mut wids = Vec::new();
            for &w in &wires_data {
                wids.push((doc_rev_index.insert_wire(w).unwrap(), w));
            }
            for &(pid, _, _, _) in &pids {
                doc_rev_index.remove_part(pid).unwrap();
            }
            for &(wid, _) in &wids {
                doc_rev_index.remove_wire(wid).unwrap();
            }
            for (pid, p, pins, bounds) in pids.into_iter().rev() {
                doc_rev_index.restore_part(pid, p, &pins, bounds).unwrap();
            }
            for (wid, w) in wids.into_iter().rev() {
                doc_rev_index.restore_wire(wid, w).unwrap();
            }
            prop_assert_eq!(snap(&doc_rev_index, at, grid, radius, &mut scratch), actual);

            // Also rebuild via Doc::from_items in reverse order (IDs reversed too);
            // the brute-force WD5 oracle must still agree on the new document.
            let doc_rev_ids = Doc::from_items(
                parts_data.into_iter().rev(),
                wires_data.into_iter().rev(),
                Vec::<Label>::new(),
            )
            .unwrap();
            let rev_actual = snap(&doc_rev_ids, at, grid, radius, &mut scratch);
            prop_assert_eq!(rev_actual, brute_snap(&doc_rev_ids, at, grid, radius));
        }
    }

    #[test]
    fn tc_snap_01_pin_beats_closer_wire_body() {
        let doc = Doc::from_items(
            [(part_at(Pt::ZERO), vec![Pt::ZERO], zero_bounds())],
            [Wire {
                a: Pt {
                    x: -10 * GRID_NM,
                    y: 10 * PX_NM_100,
                },
                b: Pt {
                    x: 10 * GRID_NM,
                    y: 10 * PX_NM_100,
                },
            }],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        // Cursor 8 px from pin (0, 0) and 2 px from wire body at y = 10 px.
        let cursor = Pt {
            x: 0,
            y: 8 * PX_NM_100,
        };
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: Pt::ZERO,
            }
        );
    }

    #[test]
    fn tc_snap_02_two_pins_nearer_wins() {
        let p_far = Pt::ZERO;
        let p_near = Pt {
            x: 6 * PX_NM_100,
            y: 0,
        };
        let doc = Doc::from_items(
            [
                (part_at(p_far), vec![Pt::ZERO], zero_bounds()),
                (part_at(p_near), vec![Pt::ZERO], zero_bounds()),
            ],
            [],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        let cursor = Pt {
            x: 4 * PX_NM_100,
            y: 0,
        };
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: PartId(1),
                pin: 0,
                at: p_near,
            }
        );
    }

    #[test]
    fn tc_snap_03_equal_distance_pins_pick_lower_id_in_either_index_order() {
        let p0 = Pt {
            x: -5 * PX_NM_100,
            y: 0,
        };
        let p1 = Pt {
            x: 5 * PX_NM_100,
            y: 0,
        };
        let mut doc = Doc::new();
        let id0 = doc
            .insert_part(part_at(p0), &[Pt::ZERO], zero_bounds())
            .unwrap();
        let id1 = doc
            .insert_part(part_at(p1), &[Pt::ZERO], zero_bounds())
            .unwrap();
        let mut scratch = Vec::new();
        let cursor = Pt { x: 0, y: 1 };
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: id0,
                pin: 0,
                at: p0,
            }
        );

        // Reverse insertion order in the spatial index while keeping IDs.
        doc.remove_part(id0).unwrap();
        doc.remove_part(id1).unwrap();
        doc.restore_part(id1, part_at(p1), &[Pt::ZERO], zero_bounds())
            .unwrap();
        doc.restore_part(id0, part_at(p0), &[Pt::ZERO], zero_bounds())
            .unwrap();
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: id0,
                pin: 0,
                at: p0,
            }
        );

        // Also check reverse PartId assignment via from_items: new PartId(0) at p1 wins.
        let doc_rev = Doc::from_items(
            [
                (part_at(p1), vec![Pt::ZERO], zero_bounds()),
                (part_at(p0), vec![Pt::ZERO], zero_bounds()),
            ],
            [],
            [],
        )
        .unwrap();
        assert_eq!(
            snap(&doc_rev, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: p1,
            }
        );
    }

    #[test]
    fn tc_snap_04_target_exactly_at_radius_is_accepted() {
        let pin_pt = Pt { x: 1_500_000, y: 0 };
        let doc =
            Doc::from_items([(part_at(pin_pt), vec![Pt::ZERO], zero_bounds())], [], []).unwrap();
        let mut scratch = Vec::new();
        // 3-4-5 right triangle with hypotenuse = 10 px = RADIUS_100.
        let on_boundary = Pt {
            x: pin_pt.x - 6 * PX_NM_100,
            y: 8 * PX_NM_100,
        };
        assert_eq!(
            snap(&doc, on_boundary, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: pin_pt,
            }
        );
        // 1 nm outside the Euclidean circle; grid fallback (0, 0) is also > RADIUS_100 from pin_pt.
        let outside = Pt {
            x: pin_pt.x - 6 * PX_NM_100 - 1,
            y: 8 * PX_NM_100,
        };
        assert_eq!(
            snap(&doc, outside, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Grid(Pt::ZERO)
        );
    }

    #[test]
    fn tc_snap_05_wire_end_beats_closer_wire_body() {
        let doc = Doc::from_items(
            [],
            [
                Wire {
                    a: Pt {
                        x: -10 * GRID_NM,
                        y: 0,
                    },
                    b: Pt {
                        x: 10 * GRID_NM,
                        y: 0,
                    },
                },
                Wire {
                    a: Pt {
                        x: 0,
                        y: 8 * PX_NM_100,
                    },
                    b: Pt {
                        x: 0,
                        y: 10 * GRID_NM,
                    },
                },
            ],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        // Cursor 1 px from wire 0's body at (0, 0) and 7 px from wire 1's end at (0, 8 px).
        let cursor = Pt { x: 0, y: PX_NM_100 };
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::WireEnd {
                wire: WireId(1),
                at: Pt {
                    x: 0,
                    y: 8 * PX_NM_100,
                },
            }
        );
    }

    #[test]
    fn tc_snap_06_wire_body_snaps_to_nearer_grid_point_on_segment() {
        let doc = Doc::from_items(
            [],
            [
                Wire {
                    a: Pt {
                        x: -4 * GRID_NM,
                        y: 0,
                    },
                    b: Pt {
                        x: 4 * GRID_NM,
                        y: 0,
                    },
                },
                Wire {
                    a: Pt {
                        x: 10 * GRID_NM,
                        y: -4 * GRID_NM,
                    },
                    b: Pt {
                        x: 10 * GRID_NM,
                        y: 4 * GRID_NM,
                    },
                },
                Wire {
                    a: Pt {
                        x: -4 * GRID_NM,
                        y: PIN_LATTICE_NM,
                    },
                    b: Pt {
                        x: 4 * GRID_NM,
                        y: PIN_LATTICE_NM,
                    },
                },
            ],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        // Between x = GRID_NM and x = 2 * GRID_NM, nearer to GRID_NM.
        let cursor_h = Pt {
            x: GRID_NM + 900_000,
            y: 200_000,
        };
        assert_eq!(
            snap(&doc, cursor_h, GRID_NM, RADIUS_100, &mut scratch),
            Snap::WireBody {
                wire: WireId(0),
                at: Pt { x: GRID_NM, y: 0 },
            }
        );
        // Between y = GRID_NM and y = 2 * GRID_NM, nearer to 2 * GRID_NM.
        let cursor_v = Pt {
            x: 10 * GRID_NM - 200_000,
            y: GRID_NM + 1_600_000,
        };
        assert_eq!(
            snap(&doc, cursor_v, GRID_NM, RADIUS_100, &mut scratch),
            Snap::WireBody {
                wire: WireId(1),
                at: Pt {
                    x: 10 * GRID_NM,
                    y: 2 * GRID_NM,
                },
            }
        );

        // Distance is measured to the final grid-snapped point on the segment, not raw projection:
        // cursor lies directly on wire 2 (y = PIN_LATTICE_NM), 1_000_001 nm from the nearest grid
        // point (GRID_NM, PIN_LATTICE_NM). With radius = 1_000_000, raw projection is 0 away
        // yet the final grid-snapped point is > radius away, so it falls back to Grid.
        let too_far_after_grid_shift = Pt {
            x: GRID_NM + 1_000_001,
            y: PIN_LATTICE_NM,
        };
        assert_eq!(
            snap(
                &doc,
                too_far_after_grid_shift,
                GRID_NM,
                1_000_000,
                &mut scratch
            ),
            Snap::Grid(Pt {
                x: GRID_NM,
                y: GRID_NM,
            })
        );
    }

    #[test]
    fn tc_snap_07_pin_with_two_wires_stays_pin() {
        let doc = Doc::from_items(
            [(part_at(Pt::ZERO), vec![Pt::ZERO], zero_bounds())],
            [
                Wire {
                    a: Pt::ZERO,
                    b: Pt {
                        x: 4 * GRID_NM,
                        y: 0,
                    },
                },
                Wire {
                    a: Pt::ZERO,
                    b: Pt {
                        x: 0,
                        y: 4 * GRID_NM,
                    },
                },
            ],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        assert_eq!(
            snap(
                &doc,
                Pt {
                    x: 3 * PX_NM_100,
                    y: 4 * PX_NM_100,
                },
                GRID_NM,
                RADIUS_100,
                &mut scratch
            ),
            Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: Pt::ZERO,
            }
        );
    }

    #[test]
    fn tc_snap_08_nothing_in_radius_falls_back_to_grid() {
        let doc =
            Doc::from_items([(part_at(Pt::ZERO), vec![Pt::ZERO], zero_bounds())], [], []).unwrap();
        let mut scratch = Vec::new();
        let cursor = Pt {
            x: 3 * GRID_NM + 400_000,
            y: -2 * GRID_NM - 500_000,
        };
        assert_eq!(
            snap(&doc, cursor, GRID_NM, RADIUS_100, &mut scratch),
            Snap::Grid(Pt {
                x: 3 * GRID_NM,
                y: -2 * GRID_NM,
            })
        );
    }

    #[test]
    fn junction_class_and_priority_rules() {
        let jpt = Pt {
            x: 5 * GRID_NM,
            y: 5 * GRID_NM,
        };
        let corner_pt = Pt {
            x: 10 * GRID_NM,
            y: 5 * GRID_NM,
        };
        let doc = Doc::from_items(
            [],
            [
                // 3 wires meeting at jpt => Junction.
                Wire {
                    a: jpt,
                    b: Pt {
                        x: 5 * GRID_NM,
                        y: 0,
                    },
                },
                Wire {
                    a: jpt,
                    b: Pt {
                        x: 5 * GRID_NM,
                        y: 8 * GRID_NM,
                    },
                },
                Wire {
                    a: jpt,
                    b: corner_pt,
                },
                // 2 wires meeting at corner_pt => WireEnd (not Junction).
                Wire {
                    a: corner_pt,
                    b: Pt {
                        x: 10 * GRID_NM,
                        y: 10 * GRID_NM,
                    },
                },
            ],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        assert_eq!(
            snap(
                &doc,
                jpt + Pt {
                    x: 2 * PX_NM_100,
                    y: -PX_NM_100
                },
                GRID_NM,
                RADIUS_100,
                &mut scratch
            ),
            Snap::Junction { at: jpt }
        );
        assert_eq!(
            snap(
                &doc,
                corner_pt
                    + Pt {
                        x: PX_NM_100,
                        y: PX_NM_100
                    },
                GRID_NM,
                RADIUS_100,
                &mut scratch
            ),
            Snap::WireEnd {
                wire: WireId(2),
                at: corner_pt,
            }
        );

        // WireBody on a half-grid y wire (e.g., op-amp input pin) still snaps x to grid on segment,
        // and a short segment with no grid point on it yields no WireBody snap.
        let doc_half = Doc::from_items(
            [],
            [
                Wire {
                    a: Pt {
                        x: PIN_LATTICE_NM,
                        y: PIN_LATTICE_NM,
                    },
                    b: Pt {
                        x: 3 * GRID_NM + PIN_LATTICE_NM,
                        y: PIN_LATTICE_NM,
                    },
                },
                Wire {
                    a: Pt {
                        x: 100,
                        y: 10 * GRID_NM,
                    },
                    b: Pt {
                        x: 500,
                        y: 10 * GRID_NM,
                    },
                },
            ],
            [],
        )
        .unwrap();
        assert_eq!(
            snap(
                &doc_half,
                Pt {
                    x: 2 * GRID_NM + 400_000,
                    y: PIN_LATTICE_NM + 100_000,
                },
                GRID_NM,
                RADIUS_100,
                &mut scratch
            ),
            Snap::WireBody {
                wire: WireId(0),
                at: Pt {
                    x: 2 * GRID_NM,
                    y: PIN_LATTICE_NM,
                },
            }
        );
        assert_eq!(
            snap(
                &doc_half,
                Pt {
                    x: 300,
                    y: 10 * GRID_NM,
                },
                GRID_NM,
                50,
                &mut scratch
            ),
            Snap::Grid(Pt {
                x: 0,
                y: 10 * GRID_NM,
            })
        );
    }

    #[test]
    fn priorities_and_settling_off_grid_targets() {
        let doc = Doc::from_items(
            [(part_at(Pt { x: 20, y: 0 }), vec![Pt::ZERO], zero_bounds())],
            [Wire {
                a: Pt { x: 10, y: 0 },
                b: Pt { x: 100, y: 0 },
            }],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();
        // Pin wins over a zero-distance wire endpoint.
        assert!(matches!(
            snap(&doc, Pt { x: 10, y: 0 }, 5, 10, &mut scratch),
            Snap::Pin { .. }
        ));
        assert!(matches!(
            snap(&doc, Pt { x: 100, y: 1 }, 5, 2, &mut scratch),
            Snap::WireEnd { .. }
        ));
        assert!(matches!(
            snap(&doc, Pt { x: 60, y: 1 }, 5, 2, &mut scratch),
            Snap::WireBody { .. }
        ));
        assert_eq!(
            snap(&doc, Pt { x: 0, y: 100 }, 7, 2, &mut scratch),
            Snap::Grid(Pt { x: 0, y: 98 })
        );
        // Input -> grid(0,0) -> end(10,0) -> pin(20,0).
        let result = snap(&doc, Pt { x: 0, y: -15 }, 100, 10, &mut scratch);
        assert!(matches!(result, Snap::Pin { .. }));
        assert_eq!(snap(&doc, result.at(), 100, 10, &mut scratch), result);
    }
}
