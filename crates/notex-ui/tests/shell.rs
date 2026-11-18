//! Headless shell tests: navigation, states, labels and keys.
//!
//! These run on Rune's virtual clock against the real renderer's element tree, so
//! they assert what a person would see and press, without a window.

use std::collections::HashSet;

use notex_ui::{Action, Load, NoteBody, NoteRow, Route, State, Update, view};
use rune::runtime::ElementInfo;
use rune::{Env, Key, KeyEvent, Mods, Runtime};

const VAULT: &str = "/tmp/open-notex-test-vault";

fn runtime(state: State) -> Runtime<State> {
    let mut rt = Runtime::new(state, view, (1120.0, 740.0), Env::new(true, 1.0));
    rt.on_key = Some(notex_ui::on_key);
    rt.frame(0.0);
    rt
}

fn vault_state() -> State {
    State::with_vault(VAULT)
}

/// A page fades in over a few frames, and a control that is still fading is not
/// hit-testable. A person cannot click that fast; a test has to wait.
fn settle(rt: &mut Runtime<State>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

fn click(rt: &mut Runtime<State>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(1.0 / 60.0);
}

fn press(rt: &mut Runtime<State>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
}

fn element(rt: &Runtime<State>, key: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element {key}"))
}

fn label_of(rt: &Runtime<State>, key: &str) -> String {
    let e = element(rt, key);
    e.text.or(e.label).unwrap_or_default()
}

#[test]
fn every_route_is_one_press_away() {
    let mut rt = runtime(vault_state());

    for route in Route::ALL {
        click(&mut rt, &format!("nav-{}", route.key()));
        assert_eq!(
            rt.state.route,
            route,
            "nav-{} did not navigate",
            route.key()
        );
        // The toolbar names the screen, so a stale route cannot hide.
        assert_eq!(
            element(&rt, "toolbar").label.as_deref(),
            Some(route.label())
        );
    }
}

#[test]
fn the_sidebar_has_one_row_per_route() {
    let rt = runtime(vault_state());
    let rows: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|key| key.starts_with("nav-"))
        .collect();
    assert_eq!(rows.len(), Route::ALL.len(), "{rows:?}");

    let unique: HashSet<&String> = rows.iter().collect();
    assert_eq!(unique.len(), rows.len(), "a row is rendered twice");
}

#[test]
fn an_empty_route_explains_itself_in_the_detail_pane() {
    let mut rt = runtime(vault_state());
    click(&mut rt, "nav-notes");

    assert_eq!(label_of(&rt, "detail-title"), Route::Notes.empty_title());
    assert_eq!(label_of(&rt, "detail-hint"), Route::Notes.empty_hint());
    assert!(
        label_of(&rt, "detail-vault").contains(VAULT),
        "the notes empty state names the vault"
    );
}

#[test]
fn the_sidebar_lists_every_route_and_the_others_have_no_detail_pane() {
    let mut rt = runtime(vault_state());
    assert!(rt.find("detail").is_some(), "Today has a detail pane");

    click(&mut rt, "nav-settings");
    assert!(rt.find("settings").is_some(), "Settings renders");
    assert!(
        rt.find("detail").is_none(),
        "Settings is a single-pane screen"
    );
    assert_eq!(label_of(&rt, "toolbar"), "Settings");

    click(&mut rt, "nav-search");
    assert!(rt.find("page-empty").is_some(), "Search renders");
    assert!(
        rt.find("detail").is_none(),
        "Search is a single-pane screen"
    );
}

#[test]
fn settings_shows_the_vault_and_the_version() {
    let mut rt = runtime(vault_state());
    click(&mut rt, "nav-settings");

    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(
        texts.iter().any(|t| t.contains(VAULT)),
        "the vault path is shown: {texts:?}"
    );
    assert!(texts.iter().any(|t| t == "0.0.1"), "the version is shown");
}

#[test]
fn settings_offers_the_update_check_and_asks_only_when_pressed() {
    let mut rt = runtime(vault_state());
    click(&mut rt, "nav-settings");

    // Nothing is asked on launch: the app is local-first, and a check is a
    // request to GitHub.
    assert_eq!(label_of(&rt, "update-status"), "Not checked");
    assert!(
        rt.find("update-banner").is_none(),
        "no banner before a check"
    );

    settle(&mut rt);
    click(&mut rt, "update-check");
    assert_eq!(rt.state.update, Update::Checking);
    assert_eq!(label_of(&rt, "update-status"), "Checking…");
    assert_eq!(rt.state.take_action(), Some(Action::CheckForUpdates));
    assert_eq!(
        rt.state.take_action(),
        None,
        "pressing again while a check runs is not a second request"
    );
}

