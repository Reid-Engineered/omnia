//! Priority snapping in world nanometres (K14: caller converts 10 px).

use crate::hit::{closest, search_area};
use crate::{Doc, ItemId, PartId, Pt, Rect, SHEET_LIMIT_NM, WireId, snap_to};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Snap {
    Pin { part: PartId, pin: usize, at: Pt },
    WireEnd { wire: WireId, at: Pt },
    WireBody { wire: WireId, at: Pt },
    Grid(Pt),
}

impl Snap {
    pub fn at(self) -> Pt {
        match self {
            Self::Pin { at, .. }
            | Self::WireEnd { at, .. }
            | Self::WireBody { at, .. }
            | Self::Grid(at) => at,
        }
    }
}

/// Pin, wire end, wire body, then grid; nearest within each tier, ties by ID,
/// pin number or endpoint coordinate. Settles through higher priorities to enforce
/// INV9 even for off-grid geometry: rounding/projection can enter another target's
/// radius. Consequently the final point can be farther than `radius` from input.
/// Grid points are clipped to the outermost in-sheet grid line (K1).
/// Uses the index maintained by Doc, including world pins.
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
    let mut best = None;
    let mut consider = |tier: u8, id: u32, sub: usize, target: Snap| {
        let p = target.at();
        let distance = at.distance_sq(p);
        if distance <= i64::from(radius).pow(2) {
            let key = (tier, distance, id, sub, p.x, p.y);
            if best.as_ref().is_none_or(|(old, _)| key < *old) {
                best = Some((key, target));
            }
        }
    };
    for &item in scratch.iter() {
        match item {
            ItemId::Part(part) => {
                if let Some(pins) = doc.pins(part) {
                    for (pin, &p) in pins.iter().enumerate() {
                        consider(0, part.0, pin, Snap::Pin { part, pin, at: p });
                    }
                }
            }
            ItemId::Wire(wire) => {
                if let Some(w) = doc.wire(wire) {
                    for p in [w.a, w.b] {
                        consider(1, wire.0, 0, Snap::WireEnd { wire, at: p });
                    }
                    let p = closest(at, Rect::from_points(w.a, w.b).unwrap());
                    consider(2, wire.0, 0, Snap::WireBody { wire, at: p });
                }
            }
            ItemId::Label(_) => {}
        }
    }
    best.map(|(_, result)| result).unwrap_or_else(|| {
        let limit = SHEET_LIMIT_NM / grid * grid;
        let p = snap_to(at, grid);
        Snap::Grid(Pt {
            x: p.x.clamp(-limit, limit),
            y: p.y.clamp(-limit, limit),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Orient, Part, SymbolId};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn inv9_dense_off_grid_geometry(
            pins in prop::collection::vec((-100i32..=100, -100i32..=100), 0..30),
            wires in prop::collection::vec((-100i32..=100, -100i32..=100, 1i32..=100, any::<bool>()), 0..30),
            x in -200i32..=200, y in -200i32..=200,
            grid in 1i32..=1000, radius in 0i32..=200,
        ) {
            let doc = Doc::from_items([(
                Part { symbol: SymbolId(0), at: Pt::ZERO, orient: Orient::IDENTITY, designator: "R".into(), value: "".into() },
                pins.into_iter().map(|(x, y)| Pt { x, y }).collect(), Rect::new(Pt::ZERO, Pt::ZERO).unwrap()
            )], wires.into_iter().map(|(x, y, length, vertical)| crate::Wire {
                a: Pt { x, y }, b: if vertical { Pt { x, y: y + length } } else { Pt { x: x + length, y } }
            }), []).unwrap();
            let mut scratch = Vec::new();
            let first = snap(&doc, Pt { x, y }, grid, radius, &mut scratch);
            prop_assert_eq!(snap(&doc, first.at(), grid, radius, &mut scratch), first);
        }
    }

    #[test]
    fn priorities_and_settling_off_grid_targets() {
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt { x: 20, y: 0 },
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "".into(),
                },
                vec![Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [crate::Wire {
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
