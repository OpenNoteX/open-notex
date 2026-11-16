//! Stable identifiers. Notes use the same id namespace as tasks, habits and
//! reminders so links and tags can point at any of them.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// What an id refers to. Stored as text in `item_type` columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemKind {
    Note,
    Task,
    Habit,
    Reminder,
    List,
}

impl ItemKind {
    pub const ALL: [ItemKind; 5] = [
        ItemKind::Note,
        ItemKind::Task,
        ItemKind::Habit,
        ItemKind::Reminder,
        ItemKind::List,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            ItemKind::Note => "note",
            ItemKind::Task => "task",
            ItemKind::Habit => "habit",
            ItemKind::Reminder => "reminder",
            ItemKind::List => "list",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    #[error("empty id")]
    Empty,
    #[error("not a valid ULID: {0}")]
    Invalid(String),
}

/// A ULID-backed identifier. Lexicographic order matches creation order, which
/// keeps `sort_key` and oplog scans cheap.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemId(String);

impl ItemId {
    pub fn new() -> Self {
        Self(ulid::Ulid::generate().to_string())
    }

    pub fn from_ulid(ulid: ulid::Ulid) -> Self {
        Self(ulid.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl Default for ItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ItemId {
    type Err = IdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(IdError::Empty);
        }
        match value.parse::<ulid::Ulid>() {
            Ok(ulid) => Ok(Self(ulid.to_string())),
            Err(_) => Err(IdError::Invalid(value.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_text() {
        let id = ItemId::new();
        let parsed: ItemId = id.as_str().parse().expect("valid ulid");
        assert_eq!(id, parsed);
    }

    #[test]
    fn ids_sort_by_creation() {
        // Written explicitly so the test does not depend on generator timing.
        let first: ItemId = "01ARZ3NDEKTSV4RRFFQ69G5FAV".parse().unwrap();
        let second: ItemId = "01BX5ZZKBKACTAV9WEVGEMMVRZ".parse().unwrap();
        assert!(first < second);
    }

    #[test]
    fn kinds_round_trip() {
        for kind in ItemKind::ALL {
            assert_eq!(ItemKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ItemKind::parse("nope"), None);
    }

    #[test]
    fn rejects_non_ulids() {
        assert_eq!("".parse::<ItemId>(), Err(IdError::Empty));
        assert!("not-a-ulid".parse::<ItemId>().is_err());
    }
}
