//! The tasks flow, end to end: the real worker, the real SQLite file and the
//! real element tree. Only the GPU window is missing.
//!
//! These cover the wiring `poll` does — a line typed in the add row becomes a
//! task row, a due day is a local date, completing moves it to the archive, and
//! a second launch reads the same rows back from the database.

use std::fs;
use std::time::{Duration, Instant};

use notex_store::Notify;
use notex_ui::{Load, Route, State, view};
use open_notex::{Session, pump, start};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

const SIZE: (f32, f32) = (1120.0, 740.0);
const TODAY: &str = "2026-11-19";

struct Quiet;

impl Notify for Quiet {
    fn notify(&self) {}
}

fn runtime(state: State) -> Runtime<State> {
    let mut rt = Runtime::new(state, view, SIZE, Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}

fn click(rt: &mut Runtime<State>, key: &str) {
    settle_layout(rt);
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

fn settle_layout(rt: &mut Runtime<State>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

/// Pump the worker until `done` holds, drawing a frame after each change.
fn settle(rt: &mut Runtime<State>, session: &Session, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let changed = pump(&mut rt.state, session.handle());
        if changed {
            rt.invalidate();
            rt.frame(1.0 / 60.0);
        }
        if done(&rt.state) {
            rt.invalidate();
            rt.frame(1.0 / 60.0);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "{what} never finished: load={:?} tasks={:?} projects={:?} error={:?}",
        rt.state.load, rt.state.tasks, rt.state.projects, rt.state.task_error
    );
}

fn all_text(rt: &Runtime<State>) -> String {
    rt.elements()
        .into_iter()
        .filter_map(|element| element.text.or(element.label))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A temp vault and database, a window on Tasks, and the store running.
fn open_tasks() -> (Runtime<State>, Session, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir(&vault).unwrap();
    let db = dir.path().join("notex.db");

    let mut state = State::with_vault(vault.display().to_string());
    state.route = Route::Tasks;
    state.set_clock(notex_ui::Now {
        day: TODAY.to_owned(),
        time: "09:41".to_owned(),
        date: "Thursday 19 November".to_owned(),
    });
    let session = start(&mut state, &vault, &db, Quiet).expect("the store starts");
    let mut rt = runtime(state);
    settle(&mut rt, &session, "the first frame", |state| {
        state.load == Load::Ready
    });
    (rt, session, dir)
}

/// Type a line and wait for the store's answer. The row is on screen before it,
/// and `pending_adds` empties when the real id has replaced it, so this leaves
/// the test with a settled checklist and a real id to act on.
fn add_line(rt: &mut Runtime<State>, session: &Session, text: &str) {
    rt.state.add_task.replace_text(text);
    rt.state.add_task_now();
    settle(rt, session, "the created task", |state| {
        state.pending_adds.is_empty()
    });
}

#[test]
fn a_typed_line_is_a_row_before_the_worker_answers() {
    let (mut rt, session, _dir) = open_tasks();
    settle_layout(&mut rt);

    click(&mut rt, "task-add-input");
    rt.paste("Send invites");
    rt.frame(1.0 / 60.0);
    rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);

    // Nothing has been pumped: the row and the wake are the app's own answer.
    assert!(
        rt.state.pending_adds.len() == 1,
        "the create is on its way to the worker"
    );
    assert_eq!(
        rt.state
            .pending_adds
            .front()
            .map(|add| add.temp_id.as_str()),
        Some("new-0")
    );
    assert!(
        rt.find("task-row-new-0").is_some(),
        "the typed task is on screen before the store answers"
    );
    let deadline = rt
        .wake_deadline()
        .expect("the queued create asks for a frame");
    assert!(
        deadline <= rt.clock,
        "the wake is due now, not at {deadline} with the clock at {}",
        rt.clock
    );

    settle(&mut rt, &session, "the created task", |state| {
        state.tasks.len() == 1 && state.pending_adds.is_empty()
    });

    let task = &rt.state.tasks[0];
    assert_eq!(task.title, "Send invites");
    assert!(
        !task.id.starts_with("new-"),
        "the real id replaced the temporary one"
    );
    assert!(rt.find(&format!("task-row-{}", task.id)).is_some());
    assert!(
        rt.find("task-row-new-0").is_none(),
        "the optimistic row did not stay behind"
    );
}

#[test]
fn a_line_in_the_add_row_becomes_a_row_in_the_checklist() {
    let (mut rt, session, _dir) = open_tasks();

    add_line(&mut rt, &session, "Send invites #Launch");
    settle(&mut rt, &session, "the new task", |state| {
        !state.tasks.is_empty()
    });

    let task = &rt.state.tasks[0];
    assert_eq!(task.title, "Send invites");
    assert_eq!(
        task.list_name.as_deref(),
        Some("Launch"),
        "the project was made"
    );
    assert_eq!(rt.state.projects.len(), 1);
    assert_eq!(rt.state.projects[0].name, "Launch");
    assert!(task.notes.is_empty());

    // The row is on screen with its project chip, in the No date group.
    assert!(rt.find(&format!("task-row-{}", task.id)).is_some());
    assert!(rt.find(&format!("task-project-{}", task.id)).is_some());
    assert!(rt.find("task-group-someday").is_some());
    assert!(all_text(&rt).contains("Send invites"));
}

#[test]
fn a_second_line_for_the_same_project_reuses_it() {
    let (mut rt, session, _dir) = open_tasks();

    add_line(&mut rt, &session, "Send invites #Launch");
    settle(&mut rt, &session, "the first task", |state| {
        state.tasks.len() == 1
    });
    add_line(&mut rt, &session, "Book the room #launch");
    settle(&mut rt, &session, "the second task", |state| {
        state.tasks.len() == 2
    });

    assert_eq!(rt.state.projects.len(), 1, "one project, whatever the case");
    let project = &rt.state.projects[0];
    assert_eq!(project.id, rt.state.tasks[0].list_id.clone().unwrap());
    assert_eq!(project.id, rt.state.tasks[1].list_id.clone().unwrap());
}

#[test]
fn a_due_day_is_a_local_date_and_the_group_follows_it() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Send invites");
    settle(&mut rt, &session, "the task", |state| {
        state.tasks.len() == 1
    });
    let id = rt.state.tasks[0].id.clone();

    rt.state.set_due(&id, Some("2026-11-17".to_owned()));
    settle(&mut rt, &session, "the due day", |state| {
        state.tasks[0].due_day.is_some()
    });

    assert_eq!(rt.state.tasks[0].due_day.as_deref(), Some("2026-11-17"));
    assert!(
        rt.find("task-group-overdue").is_some(),
        "yesterday is overdue"
    );
    assert_eq!(
        rt.elements()
            .into_iter()
            .filter_map(|e| e.text)
            .find(|text| text == "2d late"),
        Some("2d late".to_owned())
    );

    rt.state.set_due(&id, None);
    settle(&mut rt, &session, "the cleared day", |state| {
        state.tasks[0].due_day.is_none()
    });
    assert!(rt.find("task-group-overdue").is_none());
}

#[test]
fn completing_a_task_moves_it_to_the_archive_and_a_restart_agrees() {
    let dir = {
        let (mut rt, session, dir) = open_tasks();
        add_line(&mut rt, &session, "Send invites");
        settle(&mut rt, &session, "the task", |state| {
            state.tasks.len() == 1
        });
        let id = rt.state.tasks[0].id.clone();

        click(&mut rt, &format!("task-check-{id}"));
        assert!(rt.find("task-undo").is_some(), "the undo toast is up");
        settle(&mut rt, &session, "the completion", |state| {
            state.tasks[0].done
        });
        assert!(rt.state.open_task_count() == 0);

        // The archive groups it under the day it was completed, on screen.
        click(&mut rt, "tasks-archive");
        settle_layout(&mut rt);
        assert!(rt.find(&format!("archive-row-{id}")).is_some());
        assert!(
            rt.find("task-group-someday").is_none(),
            "the archive is not the checklist"
        );
        dir
    };

    // A second launch, same database: the done task is still there, archived.
    let vault = dir.path().join("vault");
    let db = dir.path().join("notex.db");
    let mut state = State::with_vault(vault.display().to_string());
    state.route = Route::Tasks;
    let session = start(&mut state, &vault, &db, Quiet).expect("the store starts again");
    let mut rt = runtime(state);
    settle(&mut rt, &session, "the reloaded tasks", |state| {
        !state.tasks.is_empty()
    });
    assert!(
        rt.state.tasks[0].done,
        "the completion survived the restart"
    );
    assert_eq!(rt.state.open_task_count(), 0);
    assert_eq!(rt.state.tasks[0].title, "Send invites");
}

#[test]
fn a_deleted_task_returns_when_the_toast_is_used_and_stays_gone_when_it_is_not() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Send invites");
    add_line(&mut rt, &session, "Book the room");
    settle(&mut rt, &session, "two tasks", |state| {
        state.tasks.len() == 2
    });
    let deleted = rt.state.tasks[0].id.clone();

    rt.state.delete_task(&deleted);
    settle(&mut rt, &session, "the delete", |state| {
        !state.tasks.iter().any(|task| task.id == deleted)
    });

    rt.state.undo_task();
    settle(&mut rt, &session, "the restore", |state| {
        state.tasks.iter().any(|task| task.id == deleted)
    });
    assert_eq!(rt.state.open_task_count(), 2);

    // Delete again and let the toast pass: only the one row goes.
    rt.state.delete_task(&deleted);
    settle(&mut rt, &session, "the second delete", |state| {
        !state.tasks.iter().any(|task| task.id == deleted)
    });
    rt.state.dismiss_undo();
    settle_layout(&mut rt);
    assert_eq!(rt.state.open_task_count(), 1);
}

#[test]
fn renaming_and_moving_a_task_both_survive_a_relist() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Send invites #Launch");
    settle(&mut rt, &session, "the task", |state| {
        state.tasks.len() == 1
    });
    let id = rt.state.tasks[0].id.clone();

    rt.state.begin_edit(&id);
    rt.state
        .task_edit
        .as_mut()
        .unwrap()
        .edit
        .replace_text("Send the invites");
    rt.state.commit_edit();
    settle(&mut rt, &session, "the rename", |state| {
        state.tasks[0].title == "Send the invites"
    });

    rt.state.set_project(&id, None);
    settle(&mut rt, &session, "the unfiling", |state| {
        state.tasks[0].list_id.is_none()
    });

    assert_eq!(rt.state.tasks[0].title, "Send the invites");
    assert_eq!(rt.state.tasks[0].list_id, None);
    // The project itself stays: deleting a task never deletes its project.
    assert_eq!(rt.state.projects.len(), 1);
}

