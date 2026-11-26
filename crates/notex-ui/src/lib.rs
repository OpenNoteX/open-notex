//! The openNoteX shell.
//!
//! Everything in this crate is a pure function of [`State`]: `view()` reads
//! state and returns a tree. No database, no filesystem, no clock, no
//! navigation history. That is what makes every screen renderable headlessly.

pub mod home;
pub mod icons;
pub mod routes;
pub mod shell;
pub mod state;
pub mod tasks;
pub mod theme;

pub use routes::{Layout, Route};
pub use shell::{on_key, view};
pub use state::{
    Action, Draft, DraftStatus, Load, NoteBody, NoteRow, Now, ProjectRow, SaveRequest, State,
    TaskRow, Undo, Update, VaultIntent, VaultSetup, VaultStatus, arg,
};

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

/// A fixed clock, so a snapshot never depends on when it was taken.
fn now() -> Now {
    Now {
        day: "2026-11-19".to_owned(),
        time: "09:41".to_owned(),
        date: "Thursday 19 November".to_owned(),
    }
}

fn project(id: &str, name: &str, color: &str) -> ProjectRow {
    ProjectRow {
        id: id.to_owned(),
        name: name.to_owned(),
        color: Some(color.to_owned()),
    }
}

#[allow(clippy::too_many_arguments)]
fn task(
    id: &str,
    title: &str,
    due_day: Option<&str>,
    project: Option<(&str, &str, &str)>,
    done: bool,
    completed_day: Option<&str>,
) -> TaskRow {
    let (list_id, list_name, list_color) = match project {
        Some((id, name, color)) => (
            Some(id.to_owned()),
            Some(name.to_owned()),
            Some(color.to_owned()),
        ),
        None => (None, None, None),
    };
    TaskRow {
        id: id.to_owned(),
        title: title.to_owned(),
        notes: String::new(),
        done,
        list_id,
        list_name,
        list_color,
        due_day: due_day.map(str::to_owned),
        completed_at: completed_day.map(|_| 1_763_500_800_000),
        completed_day: completed_day.map(str::to_owned),
        parent_id: None,
    }
}

