//! The notes flow, end to end: a real temp vault, the real worker, the real
//! element tree. Only the GPU window is missing.
//!
//! These cover the wiring `poll` does: index the vault, list the notes, read a
//! body from its file when the list selects it, and show a readable failure when
//! the vault or the database path is wrong.

use std::fs;
use std::time::{Duration, Instant};

use notex_store::Notify;
use notex_ui::{Load, NoteBody, Route, State, view};
use open_notex::{Session, pump, start};
use rune::{Env, Runtime};

const SIZE: (f32, f32) = (1120.0, 740.0);

struct Quiet;

impl Notify for Quiet {
    fn notify(&self) {}
}

fn runtime(state: State) -> Runtime<State> {
    let mut rt = Runtime::new(state, view, SIZE, Env::new(true, 1.0));
    rt.frame(0.0);
    rt
}

fn label_of(rt: &Runtime<State>, key: &str) -> String {
    rt.elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key))
        .and_then(|element| element.text.or(element.label))
        .unwrap_or_default()
}

fn click(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

/// Pump the worker until `done` holds, drawing a frame after each change.
fn settle(rt: &mut Runtime<State>, session: &Session, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let changed = pump(&mut rt.state, session.handle());
        if changed {
            // State changed outside a handler; in the app, `poll` returning true
            // does this. The test says it explicitly.
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
        "{what} never finished: load={:?} notes={}",
        rt.state.load,
        rt.state.notes.len()
    );
}

fn all_text(rt: &Runtime<State>) -> String {
    rt.elements()
        .into_iter()
        .filter_map(|element| element.text.or(element.label))
        .collect::<Vec<_>>()
        .join("\n")
}

fn temp_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("vault");
    fs::create_dir(&vault).unwrap();
    fs::write(
        vault.join("weekly-review.md"),
        "# Weekly review\n\nThe launch plan is written down.\n\n- [ ] ship it\n",
    )
    .unwrap();
    fs::write(
        vault.join("grocery-list.md"),
        "---\ntags: [errands]\n---\n# Grocery list\n\n- milk\n- coffee\n",
    )
    .unwrap();
    (dir, vault)
}

#[test]
fn a_vault_lists_its_notes_and_opens_a_body_from_the_file() {
    let (dir, vault) = temp_vault();
    let db = dir.path().join("notex.db");

    let mut state = State::with_vault(vault.display().to_string());
    state.route = Route::Notes;
    let session = start(&mut state, &vault, &db, Quiet).expect("the store starts");
    let mut rt = runtime(state);

    settle(&mut rt, &session, "the note list", |state| {
        state.load == Load::Ready && state.notes.len() == 2
    });

    // Alphabetical, with the frontmatter tags as the second line.
    let titles: Vec<&str> = rt.state.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles, vec!["Grocery list", "Weekly review"]);
    assert_eq!(rt.state.notes[0].tags, vec!["errands"]);
    assert_eq!(label_of(&rt, "list-subtitle"), "2 notes");
    assert!(
        all_text(&rt).contains("Grocery list") && all_text(&rt).contains("Weekly review"),
        "both rows are in the tree"
    );

    // The detail pane still explains itself until a row is selected.
    assert_eq!(label_of(&rt, "detail-title"), Route::Notes.empty_title());

    let id = rt.state.notes[1].id.clone();
    click(&mut rt, &format!("list-note-{id}"));
    assert_eq!(rt.state.selected.as_deref(), Some(id.as_str()));
    assert_eq!(rt.state.body, NoteBody::Loading);
    assert!(rt.find("detail-loading").is_some(), "a body shows progress");

    settle(&mut rt, &session, "the note body", |state| {
        matches!(state.body, NoteBody::Ready(_))
    });

    assert!(rt.find("detail-body").is_some(), "Markdown is rendered");
    assert_eq!(label_of(&rt, "detail-title"), "Weekly review");
    let text = all_text(&rt);
    assert!(text.contains("The launch plan is written down."), "{text}");
    assert!(!text.contains("tags:"), "frontmatter is not shown");
}

#[test]
fn pressing_the_selected_row_again_does_not_re_read_the_file() {
    let (dir, vault) = temp_vault();
    let db = dir.path().join("notex.db");

    let mut state = State::with_vault(vault.display().to_string());
    state.route = Route::Notes;
    let session = start(&mut state, &vault, &db, Quiet).expect("the store starts");
    let mut rt = runtime(state);
    settle(&mut rt, &session, "the note list", |state| {
        state.notes.len() == 2
    });

    let id = rt.state.notes[0].id.clone();
    let key = format!("list-note-{id}");
    click(&mut rt, &key);
    settle(&mut rt, &session, "the note body", |state| {
        matches!(state.body, NoteBody::Ready(_))
    });

    let before = rt.state.body.clone();
    click(&mut rt, &key);
    assert_eq!(rt.state.body, before, "the body did not reload");
}

#[test]
fn a_missing_vault_is_explained_in_the_list_pane() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("not-here");
    let db = dir.path().join("notex.db");

    let mut state = State::with_vault(missing.display().to_string());
    state.route = Route::Notes;
    let result = start(&mut state, &missing, &db, Quiet);
    assert!(result.is_err(), "a missing vault cannot start a store");
    let error = result.err().expect("an error").to_string();
    assert!(error.contains("missing"), "{error}");

    let Load::Failed(showing) = &state.load else {
        panic!("the failure is in state, not a panic");
    };
    assert_eq!(showing, &error);

    let rt = runtime(state);
    assert_eq!(label_of(&rt, "list-error"), error);
    assert!(
        rt.find("dismiss-error").is_some(),
        "the error is dismissible"
    );
}

#[test]
fn a_database_inside_the_vault_is_refused() {
    let (_dir, vault) = temp_vault();
    let db = vault.join("notex.db");

    let mut state = State::with_vault(vault.display().to_string());
    let result = start(&mut state, &vault, &db, Quiet);
    assert!(result.is_err(), "rule 8 is enforced at startup");
    let error = result.err().expect("an error").to_string();
    assert!(error.contains("vault"), "{error}");
    assert!(matches!(state.load, Load::Failed(_)));
}
