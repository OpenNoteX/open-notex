//! Headless shell tests: navigation, states, labels and keys.
//!
//! These run on Rune's virtual clock against the real renderer's element tree, so
//! they assert what a person would see and press, without a window.

use std::collections::HashSet;

use notex_ui::{Load, NoteBody, NoteRow, Route, State, view};
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
    let text = &rt.state.draft.as_ref().unwrap().edit.text;
    assert!(text.starts_with("# Apple\n\nA crisp note"), "{text}");
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
fn a_failed_save_keeps_the_draft_and_the_text() {
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nMine.\n", 7);
    state.draft_touched();
    state.save_failed("01A", "io: the disk is full");
    let rt = runtime(state);

    assert_eq!(label_of(&rt, "detail-status-label"), "Not saved");
    assert!(rt.find("detail-save-failed").is_some());
    assert_eq!(label_of(&rt, "save-error"), "io: the disk is full");
    assert_eq!(
        rt.state.draft.as_ref().unwrap().edit.text,
        "# Apple\n\nMine.\n"
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
