//! Domain entities and identifiers. This crate performs no I/O and reads no
//! clock, so everything here is trivially testable.

pub mod checklist;
pub mod ids;
pub mod update;

pub use checklist::{TaskInput, parse_task_input};
pub use ids::{IdError, ItemId, ItemKind};
pub use update::{Availability, Version};

/// How deep a task tree goes: a task, its steps, and their steps. A fourth
/// level would need a tree view and a way to move a branch; three is enough to
/// say "break this down". Shared so the store's refusal and the UI's `+` agree.
pub const MAX_TASK_DEPTH: usize = 3;
