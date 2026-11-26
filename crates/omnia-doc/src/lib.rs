//! GPUI-independent schematic document model.

pub mod arena;
pub mod clip;
pub mod command;
pub mod connectivity;
pub mod doc;
pub mod geom;
pub mod index;
mod points;
pub mod prefs;
mod undo;

pub use clip::{Clip, ClipPart};
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
pub mod part_def;
pub use part_def::{
    DefError, EmbeddedParamSpec, EmbeddedPartDef, EmbeddedPin, EmbeddedSimBinding, InstanceError,
    ParamFault, ParamKind, ParamValue, Params, PartDefId, PinRole, PrimitiveKind, SupplyDecl,
};
pub mod spice_screen;
pub use spice_screen::{ScreenError, screen_embedded, subckt_ports};
pub mod units;
pub use units::{FiniteF64, ValueError, parse_value};

/// Test-only `proptest` config that keeps `default_cases` unless `PROPTEST_CASES` raises it,
/// so gate runs (T6.4: 10× default case count) can scale tests that declare a custom baseline
/// without ever lowering any test below its default.
#[cfg(test)]
pub(crate) fn proptest_config(default_cases: u32) -> proptest::test_runner::Config {
    let mut cfg = proptest::test_runner::Config::default();
    if std::env::var_os("PROPTEST_CASES").is_none() {
        cfg.cases = default_cases;
    } else {
        cfg.cases = cfg.cases.max(default_cases);
    }
    cfg
}
