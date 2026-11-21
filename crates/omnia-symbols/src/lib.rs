//! GPUI-independent symbol definitions compiled from vendored artwork.

use std::{io::Write as _, sync::Arc};

use omnia_doc::{Command, Orient, Part, Pt, Rect};

mod generated;

/// Identifier of a symbol definition inside [`SymbolLibrary`] (defined in `omnia-doc`).
pub use omnia_doc::SymbolId;

/// A connection pin on a [`SymbolDef`], in symbol-local nanometre coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pin {
    pub at: Pt,
    pub number: &'static str,
}

/// Stroke cap style for a symbol path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

impl LineCap {
    const fn svg_attr(self) -> &'static str {
        match self {
            Self::Butt => "butt",
            Self::Round => "round",
            Self::Square => "square",
        }
    }
}

/// Stroke join style for a symbol path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

impl LineJoin {
    const fn svg_attr(self) -> &'static str {
        match self {
            Self::Miter => "miter",
            Self::Round => "round",
            Self::Bevel => "bevel",
        }
    }
}

/// One compiled vector path inside a [`SymbolArt`], in `viewBox` units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArtPath {
    pub d: &'static str,
    pub fill: bool,
    pub stroke: bool,
    pub linecap: LineCap,
    pub linejoin: LineJoin,
    pub miterlimit: f32,
}

/// Compiled monochrome vector artwork for a symbol in a square `viewBox`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymbolArt {
    pub view_box_size: f32,
    pub original_stroke_units: f32,
    pub paths: &'static [ArtPath],
}

