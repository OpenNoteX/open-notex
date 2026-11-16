//! The sidebar's routes.
//!
//! A route is a navigation target, not data. Today, Inbox and Upcoming are views
//! over the same tables; Projects lists `lists`; the rest are their own screens.
//! Keeping them in one enum means the sidebar, the keyboard shortcuts and the
//! snapshot scenes cannot drift apart.

/// What the content pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Inbox,
    Today,
    Upcoming,
    Projects,
    Notes,
    Tags,
    Habits,
    Reminders,
    Search,
    Settings,
}

impl Route {
    /// Every route, in sidebar order. The sidebar renders `SECTIONS`; this list
    /// exists so tests can assert nothing is missing or duplicated.
    pub const ALL: [Route; 10] = [
        Route::Inbox,
        Route::Today,
        Route::Upcoming,
        Route::Projects,
        Route::Notes,
        Route::Tags,
        Route::Habits,
        Route::Reminders,
        Route::Search,
        Route::Settings,
    ];

    /// Stable, lowercase identifier. Used for element keys, scene names and the
    /// `--route` flag, so it must not change casually.
    pub const fn key(self) -> &'static str {
        match self {
            Route::Inbox => "inbox",
            Route::Today => "today",
            Route::Upcoming => "upcoming",
            Route::Projects => "projects",
            Route::Notes => "notes",
            Route::Tags => "tags",
            Route::Habits => "habits",
            Route::Reminders => "reminders",
            Route::Search => "search",
            Route::Settings => "settings",
        }
    }

    /// The visible sidebar label. Also the accessible name.
    pub const fn label(self) -> &'static str {
        match self {
            Route::Inbox => "Inbox",
            Route::Today => "Today",
            Route::Upcoming => "Upcoming",
            Route::Projects => "Projects",
            Route::Notes => "Notes",
            Route::Tags => "Tags",
            Route::Habits => "Habits",
            Route::Reminders => "Reminders",
            Route::Search => "Search",
            Route::Settings => "Settings",
        }
    }

    /// One line under the title in the content pane.
    pub const fn subtitle(self) -> &'static str {
        match self {
            Route::Inbox => "Captured, not yet filed",
            Route::Today => "What is due before tomorrow",
            Route::Upcoming => "Scheduled after today",
            Route::Projects => "One list per thing you are building",
            Route::Notes => "Markdown files in your vault",
            Route::Tags => "Across notes, tasks and habits",
            Route::Habits => "What you are repeating",
            Route::Reminders => "What must not be forgotten",
            Route::Search => "Find anything by title or body",
            Route::Settings => "Vault, appearance, notifications",
        }
    }

    /// Whether the route has a list to select from, and therefore a detail pane.
    /// Search and Settings are single-pane screens.
    pub const fn has_detail(self) -> bool {
        !matches!(self, Route::Search | Route::Settings)
    }

    /// Parse a `--route` value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|route| route.key() == value)
    }

    /// The sidebar glyph.
    pub const fn icon(self) -> &'static rune::Icon {
        match self {
            Route::Inbox => &crate::icons::INBOX,
            Route::Today => &crate::icons::TODAY,
            Route::Upcoming => &crate::icons::UPCOMING,
            Route::Projects => &crate::icons::PROJECTS,
            Route::Notes => &crate::icons::NOTES,
            Route::Tags => &crate::icons::TAGS,
            Route::Habits => &crate::icons::HABITS,
            Route::Reminders => &crate::icons::REMINDERS,
            Route::Search => &crate::icons::SEARCH,
            Route::Settings => &crate::icons::SETTINGS,
        }
    }

    /// What the empty state says before there is any data.
    pub const fn empty_title(self) -> &'static str {
        match self {
            Route::Inbox => "Inbox is clear",
            Route::Today => "Nothing due today",
            Route::Upcoming => "Nothing scheduled",
            Route::Projects => "No projects yet",
            Route::Notes => "No notes yet",
            Route::Tags => "No tags yet",
            Route::Habits => "No habits yet",
            Route::Reminders => "No reminders yet",
            Route::Search => "Search your vault",
            Route::Settings => "Settings",
        }
    }

    /// Copy for an empty screen. It describes what will appear, and never
    /// promises a command the shell does not answer yet.
    pub const fn empty_hint(self) -> &'static str {
        match self {
            Route::Inbox => "Captured tasks wait here until you file them into a project.",
            Route::Today => "Tasks with a due date appear here on the day they are due.",
            Route::Upcoming => "Everything scheduled after today appears here.",
            Route::Projects => "A project groups tasks that share a goal.",
            Route::Notes => "Each note is a Markdown file, readable by any editor.",
            Route::Tags => "Tags from every note, task and habit are collected here.",
            Route::Habits => "Habits track what you repeat, with a streak for each.",
            Route::Reminders => "Reminders appear here with the time they fire.",
            Route::Search => "Search titles and bodies across the vault. Nothing leaves this Mac.",
            Route::Settings => "Choose your vault, theme and notification behaviour.",
        }
    }
}

/// A group of rows in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub routes: &'static [Route],
}

pub const SECTIONS: &[Section] = &[
    Section {
        title: "",
        routes: &[Route::Inbox, Route::Today, Route::Upcoming],
    },
    Section {
        title: "Library",
        routes: &[Route::Projects, Route::Notes, Route::Tags],
    },
    Section {
        title: "Routines",
        routes: &[Route::Habits, Route::Reminders],
    },
];

/// The rows pinned to the bottom of the sidebar.
pub const FOOTER: &[Route] = &[Route::Search, Route::Settings];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_route_is_rendered_exactly_once() {
        let mut seen: Vec<Route> = SECTIONS
            .iter()
            .flat_map(|s| s.routes.iter().copied())
            .collect();
        seen.extend_from_slice(FOOTER);

        let unique: HashSet<Route> = seen.iter().copied().collect();
        assert_eq!(seen.len(), unique.len(), "a route appears twice");
        assert_eq!(unique.len(), Route::ALL.len(), "a route is missing");

        for route in Route::ALL {
            assert!(
                unique.contains(&route),
                "{} is not in the sidebar",
                route.key()
            );
        }
    }

    #[test]
    fn keys_and_labels_are_unique_and_non_empty() {
        let keys: HashSet<_> = Route::ALL.iter().map(|r| r.key()).collect();
        let labels: HashSet<_> = Route::ALL.iter().map(|r| r.label()).collect();
        assert_eq!(keys.len(), Route::ALL.len());
        assert_eq!(labels.len(), Route::ALL.len());

        for route in Route::ALL {
            assert!(!route.label().trim().is_empty());
            assert!(!route.subtitle().trim().is_empty());
            assert!(!route.empty_title().trim().is_empty());
            assert!(!route.empty_hint().trim().is_empty());
            assert_eq!(route.key(), route.key().to_lowercase());
        }
    }

    #[test]
    fn sections_are_named_and_non_empty() {
        for section in SECTIONS {
            assert!(!section.routes.is_empty());
        }
        assert_eq!(SECTIONS[0].title, "", "the first group is unlabelled");
    }
}