/// The data every task scene starts from: three projects, a spread of due days
/// and one archived task.
fn seed_tasks(s: &mut State) {
    s.now = now();
    s.set_projects(vec![
        project("01L1", "Launch", "#3f8f6b"),
        project("01L2", "Personal", "#c2703f"),
        project("01L3", "Studio", "#5a6fb0"),
    ]);
    let mut tasks = vec![
        task(
            "01T1",
            "Send invites",
            Some("2026-11-17"),
            Some(("01L1", "Launch", "#3f8f6b")),
            false,
            None,
        ),
        task(
            "01T2",
            "Confirm the venue",
            Some("2026-11-18"),
            Some(("01L1", "Launch", "#3f8f6b")),
            false,
            None,
        ),
        task(
            "01T3",
            "Outline the launch note",
            Some("2026-11-19"),
            Some(("01L3", "Studio", "#5a6fb0")),
            false,
            None,
        ),
        task(
            "01T4",
            "Review the copy",
            Some("2026-11-19"),
            None,
            false,
            None,
        ),
        task(
            "01T5",
            "Book the room",
            Some("2026-11-22"),
            Some(("01L2", "Personal", "#c2703f")),
            false,
            None,
        ),
        task("01T6", "Read the Rune notes", None, None, false, None),
        task(
            "01T7",
            "Set up the vault",
            None,
            Some(("01L1", "Launch", "#3f8f6b")),
            true,
            Some("2026-11-18"),
        ),
        task(
            "01T8",
            "Sketch the empty states",
            None,
            Some(("01L3", "Studio", "#5a6fb0")),
            true,
            Some("2026-11-19"),
        ),
    ];
    if let Some(task) = tasks.iter_mut().find(|task| task.id == "01T3") {
        task.notes = "Bring the printed outline.".to_owned();
    }
    s.set_tasks(tasks);
    s.set_notes(vec![
        NoteRow::new("01J8Z5M4Q3", "Launch checklist", &["launch", "work"])
            .modified(1_763_600_000_000_000_000),
        NoteRow::new("01J8Z5M4R7", "Reading list", &["books"]).modified(1_763_500_000_000_000_000),
        NoteRow::new(WEEKLY_ID, "Weekly review", &["review", "work"])
            .modified(1_763_400_000_000_000_000),
    ]);
}

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
    ("home", |s| {
        seed_tasks(s);
        s.route = Route::Home;
        s.load = Load::Ready;
    }),
    ("home-empty", |s| {
        s.now = now();
        s.route = Route::Home;
        s.load = Load::Ready;
    }),
    ("tasks", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.load = Load::Ready;
    }),
    ("tasks-empty", |s| {
        s.now = now();
        s.route = Route::Tasks;
        s.load = Load::Ready;
    }),
    ("tasks-filtered", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.set_project_filter(Some("01L1".to_owned()));
        s.load = Load::Ready;
    }),
    ("tasks-add-pending", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        // The row is here before the store's answer: the point of the scene.
        s.add_task.replace_text("Send the invitations #Launch");
        s.add_task_now();
        s.load = Load::Ready;
    }),
    ("task-expanded", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.toggle_expanded("01T3");
        s.load = Load::Ready;
    }),
    ("tasks-steps", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        // A task broken into steps, one done and one broken down again, with a
        // field open under a step: the tree in one frame.
        for (id, title, done, parent) in [
            ("01S1", "Draft the outline", true, "01T3"),
            ("01S2", "Add the screenshots", false, "01T3"),
            ("01S3", "Ask for a review", false, "01T3"),
            ("01S4", "Crop the hero", false, "01S2"),
        ] {
            let mut step = task(
                id,
                title,
                None,
                Some(("01L3", "Studio", "#5a6fb0")),
                done,
                done.then_some("2026-11-19"),
            );
            step.parent_id = Some(parent.to_owned());
            s.task_changed(step);
        }
        s.begin_step("01T3");
        s.load = Load::Ready;
    }),
    ("tasks-undo", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.toggle_task("01T4");
        s.load = Load::Ready;
    }),
    ("tasks-project-menu", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.open_chip_menu("01L1", None);
        s.load = Load::Ready;
    }),
    ("tasks-project-rename", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.begin_project_edit("01L1");
        s.load = Load::Ready;
    }),
    ("tasks-project-undo", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.delete_project("01L1");
        // The store's answer, in the same frame as the toast.
        s.project_gone("01L1");
        s.load = Load::Ready;
    }),
    ("archive", |s| {
        seed_tasks(s);
        s.route = Route::Tasks;
        s.archive = true;
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
    ("settings", |s| {
        s.route = Route::Settings;
    }),
    ("settings-update", |s| {
        s.route = Route::Settings;
        s.load = Load::Ready;
        s.update_available("0.1.0");
    }),
    ("onboarding", |s| {
        s.route = Route::Home;
        s.ask_for_vault("/Users/you/Documents/Notes", None);
    }),
    ("notes-no-vault", |s| {
        s.route = Route::Notes;
        s.ask_for_vault("/Users/you/Documents/Notes", None);
        s.close_vault_dialog();
    }),
    ("settings-no-vault", |s| {
        s.route = Route::Settings;
        s.ask_for_vault("/Users/you/Documents/Notes", None);
        s.close_vault_dialog();
    }),
    ("search", |s| {
        s.route = Route::Search;
        s.load = Load::Ready;
    }),
    ("shell-loading", |s| {
        s.route = Route::Tasks;
        s.load = Load::Loading;
    }),
    ("shell-error", |s| {
        s.route = Route::Tasks;
        s.load = Load::Failed("The vault at /Users/you/Documents/Notes is missing.".to_owned());
    }),
];