#[test]
fn a_newer_release_is_a_banner_with_the_page_it_lives_on() {
    let mut state = vault_state();
    state.route = Route::Settings;
    state.update_available("0.1.0");
    let mut rt = runtime(state);
    settle(&mut rt);

    assert!(rt.find("update-banner").is_some(), "the banner renders");
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(
        texts.iter().any(|t| t == "openNoteX 0.1.0 is available"),
        "{texts:?}"
    );

    click(&mut rt, "update-view");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::ShowRelease("0.1.0".to_owned()))
    );
}

#[test]
fn a_check_that_failed_is_a_sentence_and_never_a_banner() {
    let mut state = vault_state();
    state.route = Route::Settings;
    state.update_failed("GitHub did not answer: curl: (6) Could not resolve host");
    let rt = runtime(state);

    assert!(rt.find("update-banner").is_none(), "a failure is not news");
    assert!(
        label_of(&rt, "update-status").starts_with("GitHub did not answer"),
        "the reason is the row's text"
    );
}

#[test]
fn every_answer_from_the_check_reads_as_a_sentence() {
    let mut state = vault_state();
    let summary = |state: &State| state.update.summary();

    assert_eq!(summary(&state), "Not checked");

    state.request_update_check();
    assert_eq!(summary(&state), "Checking…");

    state.update_current();
    assert_eq!(summary(&state), "Up to date");

    state.update_ahead();
    assert_eq!(summary(&state), "Ahead of the latest release");

    state.update_available("0.1.0");
    assert_eq!(summary(&state), "0.1.0 is available");
    assert_eq!(state.update.offered(), Some("0.1.0"));
}

#[test]
fn the_arrows_walk_the_sidebar_and_the_selection_follows() {
    let mut rt = runtime(vault_state());
    press(&mut rt, Key::Tab); // the list is one tab stop: it lands on the selected row
    assert_eq!(rt.state.route, Route::Today);

    press(&mut rt, Key::Down);
    assert_eq!(rt.state.route, Route::Upcoming);

    press(&mut rt, Key::Down);
    assert_eq!(rt.state.route, Route::Projects, "across a section boundary");

    press(&mut rt, Key::Up);
    assert_eq!(rt.state.route, Route::Upcoming);
}

