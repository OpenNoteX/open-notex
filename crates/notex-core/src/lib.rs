//! Domain entities and identifiers. This crate performs no I/O and reads no
//! clock, so everything here is trivially testable.

pub mod checklist;
pub mod ids;
pub mod update;

pub use checklist::{TaskInput, parse_task_input};
pub use ids::{IdError, ItemId, ItemKind};
pub use update::{Availability, Version};
