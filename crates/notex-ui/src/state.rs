//! The window's state.
//!
//! `view()` is a pure function of this struct: no database, no filesystem, no
//! clock. Everything here is cheap to clone and cheap to compare, because Rune
//! re-runs the view on every event.

use std::ffi::OsString;

use rune::TextEdit;
use rune_kit::shell::PaneState;

use crate::routes::Route;

/// How long the editor waits after the last keystroke before saving.
pub const AUTOSAVE_IDLE: f64 = 0.7;
/// The longest a continuous run of typing goes unsaved.
pub const AUTOSAVE_MAX: f64 = 5.0;

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
///
/// `Ready` means the editor has the body: [`State::draft`] owns the text, so it
/// is never stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NoteBody {
    #[default]
    None,
    Loading,
    Ready,
    Failed(String),
}

/// The open note's editor. One per selected note; dropped when the note closes.
pub struct Draft {
    pub id: String,
    /// The file's whole Markdown source, frontmatter excluded. `# Title` is part
    /// of it, so editing the heading is how a note is renamed.
    pub edit: TextEdit,
    /// The file hash the text was read from. A different file on disk is a
    /// conflict, not something to overwrite.
    pub base_hash: u64,
    /// Why the last save was refused because the file changed underneath.
    pub conflict: Option<String>,
    /// Why the last save failed (a read-only vault, a full disk).
    pub error: Option<String>,
    /// Bumped on every user edit, echoed by the worker on save.
    token: u64,
    /// The highest token the worker has confirmed. Dirty is `token > acked`.
    acked: u64,
    /// Seconds since the last keystroke, and since the last save.
    idle: f64,
    since_save: f64,
    /// A save is out; no second one goes until it answers.
    saving: bool,
    /// The next save overwrites whatever is on disk (the user chose "Overwrite").
    force: bool,
}

/// What the editor's status line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftStatus {
    Conflict,
    Failed,
    Saving,
    Edited,
    Saved,
}

impl Draft {
    pub fn new(id: &str, body: String, base_hash: u64) -> Self {
        Self {
            id: id.to_owned(),
            edit: TextEdit::new(body),
            base_hash,
            conflict: None,
            error: None,
            token: 0,
            acked: 0,
            idle: AUTOSAVE_IDLE,
            since_save: 0.0,
            saving: false,
            force: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.token > self.acked
    }

    pub fn status(&self) -> DraftStatus {
        if self.conflict.is_some() {
            DraftStatus::Conflict
        } else if self.error.is_some() {
            DraftStatus::Failed
        } else if self.saving {
            DraftStatus::Saving
        } else if self.is_dirty() {
            DraftStatus::Edited
        } else {
            DraftStatus::Saved
        }
    }
}

/// A save the app should send. The view never sends it; `poll` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub id: String,
    pub body: String,
    pub base_hash: u64,
    pub token: u64,
    pub force: bool,
}