impl SymbolArt {
    /// Appends an SVG document for this artwork with stroked paths set to `stroke_units`.
    ///
    /// Callers pass `stroke_units` in `viewBox` units (K7: 150 units square), widening it
    /// when zoomed out so strokes never fall below one device pixel (D4).
    pub fn write_svg(&self, stroke_units: f32, out: &mut Vec<u8>) {
        let vb = clean_f32(self.view_box_size);
        let _ = write!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {vb} {vb}">"#
        );
        let stroke_w = clean_f32(stroke_units);
        let has_stroke = stroke_w.is_finite() && stroke_w > 0.0;
        for path in self.paths {
            let fill = if path.fill { "#000" } else { "none" };
            let _ = write!(out, r#"<path fill="{fill}""#);
            if path.stroke && has_stroke {
                let _ = write!(out, r##" stroke="#000" stroke-width="{stroke_w}""##);
                if path.linecap != LineCap::Butt {
                    let _ = write!(out, r#" stroke-linecap="{}""#, path.linecap.svg_attr());
                }
                if path.linejoin != LineJoin::Miter {
                    let _ = write!(out, r#" stroke-linejoin="{}""#, path.linejoin.svg_attr());
                } else if (path.miterlimit - 4.0).abs() > 1e-4 {
                    let ml = clean_f32(path.miterlimit);
                    let _ = write!(out, r#" stroke-miterlimit="{ml}""#);
                }
            }
            let _ = write!(out, r#" d="{}"/>"#, path.d);
        }
        let _ = out.write_all(b"</svg>");
    }
}

#[inline]
fn clean_f32(v: f32) -> f32 {
    if v == 0.0 { 0.0 } else { v }
}

/// Complete definition of one schematic symbol: identity, placed bounds, pins, and artwork.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SymbolDef {
    pub id: SymbolId,
    pub name: &'static str,
    pub bounds: Rect,
    pub pins: &'static [Pin],
    pub art: &'static SymbolArt,
}

/// Library of compiled schematic symbols.
#[derive(Debug)]
pub struct SymbolLibrary {
    defs: &'static [SymbolDef],
}

impl SymbolLibrary {
    /// Returns the built-in symbol library compiled from `pins.toml` and vendored SVGs.
    pub fn builtin() -> Arc<Self> {
        Arc::new(Self {
            defs: generated::SYMBOLS,
        })
    }

    /// Looks up a symbol definition by its [`SymbolId`].
    #[inline]
    pub fn get(&self, id: SymbolId) -> &SymbolDef {
        &self.defs[id.index()]
    }

    /// Finds a symbol by name (e.g. `"Resistor-IEEE-Standard"`).
    pub fn by_name(&self, name: &str) -> Option<SymbolId> {
        self.defs
            .iter()
            .find(|def| def.name == name)
            .map(|def| def.id)
    }

    /// Builds the [`Command::PlacePart`] that places symbol `id` at `at` with `orient` (A1.2,
    /// AD11). The command carries the symbol's local pins and bounds from its [`SymbolDef`], so
    /// the canvas place tool (T5.3) and the agent place parts through this one helper. `Doc`
    /// applies `orient` and `at` to them and validates the result.
    ///
    /// Panics, like [`Self::get`], if `id` is not in this library. A caller resolving a name
    /// uses [`Self::by_name`] and handles a missing symbol first.
    pub fn place_command(
        &self,
        id: SymbolId,
        at: Pt,
        orient: Orient,
        designator: Arc<str>,
        value: Arc<str>,
    ) -> Command {
        let def = self.get(id);
        Command::PlacePart {
            part: Part {
                symbol: id,
                at,
                orient,
                designator,
                value,
            },
            local_pins: def.pins.iter().map(|pin| pin.at).collect(),
            local_bounds: def.bounds,
        }
    }

    /// Iterates over all symbols in the library in deterministic ID order.
    pub fn iter(&self) -> impl Iterator<Item = &SymbolDef> {
        self.defs.iter()
    }

    /// Number of symbols in the library.
    #[inline]
    pub const fn len(&self) -> usize {
        self.defs.len()
    }

    /// Returns `true` if the library has no symbols.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{Doc, ItemId, PIN_LATTICE_NM};

    /// A1.2 done-when (AD11): for every built-in symbol under all 8 orientations, applying
    /// `place_command` gives the world pins `Doc::pins` reports, equal to the `SymbolDef` pins
    /// transformed by `Orient::apply` and offset by the placement point, in pin order.
    #[test]
    fn a1_2_place_command_world_pins_for_every_symbol_and_orientation() {
        let library = SymbolLibrary::builtin();
        assert!(!library.is_empty());
        let at = Pt {
            x: 7 * PIN_LATTICE_NM,
            y: -3 * PIN_LATTICE_NM,
        };
        let mut placed = 0;
        for def in library.iter() {
            for orient in Orient::ALL {
                let mut doc = Doc::new();
                let cmd = library.place_command(def.id, at, orient, "U1".into(), "v".into());
                let Command::PlacePart {
                    part,
                    local_pins,
                    local_bounds,
                } = &cmd
                else {
                    panic!("place_command must build PlacePart");
                };
                assert_eq!(part.symbol, def.id);
                assert_eq!((part.at, part.orient), (at, orient));
                assert_eq!((&*part.designator, &*part.value), ("U1", "v"));
                assert_eq!(*local_bounds, def.bounds);
                assert_eq!(local_pins.len(), def.pins.len());
                let part = part.clone();

                let change = doc.apply(cmd).unwrap_or_else(|e| {
                    panic!("placing {} at {orient:?} was refused: {e:?}", def.name)
                });
                let [ItemId::Part(id)] = change.added[..] else {
                    panic!("expected one added part, got {:?}", change.added);
                };
                let expected: Vec<Pt> = def
                    .pins
                    .iter()
                    .map(|pin| at + orient.apply(pin.at))
                    .collect();
                assert_eq!(
                    doc.pins(id).unwrap(),
                    expected.as_slice(),
                    "{} at {orient:?}",
                    def.name
                );
                assert_eq!(doc.part(id), Some(&part));
                placed += 1;
            }
        }
        assert_eq!(placed, library.len() * 8);
    }
}
