//! T5.3: part placement — anchor-pin grid snapping (K2), deterministic designator assignment,
//! and shell-supplied placement provider integration (`parts-plan.md` section 14).
//!
//! Committing `Tool::Place(symbol)` asks the installed [`PlaceProvider`] (set through
//! [`crate::SheetCanvas::set_place_provider`] or [`crate::OverlayView::set_place_provider`]) for
//! the [`Command`] carrying the chosen symbol, anchor-pin-snapped origin, orientation, and
//! designator. When no provider is installed, placement defaults to
//! [`SymbolLibrary::place_command`].

use std::{rc::Rc, sync::Arc};

use omnia_doc::{Command, Doc, GRID_NM, Orient, Pt, SHEET_LIMIT_NM, SymbolId, snap_to};
use omnia_symbols::SymbolLibrary;

/// Input passed to a shell-supplied [`PlaceProvider`] when `Tool::Place` commits
/// (`parts-plan.md` section 14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaceRequest {
    pub symbol: SymbolId,
    /// Part placement origin in world nanometres, snapped so anchor pin 0 lands on `GRID_NM` (K2).
    pub at: Pt,
    /// Orientation accumulated from R, X and Y presses before placement.
    pub orient: Orient,
    /// Next unused designator for this symbol's prefix in the target document.
    pub designator: Arc<str>,
}

/// Shell-supplied placement provider (`parts-plan.md` section 14).
///
/// Returning `Some(cmd)` commits `cmd` as one document edit (INV5); returning `None` cancels
/// that placement without touching the document.
pub type PlaceProvider = Rc<dyn Fn(PlaceRequest) -> Option<Command>>;

/// Returns the symbol-local position of `symbol`'s anchor pin (pin 0), or [`Pt::ZERO`] when
/// `symbol` has no pins or is unknown to the built-in library.
pub fn anchor_pin_local(symbol: SymbolId) -> Pt {
    let library = SymbolLibrary::builtin();
    if symbol.index() >= library.len() {
        return Pt::ZERO;
    }
    library
        .get(symbol)
        .pins
        .first()
        .map_or(Pt::ZERO, |pin| pin.at)
}

/// Largest multiple of [`GRID_NM`] that lies within K1 (`±SHEET_LIMIT_NM`).
const MAX_GRID_NM: i64 = (SHEET_LIMIT_NM / GRID_NM) as i64 * GRID_NM as i64;

/// Snaps `raw_cursor` so `symbol`'s anchor pin (pin 0) under `orient` lands on [`GRID_NM`] (K2),
/// keeping both the anchor pin and resulting part origin inside K1 (`±SHEET_LIMIT_NM`).
///
/// For symbols whose pin 0 lies on [`GRID_NM`] (resistor, capacitor, diode, potentiometer,
/// ground), this equals `snap_to(raw_cursor, GRID_NM)` away from the sheet edge. For symbols
/// whose pin 0 lies on the half-grid [`omnia_doc::PIN_LATTICE_NM`] (such as `IC-COM-OpAmp`), the
/// origin shifts by a half-grid step so pin 0 lands squarely on [`GRID_NM`].
pub fn snap_place_origin(symbol: SymbolId, raw_cursor: Pt, orient: Orient) -> Pt {
    let offset = orient.apply(anchor_pin_local(symbol));
    let pin_raw = Pt {
        x: (i64::from(raw_cursor.x) + i64::from(offset.x)).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
        y: (i64::from(raw_cursor.y) + i64::from(offset.y)).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
    };
    let snapped_pin = snap_to(pin_raw, GRID_NM);
    let limit = i64::from(SHEET_LIMIT_NM);
    let mut x = i64::from(snapped_pin.x) - i64::from(offset.x);
    let mut y = i64::from(snapped_pin.y) - i64::from(offset.y);
    let step = i64::from(GRID_NM);
    while x < -limit {
        x += step;
    }
    while x > limit {
        x -= step;
    }
    while y < -limit {
        y += step;
    }
    while y > limit {
        y -= step;
    }
    Pt {
        x: x as i32,
        y: y as i32,
    }
}

