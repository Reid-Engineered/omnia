//! GPUI-independent schematic document model.

pub mod arena;
pub mod command;
pub mod connectivity;
pub mod doc;
pub mod geom;
pub mod index;
mod points;
pub mod prefs;
mod undo;

pub use command::{Change, Command, CommandError, TextField};
pub use connectivity::{Connectivity, NetId};
pub use doc::{Doc, ItemId, Label, LabelId, Part, PartId, StoreError, SymbolId, Wire, WireId};

pub use index::{INDEX_CELL_NM, SpatialIndex};

pub use geom::{GRID_NM, GeomError, Orient, PIN_LATTICE_NM, Pt, Rect, SHEET_LIMIT_NM, snap_to};

pub mod route;
pub use route::l_route;

pub mod hit;
pub mod snap;
pub use hit::{Hit, hit_test};
pub use snap::{Snap, snap};
pub mod run;
pub use run::{Anchor, Boundary, Run};
pub mod follow;
pub use follow::{FollowError, FollowPlan, Isometry, RunFate, plan_follow};
pub mod spice_screen;
pub use spice_screen::{ScreenError, screen_embedded, subckt_ports};
pub mod units;
pub use units::{FiniteF64, ValueError, parse_value};
