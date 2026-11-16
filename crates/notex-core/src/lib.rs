//! Domain entities and identifiers. This crate performs no I/O and reads no
//! clock, so everything here is trivially testable.

pub mod ids;

pub use ids::{IdError, ItemId, ItemKind};
