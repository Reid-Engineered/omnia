//! GPUI-independent schematic document model.

pub mod arena;
pub mod doc;
pub mod geom;
pub mod index;
mod points;

pub use doc::{Doc, ItemId, Label, LabelId, Part, PartId, StoreError, SymbolId, Wire, WireId};

pub use index::{INDEX_CELL_NM, SpatialIndex};

pub use geom::{GRID_NM, GeomError, Orient, PIN_LATTICE_NM, Pt, Rect, SHEET_LIMIT_NM, snap_to};