#[test]
fn a_task_note_is_saved_and_comes_back_on_the_next_open() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Send invites");
    settle(&mut rt, &session, "the task", |state| {
        state.tasks.len() == 1
    });
    let id = rt.state.tasks[0].id.clone();

    rt.state.toggle_expanded(&id);
    rt.state
        .task_note
        .as_mut()
        .unwrap()
        .edit
        .replace_text("Ask about the ramp.");
    rt.state.save_task_note();
    settle(&mut rt, &session, "the note", |state| {
        state.tasks[0].notes == "Ask about the ramp."
    });

    rt.state.toggle_expanded(&id);
    rt.state.toggle_expanded(&id);
    assert_eq!(
        rt.state.task_note.as_ref().unwrap().edit.text,
        "Ask about the ramp.",
        "reopening the row reads the saved notes"
    );
    assert!(rt.find(&format!("task-notes-{id}")).is_some());
}

#[test]
fn a_renamed_project_renames_its_tasks_filing_and_a_delete_keeps_the_tasks() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Send invites #Launch");
    settle(&mut rt, &session, "the task", |state| {
        state.tasks.len() == 1
    });
    let project = rt.state.projects[0].id.clone();

    rt.state.begin_project_edit(&project);
    rt.state
        .project_edit
        .as_mut()
        .unwrap()
        .edit
        .replace_text("Launch party");
    rt.state.commit_project_edit();
    settle(&mut rt, &session, "the rename", |state| {
        state.projects[0].name == "Launch party"
    });
    assert_eq!(
        rt.state.tasks[0].list_name.as_deref(),
        Some("Launch party"),
        "the task reads its project through the join"
    );
    assert!(
        rt.state.project_edit.is_none(),
        "the field closed on success"
    );

    // Deleting the project releases the task to No project; it is never deleted.
    rt.state.delete_project(&project);
    settle(&mut rt, &session, "the project delete", |state| {
        state.projects.is_empty()
    });
    assert_eq!(rt.state.tasks.len(), 1);
    assert_eq!(rt.state.tasks[0].list_id, None);
    assert_eq!(rt.state.tasks[0].list_name, None);

    // Undo brings the project back and refiles the task exactly where it was.
    rt.state.undo_task();
    settle(&mut rt, &session, "the project restore", |state| {
        !state.projects.is_empty()
    });
    assert_eq!(rt.state.projects[0].name, "Launch party");
    assert_eq!(rt.state.tasks[0].list_id.as_deref(), Some(project.as_str()));
}

