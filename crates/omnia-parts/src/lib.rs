//! On-disk component and library pack definitions.

pub mod catalog;
pub mod container;
mod generated;
mod hash;
pub mod schema;
pub mod spice;

pub use catalog::{Catalog, LicenseInfo, PartDef};
pub use container::{
    LibraryError, LibrarySource, PackSummary, ResolvedPart, pack_olib, pack_opart,
};
pub use hash::ContentHash;
pub use schema::PartIdSlug;
pub use spice::{Deck, DeckSource, DefKind, EntryKind, Extraction, MemSource, SpiceError, extract};
