//! GPUI-independent symbol definitions compiled from vendored artwork.

use std::{io::Write as _, sync::Arc};

use omnia_doc::{Pt, Rect};

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