/// Type a step into the expanded row and wait for the store's row, so the test
/// holds a real id.
fn add_step(rt: &mut Runtime<State>, session: &Session, parent: &str, title: &str) -> String {
    rt.state.begin_step(parent);
    rt.state
        .step_input
        .as_mut()
        .unwrap()
        .edit
        .replace_text(title);
    rt.state.add_step_now();
    settle(rt, session, "the created step", |state| {
        state.pending_adds.is_empty() && state.steps_of(parent).len() == 1
    });
    rt.state.steps_of(parent)[0].id.clone()
}

#[test]
fn a_step_is_saved_under_its_task_and_a_restart_shows_it() {
    let dir = {
        let (mut rt, session, dir) = open_tasks();
        add_line(&mut rt, &session, "Ship the release #Launch");
        settle(&mut rt, &session, "the task", |state| {
            state.tasks.len() == 1
        });
        let parent = rt.state.tasks[0].id.clone();
        let project = rt.state.tasks[0].list_id.clone();

        let step = add_step(&mut rt, &session, &parent, "Write the notes");
        let row = rt
            .state
            .tasks
            .iter()
            .find(|task| task.id == step)
            .expect("the step row");
        assert_eq!(row.parent_id.as_deref(), Some(parent.as_str()));
        assert_eq!(row.list_id, project, "a step is filed with its task");
        assert_eq!(row.due_day, None);
        assert_eq!(
            rt.state.open_task_count(),
            1,
            "a step is not a task of its own"
        );

        // The expanded row shows it, with its count.
        rt.state.toggle_expanded(&parent);
        rt.invalidate();
        settle_layout(&mut rt);
        assert!(rt.find(&format!("task-step-{step}")).is_some());
        assert!(all_text(&rt).contains("Write the notes"));

        // Completing the task closes the step in the same write, on screen.
        rt.state.toggle_task(&parent);
        settle(&mut rt, &session, "the completion", |state| {
            state.tasks.iter().all(|task| task.done)
        });
        assert_eq!(rt.state.open_steps_of(&parent), 0);

        // Undo takes the whole check back: the task and the step it closed.
        rt.state.undo_task();
        settle(&mut rt, &session, "the undo", |state| {
            state.tasks.iter().all(|task| !task.done)
        });
        assert_eq!(rt.state.open_steps_of(&parent), 1);
        dir
    };

    // A second launch, same database: the step is still under its task.
    let vault = dir.path().join("vault");
    let db = dir.path().join("notex.db");
    let mut state = State::with_vault(vault.display().to_string());
    state.route = Route::Tasks;
    let session = start(&mut state, &vault, &db, Quiet).expect("the store starts again");
    let mut rt = runtime(state);
    settle(&mut rt, &session, "the reloaded tasks", |state| {
        state.tasks.len() == 2
    });
    let parent = rt
        .state
        .tasks
        .iter()
        .find(|task| task.parent_id.is_none())
        .expect("the task");
    let steps = rt.state.steps_of(&parent.id);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].title, "Write the notes");
    assert_eq!(rt.state.open_task_count(), 1);
}

#[test]
fn deleting_a_task_takes_its_steps_and_undo_brings_them_back() {
    let (mut rt, session, _dir) = open_tasks();
    add_line(&mut rt, &session, "Ship the release");
    settle(&mut rt, &session, "the task", |state| {
        state.tasks.len() == 1
    });
    let parent = rt.state.tasks[0].id.clone();
    let step = add_step(&mut rt, &session, &parent, "Write the notes");

    rt.state.delete_task(&parent);
    settle(&mut rt, &session, "the delete", |state| {
        state.tasks.is_empty()
    });

    rt.state.undo_task();
    settle(&mut rt, &session, "the restore", |state| {
        state.tasks.len() == 2
    });
    assert_eq!(
        rt.state
            .steps_of(&parent)
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>(),
        [step],
        "undo puts the step back under its task"
    );
}
