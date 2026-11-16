//! GPUI-independent schematic document model.

pub mod arena;
pub mod command;
pub mod connectivity;
pub mod doc;
pub mod geom;
pub mod index;
mod points;
mod undo;

pub use command::{Change, Command, CommandError, TextField};
pub use connectivity::{Connectivity, NetId};
pub use doc::{Doc, ItemId, Label, LabelId, Part, PartId, StoreError, SymbolId, Wire, WireId};

pub use index::{INDEX_CELL_NM, SpatialIndex};

pub use geom::{GRID_NM, GeomError, Orient, PIN_LATTICE_NM, Pt, Rect, SHEET_LIMIT_NM, snap_to};

pub mod hit;
pub mod snap;
pub use hit::{Hit, hit_test};
pub use snap::{Snap, snap};