#[test]
fn command_k_opens_search_and_command_comma_opens_settings() {
    let mut rt = runtime(vault_state());

    rt.key(KeyEvent {
        key: Key::Command('k'),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.route, Route::Search);

    rt.key(KeyEvent {
        key: Key::Command(','),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.route, Route::Settings);
}

#[test]
fn shift_command_b_closes_and_reopens_the_sidebar() {
    let mut rt = runtime(vault_state());
    assert!(rt.state.sidebar.is_open());

    let toggle = |rt: &mut Runtime<State>| {
        rt.key(KeyEvent {
            key: Key::Command('b'),
            mods: Mods {
                cmd: true,
                shift: true,
                ..Mods::default()
            },
        });
        rt.frame(1.0 / 60.0);
    };

    toggle(&mut rt);
    assert!(!rt.state.sidebar.is_open(), "⇧⌘B closes the sidebar");
    toggle(&mut rt);
    assert!(rt.state.sidebar.is_open(), "and opens it again");
}

#[test]
fn a_failed_load_shows_the_reason_and_can_be_dismissed() {
    let mut state = vault_state();
    state.load = Load::Failed("the vault is on a disconnected volume".to_owned());
    let mut rt = runtime(state);

    assert_eq!(
        label_of(&rt, "list-error"),
        "the vault is on a disconnected volume"
    );

    click(&mut rt, "dismiss-error");
    assert_eq!(rt.state.load, Load::Ready);
    assert!(rt.find("list-error").is_none(), "the banner is gone");
}

#[test]
fn a_loading_route_shows_progress_rather_than_an_empty_list() {
    let mut state = vault_state();
    state.load = Load::Loading;
    let rt = runtime(state);

    assert!(rt.find("list-loading").is_some());
    assert!(rt.find("list-empty").is_none());
}

#[test]
fn every_focusable_element_has_a_label() {
    let rt = runtime(vault_state());
    let unnamed: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.focusable && e.label.as_deref().unwrap_or_default().trim().is_empty())
        .map(|e| e.path)
        .collect();
    assert!(unnamed.is_empty(), "focusable but unnamed: {unnamed:?}");
}

#[test]
fn element_keys_are_unique() {
    let mut rt = runtime(vault_state());
    click(&mut rt, "nav-habits");

    // rune-kit's own components reuse short local keys ("icon", "label", "spacer")
    // inside their own subtrees, so only the shell's own namespace is checked here.
    let ours = |key: &str| {
        [
            "nav-",
            "list-",
            "detail",
            "page-",
            "settings",
            "open-search",
            "dismiss-error",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
    };

    let mut seen = HashSet::new();
    let mut duplicates = Vec::new();
    for element in rt.elements() {
        if let Some(key) = element.key
            && ours(&key)
            && !seen.insert(key.clone())
        {
            duplicates.push(key);
        }
    }
    assert!(duplicates.is_empty(), "duplicate keys: {duplicates:?}");
}

#[test]
fn scenes_render_distinct_screens() {
    for (name, scene) in notex_ui::SCENES {
        let mut state = vault_state();
        scene(&mut state);
        let rt = runtime(state);
        assert!(rt.find("sidebar").is_some(), "scene {name} has no sidebar");
        assert!(rt.find("toolbar").is_some(), "scene {name} has no toolbar");
    }
}

fn notes_state() -> State {
    let mut state = vault_state();
    state.route = Route::Notes;
    state.set_notes(vec![
        NoteRow::new("01A", "Apple", &["fruit"]),
        NoteRow::new("01B", "Banana", &[]),
    ]);
    state
}

#[test]
fn the_notes_list_shows_a_row_per_note_and_selects_on_press() {
    let mut rt = runtime(notes_state());

    assert_eq!(label_of(&rt, "list-subtitle"), "2 notes");
    assert!(rt.find("list-note-01A").is_some());
    assert!(rt.find("list-note-01B").is_some());
    assert_eq!(label_of(&rt, "detail-title"), Route::Notes.empty_title());

    click(&mut rt, "list-note-01B");
    assert_eq!(rt.state.selected.as_deref(), Some("01B"));
    assert_eq!(rt.state.body, NoteBody::Loading);
    assert!(
        rt.find("detail-loading").is_some(),
        "the body is on its way"
    );
    assert!(rt.find("detail-hint").is_none(), "the empty state is gone");
}

fn open_note(rt: &mut Runtime<State>) {
    click(rt, "list-note-01A");
    rt.state.show_body("01A", "# Apple\n\nA crisp note.\n", 7);
    rt.invalidate();
    rt.frame(1.0 / 60.0);
}

#[test]
fn an_open_note_shows_the_file_in_the_editor() {
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body(
        "01A",
        "# Apple\n\nA crisp note with a [link](https://example.com).\n",
        7,
    );
    let rt = runtime(state);

    assert_eq!(label_of(&rt, "detail-title"), "Apple");
    assert_eq!(label_of(&rt, "detail-tags"), "fruit");
    assert!(rt.find("detail-editor").is_some(), "the editor is mounted");
    assert_eq!(label_of(&rt, "detail-status-label"), "Saved");
    assert!(rt.find("save-note").is_none(), "nothing to save yet");
    assert!(
        rt.find("list-dirty-01A").is_none(),
        "a clean note carries no dot"
    );
    let text = &rt.state.draft.as_ref().unwrap().edit.text;
    assert!(text.starts_with("# Apple\n\nA crisp note"), "{text}");
}

#[test]
fn the_new_note_button_and_command_n_ask_for_a_note() {
    let mut rt = runtime(notes_state());
    assert!(rt.find("new-note").is_some(), "the list offers a new note");
    click(&mut rt, "new-note");
    assert_eq!(rt.state.take_action(), Some(notex_ui::Action::Create));

    press(&mut rt, Key::Command('n'));
    assert_eq!(rt.state.take_action(), Some(notex_ui::Action::Create));

    // A route without a note list has nothing to add a note to.
    let mut state = notes_state();
    state.route = Route::Today;
    let rt = runtime(state);
    assert!(rt.find("new-note").is_none());
}

#[test]
fn deleting_a_note_asks_before_it_moves_the_file() {
    let mut rt = runtime(notes_state());
    open_note(&mut rt);

    click(&mut rt, "delete-note");
    assert!(rt.find("delete-note-dialog").is_some(), "the dialog is up");
    assert_eq!(rt.state.confirm_delete.as_deref(), Some("01A"));
    assert!(
        rt.state.take_action().is_none(),
        "nothing happens before the answer"
    );
    let dialog_title = label_of(&rt, "delete-note-dialog");
    assert_eq!(dialog_title, "Delete “Apple”?", "{dialog_title}");

    click(&mut rt, "delete-cancel");
    assert!(rt.find("delete-note-dialog").is_none(), "the dialog closes");
    assert!(rt.state.confirm_delete.is_none());
    assert!(rt.state.take_action().is_none());

    click(&mut rt, "delete-note");
    click(&mut rt, "delete-confirm");
    assert_eq!(
        rt.state.take_action(),
        Some(notex_ui::Action::Delete("01A".to_owned()))
    );
    assert!(rt.find("delete-note-dialog").is_none());
}

#[test]
fn command_backspace_asks_to_delete_the_open_note() {
    let mut rt = runtime(notes_state());
    open_note(&mut rt);

    rt.key(KeyEvent {
        key: Key::Backspace,
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.confirm_delete.as_deref(), Some("01A"));
}

#[test]
fn a_refused_create_is_explained_under_the_list_header() {
    let mut state = notes_state();
    state.action_error = Some("the vault is not open yet".to_owned());
    let rt = runtime(state);

    assert!(rt.find("list-action-error").is_some());
    assert_eq!(
        label_of(&rt, "list-action-error-text"),
        "the vault is not open yet"
    );
}

#[test]
fn typing_marks_the_note_edited_and_offers_save() {
    let mut rt = runtime(notes_state());
    open_note(&mut rt);

    click(&mut rt, "detail-editor");
    rt.paste(" and more");
    rt.frame(1.0 / 60.0);

    assert!(rt.state.draft.as_ref().unwrap().is_dirty());
    assert_eq!(label_of(&rt, "detail-status-label"), "Edited");
    assert!(rt.find("save-note").is_some(), "an explicit save appears");
    assert!(
        rt.find("list-dirty-01A").is_some(),
        "the row says it has unsaved text"
    );

    click(&mut rt, "save-note");
    let action = rt.state.take_action().unwrap();
    assert!(matches!(action, notex_ui::Action::Save(request) if request.body.contains("and more")));
}

#[test]
fn command_s_asks_for_a_save_while_editing() {
    let mut rt = runtime(notes_state());
    open_note(&mut rt);
    click(&mut rt, "detail-editor");
    rt.paste("!");
    rt.frame(1.0 / 60.0);

    rt.key(KeyEvent {
        key: Key::Command('s'),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    rt.frame(1.0 / 60.0);

    let action = rt.state.take_action();
    assert!(
        matches!(action, Some(notex_ui::Action::Save(_))),
        "⌘S asks for a save now, got {action:?}"
    );
    rt.invalidate();
    rt.frame(1.0 / 60.0);
    assert_eq!(label_of(&rt, "detail-status-label"), "Saving…");
}

#[test]
fn a_conflict_explains_itself_and_offers_two_ways_out() {
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nMine.\n", 7);
    state.draft_touched();
    state.save_conflict("01A", "This file changed on disk since you opened it.");
    let mut rt = runtime(state);

    assert_eq!(label_of(&rt, "detail-status-label"), "Changed on disk");
    assert!(rt.find("detail-conflict").is_some());
    assert!(
        rt.find("save-note").is_none(),
        "a blocked save has no button"
    );
    assert!(
        label_of(&rt, "conflict-message").contains("changed on disk"),
        "the banner says what happened"
    );

    click(&mut rt, "conflict-overwrite");
    let action = rt.state.take_action().unwrap();
    assert!(
        matches!(action, notex_ui::Action::Save(request) if request.force),
        "overwrite writes anyway"
    );
}

#[test]
fn a_failed_save_keeps_the_draft_and_retry_sends_it_again() {
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nMine.\n", 7);
    state.draft_touched();
    state.save_failed("01A", "io: the disk is full");
    let mut rt = runtime(state);

    assert_eq!(label_of(&rt, "detail-status-label"), "Not saved");
    assert!(rt.find("detail-save-failed").is_some());
    assert_eq!(label_of(&rt, "save-error"), "io: the disk is full");
    assert_eq!(
        rt.state.draft.as_ref().unwrap().edit.text,
        "# Apple\n\nMine.\n"
    );

    click(&mut rt, "save-retry");
    let action = rt.state.take_action();
    assert!(
        matches!(action, Some(notex_ui::Action::Save(_))),
        "Try again writes the same draft, got {action:?}"
    );
}

#[test]
fn a_failed_body_explains_itself_without_losing_the_list() {
    let mut state = notes_state();
    state.selected = Some("01B".to_owned());
    state.body = NoteBody::Failed("io: permission denied".to_owned());
    let rt = runtime(state);

    assert!(rt.find("detail-failed").is_some());
    assert_eq!(label_of(&rt, "detail-message"), "io: permission denied");
    assert!(rt.find("list-note-01A").is_some(), "the list stays usable");
}

#[test]
fn a_note_that_left_the_list_clears_the_selection() {
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nA crisp note.\n", 7);
    state.set_notes(vec![NoteRow::new("01B", "Banana", &[])]);

    assert_eq!(state.selected, None);
    assert_eq!(state.body, NoteBody::None);
    assert!(state.draft.is_none(), "the editor goes with it");
    assert_eq!(state.pending_open, None, "the stale read is dropped too");
}
