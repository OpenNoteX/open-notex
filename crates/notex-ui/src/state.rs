//! The window's state.
//!
//! `view()` is a pure function of this struct: no database, no filesystem, no
//! clock. Everything here is cheap to clone and cheap to compare, because Rune
//! re-runs the view on every event.

use std::ffi::OsString;

use rune_kit::shell::PaneState;

use crate::routes::Route;

/// What the data behind the current route looks like.
///
/// The skeleton has no store yet, so every route is `Ready` with no items. The
/// other two variants exist now because empty, loading and failed states are the
/// hardest thing to retrofit once a screen has content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Ready,
    Loading,
    Failed(String),
}

/// A row in the notes list. The body is not here: it is read on demand, so a long
/// vault never sits in memory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteRow {
    pub id: String,
    pub title: String,
    /// Frontmatter tags, without the leading `#`.
    pub tags: Vec<String>,
}

impl NoteRow {
    pub fn new(id: impl Into<String>, title: impl Into<String>, tags: &[&str]) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    /// The second line of the row: `work · review`, or the empty string.
    pub fn tag_line(&self) -> String {
        self.tags.join(" · ")
    }
}

/// What the detail pane holds for the selected note.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NoteBody {
    #[default]
    None,
    Loading,
    Ready(String),
    Failed(String),
}

pub struct State {
    /// What the content pane is showing.
    pub route: Route,
    /// Sidebar width and open state, owned by the app as rune-kit requires.
    pub sidebar: PaneState,
    /// The detail pane's width and open state.
    pub detail: PaneState,
    pub load: Load,
    /// The notes list, alphabetical, from the index.
    pub notes: Vec<NoteRow>,
    /// The selected note's id; `None` when the list has no selection.
    pub selected: Option<String>,
    /// The selected note's body, or why it is not here yet.
    pub body: NoteBody,
    /// A note the list asked the app to read. The app drains it in `poll`; the
    /// view never opens a file itself.
    pub wanted: Option<String>,
    /// Where notes live, for display. The app opens it; the UI only shows it.
    pub vault: String,
    /// Build identity, shown in Settings.
    pub version: &'static str,
}

impl State {
    pub fn new() -> Self {
        Self::with_vault(default_vault())
    }

    pub fn with_vault(vault: impl Into<String>) -> Self {
        Self {
            route: Route::Today,
            sidebar: PaneState::new(216.0).range(180.0, 320.0),
            detail: PaneState::new(440.0).range(320.0, 720.0),
            load: Load::Ready,
            notes: Vec::new(),
            selected: None,
            body: NoteBody::None,
            wanted: None,
            vault: vault.into(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Replace the list. A selection whose note disappeared (deleted or renamed
    /// elsewhere) is cleared rather than left pointing at nothing.
    pub fn set_notes(&mut self, notes: Vec<NoteRow>) {
        if let Some(selected) = &self.selected
            && !notes.iter().any(|note| &note.id == selected)
        {
            if self.wanted.as_deref() == Some(selected.as_str()) {
                self.wanted = None;
            }
            self.selected = None;
            self.body = NoteBody::None;
        }
        self.notes = notes;
    }

    /// Select a note and ask the app to read its body. Pressing the selected row
    /// again is a no-op, so it does not re-read the file.
    pub fn select_note(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.selected = Some(id.to_owned());
        self.body = NoteBody::Loading;
        self.wanted = Some(id.to_owned());
    }

    /// The note the app should read, if any. Called from `poll`, never the view.
    pub fn take_wanted(&mut self) -> Option<String> {
        self.wanted.take()
    }

    /// A body arrived. A stale one (the selection moved on) is dropped.
    pub fn show_body(&mut self, id: &str, body: String) {
        if self.selected.as_deref() == Some(id) {
            self.body = NoteBody::Ready(body);
        }
    }

    pub fn fail_body(&mut self, id: &str, error: String) {
        if self.selected.as_deref() == Some(id) {
            self.body = NoteBody::Failed(error);
        }
    }

    /// `init` for the app: `--vault DIR` and `--route NAME` from the command line,
    /// with the rest of Rune's flags left to Rune.
    pub fn from_args() -> Self {
        let args = rune::extra_args();
        let mut state = Self::new();
        if let Some(vault) = arg(args, "vault") {
            state.vault = vault;
        }
        if let Some(route) = arg(args, "route").and_then(|name| Route::parse(&name)) {
            state.route = route;
        }
        state
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// `~/Documents/Notes`. Resolved here, not in the view, and only once.
fn default_vault() -> String {
    let home = std::env::var_os("HOME").unwrap_or_else(|| OsString::from("."));
    std::path::PathBuf::from(home)
        .join("Documents")
        .join("Notes")
        .to_string_lossy()
        .into_owned()
}

/// The value of `--name value` or `--name=value` among Rune's extra arguments.
pub fn arg(args: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    if let Some(index) = args.iter().position(|a| a == &flag) {
        return args.get(index + 1).cloned();
    }
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{flag}=")).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_vault_is_under_documents() {
        let vault = default_vault();
        assert!(vault.ends_with("Documents/Notes"), "{vault}");
        assert!(vault.starts_with('/'), "an absolute path: {vault}");
    }

    #[test]
    fn flags_are_read_in_both_spellings() {
        let spaced = vec!["--vault".to_owned(), "/tmp/v".to_owned()];
        let equals = vec!["--vault=/tmp/w".to_owned()];
        assert_eq!(arg(&spaced, "vault").as_deref(), Some("/tmp/v"));
        assert_eq!(arg(&equals, "vault").as_deref(), Some("/tmp/w"));
        assert_eq!(arg(&[], "vault"), None);
    }

    #[test]
    fn panes_start_open_at_sensible_widths() {
        let state = State::new();
        assert!(state.sidebar.is_open());
        assert!(state.detail.is_open());
        assert!(state.sidebar.width() > 180.0);
        assert!(state.detail.width() > 320.0);
    }
}