/// What the app should do next. Drained by `poll`, never by the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Save(SaveRequest),
    Open(String),
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
    /// The open note's editor, once the body has arrived.
    pub draft: Option<Draft>,
    /// A row the user picked while the editor had unsaved text. Opened after the
    /// save lands, so typing is never thrown away.
    pub pending_open: Option<String>,
    /// `⌘S` or the Save button asked for a save now.
    save_requested: bool,
    /// The last `poll` clock, for `tick`. Seconds since the app started.
    clock: f64,
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
            draft: None,
            pending_open: None,
            save_requested: false,
            clock: 0.0,
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
            self.selected = None;
            self.body = NoteBody::None;
            self.draft = None;
            self.pending_open = None;
        }
        self.notes = notes;
    }

    /// Select a note. If the editor has unsaved text, the switch waits for the
    /// save: the open is queued and the old draft stays on screen until then.
    pub fn select_note(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        if self.draft.as_ref().is_some_and(Draft::is_dirty) {
            self.pending_open = Some(id.to_owned());
            return;
        }
        self.open_now(id);
    }

    fn open_now(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
        self.body = NoteBody::Loading;
        self.draft = None;
        self.pending_open = Some(id.to_owned());
    }

    /// A body arrived. A stale one (the selection moved on) is dropped.
    pub fn show_body(&mut self, id: &str, body: impl Into<String>, hash: u64) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        self.pending_open = None;
        self.body = NoteBody::Ready;
        self.draft = Some(Draft::new(id, body.into(), hash));
    }

    pub fn fail_body(&mut self, id: &str, error: String) {
        if self.selected.as_deref() == Some(id) {
            self.pending_open = None;
            self.draft = None;
            self.body = NoteBody::Failed(error);
        }
    }

    /// The editor's text changed. Called from the field's `on_change` handler.
    /// The token moves so a save that is already in flight cannot mark newer
    /// text clean.
    pub fn draft_touched(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.token += 1;
            draft.idle = 0.0;
        }
    }

    /// `⌘S`, the Save button, and the close hook.
    pub fn request_save(&mut self) {
        self.save_requested = true;
    }

    /// After a conflict: write our draft over the file on disk.
    pub fn request_overwrite(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.conflict = None;
            draft.error = None;
            draft.force = true;
        }
        self.save_requested = true;
    }

    /// After a conflict: drop the draft and read the file again.
    pub fn request_reload(&mut self) {
        if let Some(draft) = &self.draft {
            let id = draft.id.clone();
            self.open_now(&id);
        }
    }

    /// A save landed. `token` is the edit revision that was written, so text
    /// typed while the save was in flight stays dirty.
    pub fn saved(&mut self, id: &str, token: u64, hash: u64, title: String, tags: Vec<String>) {
        if let Some(row) = self.notes.iter_mut().find(|note| note.id == id) {
            row.title = title;
            row.tags = tags;
        }
        self.sort_notes();

        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.base_hash = hash;
            draft.acked = draft.acked.max(token);
            draft.saving = false;
            draft.conflict = None;
            draft.error = None;
            draft.force = false;
            draft.since_save = 0.0;
        }
    }

    /// The file changed on disk since the draft was read.
    pub fn save_conflict(&mut self, id: &str, message: impl Into<String>) {
        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.saving = false;
            draft.conflict = Some(message.into());
        }
    }

    pub fn save_failed(&mut self, id: &str, error: impl Into<String>) {
        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.saving = false;
            draft.error = Some(error.into());
        }
    }

    /// Advance the editor's clocks. `now` is `poll`'s clock: seconds since the
    /// app started. Returns true when a save is now due, so `poll` draws a frame
    /// and schedules the next wake.
    pub fn tick(&mut self, now: f64) -> bool {
        let dt = (now - self.clock).max(0.0);
        self.clock = now;
        if let Some(draft) = &mut self.draft {
            draft.idle += dt;
            draft.since_save += dt;
        }
        self.next_wake().is_some_and(|wait| wait <= 0.0)
    }

    /// How long `poll` should sleep before asking for a save, if the draft is
    /// dirty and nothing is blocking it.
    pub fn next_wake(&self) -> Option<f64> {
        let draft = self.draft.as_ref()?;
        if !draft.is_dirty() || draft.saving || draft.conflict.is_some() || draft.error.is_some() {
            return None;
        }
        Some(
            (AUTOSAVE_IDLE - draft.idle)
                .min(AUTOSAVE_MAX - draft.since_save)
                .max(0.0),
        )
    }

    /// The next command the app should send. Called from `poll` after `tick`.
    pub fn take_action(&mut self) -> Option<Action> {
        if let Some(draft) = &mut self.draft
            && draft.conflict.is_none()
            && draft.error.is_none()
            && draft.is_dirty()
            && !draft.saving
        {
            let due = draft.idle >= AUTOSAVE_IDLE || draft.since_save >= AUTOSAVE_MAX;
            if due || std::mem::take(&mut self.save_requested) {
                draft.saving = true;
                draft.idle = 0.0;
                draft.since_save = 0.0;
                return Some(Action::Save(SaveRequest {
                    id: draft.id.clone(),
                    body: draft.edit.text.clone(),
                    base_hash: draft.base_hash,
                    token: draft.token,
                    force: draft.force,
                }));
            }
        }
        self.save_requested = false;

        if self.draft.as_ref().is_none_or(|draft| !draft.is_dirty())
            && let Some(id) = self.pending_open.take()
        {
            // The switch is now safe: the old note is saved or untouched.
            self.selected = Some(id.clone());
            self.body = NoteBody::Loading;
            self.draft = None;
            return Some(Action::Open(id));
        }
        None
    }

    fn sort_notes(&mut self) {
        self.notes.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
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

    fn state_with_two_notes() -> State {
        let mut state = State::new();
        state.set_notes(vec![
            NoteRow::new("01A", "Alpha", &["work"]),
            NoteRow::new("01B", "Beta", &[]),
        ]);
        state
    }

    fn open_alpha(state: &mut State) {
        state.select_note("01A");
        assert_eq!(state.take_action(), Some(Action::Open("01A".to_owned())));
        state.show_body("01A", "# Alpha\n\nold\n", 11);
    }

    fn edit(state: &mut State, text: &str) {
        state.draft.as_mut().unwrap().edit.replace_text(text);
        state.draft_touched();
    }

    fn next_save(state: &mut State) -> SaveRequest {
        state.tick(state.clock + 0.8);
        match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected a save, got {other:?}"),
        }
    }

    #[test]
    fn the_state_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<State>();
        assert_send_sync::<Draft>();
    }

    #[test]
    fn an_edit_saves_the_whole_body_after_the_idle_delay() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nnew\n");
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Edited);
        assert!(state.take_action().is_none(), "not due yet");

        let request = next_save(&mut state);
        assert_eq!(request.id, "01A");
        assert_eq!(request.body, "# Alpha\n\nnew\n");
        assert_eq!(request.base_hash, 11);
        assert_eq!(request.token, 1, "the save carries the edit revision");
        assert!(!request.force);
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Saving);

        // Nothing more goes out while that save is in flight.
        state.tick(state.clock + 10.0);
        assert!(state.take_action().is_none());
    }

    #[test]
    fn typing_during_a_save_keeps_the_draft_dirty() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nfirst\n");
        let first = next_save(&mut state);

        edit(&mut state, "# Alpha\n\nsecond\n");
        state.saved("01A", first.token, 22, "Alpha".to_owned(), vec![]);

        assert!(
            state.draft.as_ref().unwrap().is_dirty(),
            "the ack is older than the text"
        );
        let second = next_save(&mut state);
        assert_eq!(second.body, "# Alpha\n\nsecond\n");
        assert_eq!(second.base_hash, 22, "the saved file is the new base");

        state.saved("01A", second.token, 33, "Alpha".to_owned(), vec![]);
        assert!(!state.draft.as_ref().unwrap().is_dirty());
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Saved);
    }

    #[test]
    fn a_dirty_switch_waits_for_the_save_then_opens() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nchanged\n");

        state.select_note("01B");
        assert_eq!(state.selected.as_deref(), Some("01A"), "still on Alpha");
        assert_eq!(state.pending_open.as_deref(), Some("01B"));

        state.request_save();
        let save = match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected the flush, got {other:?}"),
        };
        assert!(state.take_action().is_none(), "the open waits for the ack");

        state.saved("01A", save.token, 44, "Alpha".to_owned(), vec![]);
        assert_eq!(
            state.take_action(),
            Some(Action::Open("01B".to_owned())),
            "the save landed, so the switch can happen"
        );
        assert_eq!(state.selected.as_deref(), Some("01B"));
        assert!(state.draft.is_none());
        assert_eq!(state.body, NoteBody::Loading);
    }

    #[test]
    fn a_clean_switch_opens_immediately() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        state.select_note("01B");
        assert_eq!(state.take_action(), Some(Action::Open("01B".to_owned())));
    }

    #[test]
    fn a_conflict_blocks_autosave_until_the_user_chooses() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        next_save(&mut state);
        state.save_conflict("01A", "changed on disk");
        assert_eq!(
            state.draft.as_ref().unwrap().status(),
            DraftStatus::Conflict
        );

        state.tick(state.clock + 20.0);
        assert!(state.take_action().is_none(), "a conflict never retries");

        state.request_overwrite();
        let request = match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected the overwrite, got {other:?}"),
        };
        assert!(request.force);
        assert_eq!(request.body, "# Alpha\n\nmine\n");
    }

    #[test]
    fn reloading_after_a_conflict_drops_the_draft_and_reads_again() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        state.save_conflict("01A", "changed on disk");

        state.request_reload();
        assert_eq!(state.take_action(), Some(Action::Open("01A".to_owned())));
        assert!(state.draft.is_none());
        assert_eq!(state.body, NoteBody::Loading);
    }

    #[test]
    fn a_saved_title_updates_the_row_and_the_order() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Zed\n\nold\n");
        let save = next_save(&mut state);
        state.saved(
            "01A",
            save.token,
            55,
            "Zed".to_owned(),
            vec!["work".to_owned()],
        );

        assert_eq!(state.notes[0].title, "Beta", "Beta sorts before Zed");
        assert_eq!(state.notes[1].title, "Zed");
        assert_eq!(state.notes[1].tags, vec!["work".to_owned()]);
    }

    #[test]
    fn a_missing_note_drops_its_draft() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nlost\n");

        state.set_notes(vec![NoteRow::new("01B", "Beta", &[])]);
        assert!(state.draft.is_none());
        assert_eq!(state.selected, None);
        assert_eq!(state.body, NoteBody::None);
    }

    #[test]
    fn a_late_body_for_another_note_is_dropped() {
        let mut state = state_with_two_notes();
        state.select_note("01A");
        state.select_note("01B");
        state.show_body("01A", "# Alpha\n\nstale\n", 1);
        assert!(state.draft.is_none(), "the selection had moved on");
    }
}
