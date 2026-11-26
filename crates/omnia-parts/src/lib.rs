//! On-disk component and library pack definitions.

pub mod schema;
pub mod spice;
pub use schema::PartIdSlug;
pub use spice::{Deck, DeckSource, DefKind, EntryKind, Extraction, MemSource, SpiceError, extract};
