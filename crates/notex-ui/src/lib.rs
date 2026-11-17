//! The openNoteX shell.
//!
//! Everything in this crate is a pure function of [`State`]: `view()` reads
//! state and returns a tree. No database, no filesystem, no clock, no
//! navigation history. That is what makes every screen renderable headlessly.

pub mod icons;
pub mod routes;
pub mod shell;
pub mod state;
pub mod theme;

pub use routes::Route;
pub use shell::{on_key, view};
pub use state::{Action, Draft, DraftStatus, Load, NoteBody, NoteRow, SaveRequest, State};

pub const TITLE: &str = "openNoteX";
pub const SIZE: (f32, f32) = (1120.0, 740.0);
pub const MIN_SIZE: (f32, f32) = (880.0, 560.0);

/// One named state: the name `--scene` takes, and the change it makes.
pub type Scene = (&'static str, fn(&mut State));

/// The scene's note: the Markdown a person would see in the editor.
const WEEKLY: &str = concat!(
    "# Weekly review\n\n",
    "A quiet week. The shell landed, and notes are files again.\n\n",
    "## Done\n\n",
    "- [x] Rune shell\n",
    "- [x] Store worker\n",
    "- [ ] Markdown preview\n\n",
    "## Next\n\n",
    "1. Watch the vault\n",
    "2. Add search\n\n",
    "> The vault stays readable in any editor.\n",
);

const WEEKLY_ID: &str = "01J8Z5M4S1";

/// The Notes screen with one note open in the editor.
fn weekly_review(s: &mut State) {
    s.route = Route::Notes;
    s.load = Load::Ready;
    s.set_notes(vec![
        NoteRow::new("01J8Z5M4Q3", "Launch checklist", &["launch", "work"]),
        NoteRow::new("01J8Z5M4R7", "Reading list", &["books"]),
        NoteRow::new(WEEKLY_ID, "Weekly review", &["review", "work"]),
    ]);
    s.select_note(WEEKLY_ID);
    s.show_body(WEEKLY_ID, WEEKLY.to_owned(), 42);
}

/// Named states for `--scene`, used by the snapshot harness and the tests.
/// Every scene starts from `init()` (which reads `--vault`) and then changes one
/// thing, so a snapshot never depends on unstated state.
pub const SCENES: &[Scene] = &[
    ("shell-today", |s| {
        s.route = Route::Today;
        s.load = Load::Ready;
    }),
    ("shell-empty", |s| {
        s.route = Route::Inbox;
        s.load = Load::Ready;
    }),
    ("notes-empty", |s| {
        s.route = Route::Notes;
        s.load = Load::Ready;
    }),
    ("notes-list", weekly_review),
    ("notes-edit", |s| {
        weekly_review(s);
        s.draft_touched();
        s.draft.as_mut().unwrap().edit.replace_text(concat!(
            "# Weekly review\n\n",
            "A quiet week. The shell landed, and notes are files again.\n\n",
            "## Done\n\n",
            "- [x] Rune shell\n",
            "- [x] Store worker\n",
            "- [x] Markdown preview\n\n",
            "## Next\n\n",
            "1. Watch the vault\n",
            "2. Add search\n\n",
            "> The vault stays readable in any editor.\n",
        ));
    }),
    ("notes-conflict", |s| {
        weekly_review(s);
        s.draft_touched();
        s.save_conflict(WEEKLY_ID, "This file changed on disk since you opened it.");
    }),
    ("notes-delete", |s| {
        weekly_review(s);
        s.request_delete();
    }),
    ("projects-empty", |s| {
        s.route = Route::Projects;
        s.load = Load::Ready;
    }),
    ("settings", |s| {
        s.route = Route::Settings;
    }),
    ("search", |s| {
        s.route = Route::Search;
        s.load = Load::Ready;
    }),
    ("shell-loading", |s| {
        s.route = Route::Today;
        s.load = Load::Loading;
    }),
    ("shell-error", |s| {
        s.route = Route::Today;
        s.load = Load::Failed("The vault at /Users/you/Documents/Notes is missing.".to_owned());
    }),
];
