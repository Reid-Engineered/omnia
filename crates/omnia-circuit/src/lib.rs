//! Derived, GPUI-independent Circuit Context Engine (`omnia-circuit`).
//!
//! Reads an [`omnia_doc::Doc`] through its public API only and derives dense row storage, CSR
//! net membership, interned strings, stable [`CircuitKey`]s (CK1), display names (CK2), pin roles
//! (CK3) and ground evidence (CK4). Never mutates a document (CD1, CINV1).

pub mod graph;
pub mod intern;
pub mod keys;

pub use graph::{
    Circuit, EvidenceTier, GroundEvidence, HUB_PIN_THRESHOLD, NetFlags, NetKind, NetRef, NetRow,
    PartRow, PinRole, PinRow, SymbolClass,
};
pub use intern::{Interner, StrId};
pub use keys::{CircuitKey, cmp_natural, write_ck9_quoted};