/// Canonical designator prefix for a built-in symbol (`"R"`, `"C"`, `"D"`, `"U"`, `"GND"`, or `"X"`).
pub fn designator_prefix(symbol: SymbolId) -> &'static str {
    let library = SymbolLibrary::builtin();
    if symbol.index() >= library.len() {
        return "X";
    }
    let name = library.get(symbol).name;
    if name.starts_with("Resistor-") {
        "R"
    } else if name.starts_with("Capacitor-") {
        "C"
    } else if name.starts_with("Diode-") {
        "D"
    } else if name.starts_with("IC-") {
        "U"
    } else if name.starts_with("Ground-") {
        "GND"
    } else {
        "X"
    }
}

/// Deterministically picks the smallest unused positive integer designator for `symbol` in `doc`
/// (e.g. `"R1"`, `"R2"`, reusing gaps left by deletions).
pub fn next_designator(doc: &Doc, symbol: SymbolId) -> Arc<str> {
    let prefix = designator_prefix(symbol);
    let mut used = Vec::new();
    for (_, part) in doc.parts() {
        if let Some(rest) = part.designator.strip_prefix(prefix)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
            && !rest.starts_with('0')
            && let Ok(n) = rest.parse::<u32>()
            && n > 0
        {
            used.push(n);
        }
    }
    used.sort_unstable();
    used.dedup();
    let mut next = 1u32;
    for n in used {
        if n == next {
            next = next.saturating_add(1);
        } else if n > next {
            break;
        }
    }
    Arc::from(format!("{prefix}{next}"))
}

