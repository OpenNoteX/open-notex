//! The sidebar's routes.
//!
//! A route is a navigation target, not data. Home is the dashboard; Tasks is one
//! checklist, grouped by when it is due, with projects as a filter inside it;
//! Notes is the only route with a detail pane. Keeping them in one enum means
//! the sidebar, the keyboard shortcuts and the snapshot scenes cannot drift
//! apart.

/// What the content pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    Home,
    Tasks,
    Notes,
    Tags,
    Habits,
    Reminders,
    Search,
    Settings,
}

/// How a route fills the window. The route decides the layout now; a route is
/// not a promise of a list plus a detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Home: blocks of the day, one scrolling column.
    Dashboard,
    /// Tasks, Tags, Habits, Reminders: one list, no detail pane.
    List,
    /// Notes: a list on the left, the open note on the right.
    ListDetail,
    /// Search and Settings: a single page.
    Page,
}

impl Route {
    /// Every route, in sidebar order. The sidebar renders `SECTIONS`; this list
    /// exists so tests can assert nothing is missing or duplicated.
    pub const ALL: [Route; 8] = [
        Route::Home,
        Route::Tasks,
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
            Route::Home => "home",
            Route::Tasks => "tasks",
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
            Route::Home => "Home",
            Route::Tasks => "Tasks",
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
            Route::Home => "Today at a glance",
            Route::Tasks => "One list, grouped by when it is due",
            Route::Notes => "Markdown files in your vault",
            Route::Tags => "Across notes, tasks and habits",
            Route::Habits => "What you are repeating",
            Route::Reminders => "What must not be forgotten",
            Route::Search => "Find anything by title or body",
            Route::Settings => "Vault, appearance, notifications",
        }
    }

    pub const fn layout(self) -> Layout {
        match self {
            Route::Home => Layout::Dashboard,
            Route::Tasks | Route::Tags | Route::Habits | Route::Reminders => Layout::List,
            Route::Notes => Layout::ListDetail,
            Route::Search | Route::Settings => Layout::Page,
        }
    }

    /// Whether the route has a note list to select from, and therefore a detail
    /// pane. Only Notes does.
    pub const fn has_detail(self) -> bool {
        matches!(self.layout(), Layout::ListDetail)
    }

    /// Parse a `--route` value.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|route| route.key() == value)
    }

    /// The sidebar glyph.
    pub const fn icon(self) -> &'static rune::Icon {
        match self {
            Route::Home => &crate::icons::HOME,
            Route::Tasks => &crate::icons::TASKS,
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
            Route::Home => "Nothing here yet",
            Route::Tasks => "No tasks yet",
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
            Route::Home => "Your day, your tasks and your notes, in one place.",
            Route::Tasks => "Add a task above. Type #project to file it under a project.",
            Route::Notes => "Each note is a Markdown file, readable by any editor.",
            Route::Tags => "Tags from every note, task and habit are collected here.",
            Route::Habits => "Habits track what you repeat, with a streak for each.",
            Route::Reminders => "Reminders are not in this build yet.",
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
        routes: &[Route::Home, Route::Tasks],
    },
    Section {
        title: "Library",
        routes: &[Route::Notes, Route::Tags],
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

    #[test]
    fn home_is_the_first_route_and_tasks_follows_it() {
        assert_eq!(Route::ALL[0], Route::Home);
        assert_eq!(SECTIONS[0].routes, [Route::Home, Route::Tasks]);
        assert_eq!(Route::Home.layout(), Layout::Dashboard);
    }

    #[test]
    fn only_notes_has_a_detail_pane() {
        for route in Route::ALL {
            assert_eq!(
                route.has_detail(),
                route == Route::Notes,
                "{} has the wrong layout",
                route.key()
            );
        }
    }

    #[test]
    fn projects_are_not_a_route() {
        for route in Route::ALL {
            assert_ne!(
                route.key(),
                "projects",
                "projects are a filter, not a screen"
            );
        }
    }
}