/// Builds the placement [`Command`] for `(symbol, at, orient)` against `doc`.
///
/// Calls `provider` when installed; otherwise falls back to [`SymbolLibrary::place_command`]
/// with an empty value string. Returns `None` if an installed provider declines placement or if
/// `symbol` is unknown when using the default library fallback.
pub fn build_place_command(
    doc: &Doc,
    symbol: SymbolId,
    at: Pt,
    orient: Orient,
    provider: Option<&PlaceProvider>,
) -> Option<Command> {
    let designator = next_designator(doc, symbol);
    if let Some(provider) = provider {
        return provider(PlaceRequest {
            symbol,
            at,
            orient,
            designator,
        });
    }
    let library = SymbolLibrary::builtin();
    if symbol.index() >= library.len() {
        return None;
    }
    Some(library.place_command(symbol, at, orient, designator, Arc::from("")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{PIN_LATTICE_NM, Part};

    #[test]
    fn t5_3_k2_anchor_pin_snaps_to_grid_for_every_symbol_and_orientation() {
        let lib = SymbolLibrary::builtin();
        let cursors = [
            Pt::ZERO,
            Pt {
                x: GRID_NM / 3,
                y: -GRID_NM / 3,
            },
            Pt {
                x: PIN_LATTICE_NM,
                y: -PIN_LATTICE_NM,
            },
            Pt {
                x: 7 * GRID_NM + 500_000,
                y: -4 * GRID_NM - 900_000,
            },
            Pt {
                x: SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
        ];
        for def in lib.iter() {
            for orient in Orient::ALL {
                for &raw in &cursors {
                    let at = snap_place_origin(def.id, raw, orient);
                    assert!(
                        at.is_in_bounds(),
                        "{} {orient:?} raw={raw:?} -> {at:?}",
                        def.name
                    );
                    if let Some(anchor) = def.pins.first() {
                        let world_pin = at + orient.apply(anchor.at);
                        assert!(
                            world_pin.is_in_bounds(),
                            "{} {orient:?} raw={raw:?} pin 0 out of K1: {world_pin:?}",
                            def.name
                        );
                        assert_eq!(
                            world_pin.x % GRID_NM,
                            0,
                            "{} {orient:?} pin 0 x off grid: {world_pin:?}",
                            def.name
                        );
                        assert_eq!(
                            world_pin.y % GRID_NM,
                            0,
                            "{} {orient:?} pin 0 y off grid: {world_pin:?}",
                            def.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn t5_3_opamp_origin_offsets_to_half_grid_so_anchor_pin_lands_on_grid() {
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        let at = snap_place_origin(opamp, Pt::ZERO, Orient::IDENTITY);
        assert_eq!(at.x.rem_euclid(GRID_NM), PIN_LATTICE_NM);
        assert_eq!(at.y.rem_euclid(GRID_NM), PIN_LATTICE_NM);
        let world_pin0 = at + def.pins[0].at;
        assert_eq!(world_pin0.x % GRID_NM, 0);
        assert_eq!(world_pin0.y % GRID_NM, 0);
    }

    #[test]
    fn t5_3_next_designator_is_deterministic_and_fills_gaps() {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let pot = lib.by_name("Resistor-IEEE-Potentiometer").unwrap();
        let cap = lib.by_name("Capacitor-IEEE-NonPolarized").unwrap();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let gnd = lib.by_name("Ground-COM-General").unwrap();

        let mut doc = Doc::new();
        assert_eq!(&*next_designator(&doc, res), "R1");
        assert_eq!(&*next_designator(&doc, pot), "R1");
        assert_eq!(&*next_designator(&doc, cap), "C1");
        assert_eq!(&*next_designator(&doc, opamp), "U1");
        assert_eq!(&*next_designator(&doc, gnd), "GND1");

        // Place R1 and R3; next should fill gap R2.
        let r1 = lib.place_command(res, Pt::ZERO, Orient::IDENTITY, "R1".into(), "".into());
        let r3 = lib.place_command(
            res,
            Pt {
                x: 6 * GRID_NM,
                y: 0,
            },
            Orient::IDENTITY,
            "R3".into(),
            "".into(),
        );
        doc.apply(r1).unwrap();
        doc.apply(r3).unwrap();
        assert_eq!(&*next_designator(&doc, res), "R2");
        assert_eq!(&*next_designator(&doc, pot), "R2");
    }

    #[test]
    fn t5_3_build_place_command_uses_provider_or_falls_back_to_symbol_library() {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let doc = Doc::new();
        let at = Pt {
            x: 2 * GRID_NM,
            y: -GRID_NM,
        };

        // Default fallback without provider.
        let default_cmd = build_place_command(&doc, res, at, Orient::ROT_90, None).unwrap();
        assert_eq!(
            default_cmd,
            lib.place_command(res, at, Orient::ROT_90, "R1".into(), "".into())
        );

        // Unknown symbol returns None on fallback.
        assert!(build_place_command(&doc, SymbolId(999), at, Orient::IDENTITY, None).is_none());

        // Custom provider controls the returned Command.
        let stub: PlaceProvider = Rc::new(move |req| {
            let def = lib.get(req.symbol);
            Some(Command::PlacePart {
                part: Part {
                    symbol: req.symbol,
                    at: req.at,
                    orient: req.orient,
                    designator: req.designator,
                    value: Arc::from("4.7k"),
                },
                local_pins: def.pins.iter().map(|p| p.at).collect(),
                local_bounds: def.bounds,
                def: omnia_doc::PlaceDef::Unmodelled,
                params: omnia_doc::Params::new(),
            })
        });
        let provided = build_place_command(&doc, res, at, Orient::FLIP_X, Some(&stub)).unwrap();
        let Command::PlacePart { part, .. } = provided else {
            panic!("expected PlacePart");
        };
        assert_eq!((&*part.designator, &*part.value), ("R1", "4.7k"));
        assert_eq!((part.at, part.orient), (at, Orient::FLIP_X));

        // Provider returning None cancels placement without falling back.
        let decline: PlaceProvider = Rc::new(|_| None);
        assert!(build_place_command(&doc, res, at, Orient::IDENTITY, Some(&decline)).is_none());
    }
}
