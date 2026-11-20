//! Headless shell tests: navigation, states, labels and keys.
//!
//! These run on Rune's virtual clock against the real renderer's element tree, so
//! they assert what a person would see and press, without a window.

use std::collections::HashSet;

use notex_ui::{
    Action, Load, NoteBody, NoteRow, Route, State, Update, VaultIntent, VaultStatus, view,
};
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
    assert!(rt.find("detail").is_none(), "Home is one column");
    click(&mut rt, "nav-notes");
    assert!(rt.find("detail").is_some(), "Notes has a detail pane");

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
    assert_eq!(rt.state.route, Route::Home);

    press(&mut rt, Key::Down);
    assert_eq!(rt.state.route, Route::Tasks);

    press(&mut rt, Key::Down);
    assert_eq!(rt.state.route, Route::Notes, "across a section boundary");

    press(&mut rt, Key::Up);
    assert_eq!(rt.state.route, Route::Tasks);
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
    state.route = Route::Tasks;
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

#[test]
fn a_note_changed_on_disk_is_re_read_unless_the_draft_is_dirty() {
    // Another program saved the file and the editor has nothing unsaved: show
    // what is on disk now.
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nfrom the file\n", 7);
    assert!(state.refresh_from_disk(&["01A".to_owned()]));
    assert_eq!(
        state.take_action(),
        Some(notex_ui::Action::Open("01A".to_owned())),
        "the file is read again"
    );

    // The same change with unsaved text in the editor: nothing is taken away.
    // The next save reports the conflict and the person chooses.
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nmine\n", 7);
    state.draft_touched();
    assert!(!state.refresh_from_disk(&["01A".to_owned()]));
    assert!(
        state
            .draft
            .as_ref()
            .is_some_and(|draft| draft.edit.text.contains("mine")),
        "the draft is untouched"
    );
    assert_eq!(state.take_action(), None, "no read goes out");

    // A different note moving is not this editor's business.
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nmine\n", 7);
    assert!(!state.refresh_from_disk(&["01B".to_owned()]));
    assert_eq!(state.take_action(), None);
}

#[test]
fn a_conflict_banner_is_not_replaced_by_a_quiet_reload() {
    // The banner is the person's decision. A background re-read would erase the
    // message before they answered it.
    let mut state = notes_state();
    state.select_note("01A");
    state.show_body("01A", "# Apple\n\nmine\n", 7);
    state.save_conflict("01A", "This file changed on disk since you opened it.");
    assert!(!state.refresh_from_disk(&["01A".to_owned()]));
    assert!(state.draft.as_ref().unwrap().conflict.is_some());
}

/// A first launch: no vault, the dialog up, and a status the app already
/// answered (the real app inspects the folder; a test does not need a disk).
fn setup_state(status: VaultStatus) -> State {
    let mut state = State::with_vault(VAULT);
    state.ask_for_vault(VAULT, None);
    state.vault_checked(VAULT, status);
    state
}

#[test]
fn a_missing_vault_is_one_dialog_with_create_choose_and_not_now() {
    let mut rt = runtime(setup_state(VaultStatus::Missing));
    settle(&mut rt);

    assert!(rt.find("vault-dialog").is_some(), "the dialog is up");
    assert_eq!(label_of(&rt, "vault-path"), VAULT);
    assert_eq!(
        label_of(&rt, "vault-status"),
        "This folder does not exist yet."
    );
    assert_eq!(label_of(&rt, "vault-primary"), "Create folder");
    assert!(
        rt.find("vault-choose").is_some(),
        "the system sheet is offered"
    );
    assert!(rt.find("vault-not-now").is_some(), "leaving is an option");

    click(&mut rt, "vault-primary");
    assert_eq!(
        rt.state.take_vault_intent(),
        Some(VaultIntent::Create(VAULT.to_owned()))
    );
    assert!(rt.state.vault_setup.busy, "the buttons wait for the answer");
}

#[test]
fn the_status_line_names_what_the_folder_is() {
    for (status, line, label) in [
        (
            VaultStatus::Missing,
            "This folder does not exist yet.",
            "Create folder",
        ),
        (
            VaultStatus::Empty,
            "This folder is empty. It becomes your vault.",
            "Use this folder",
        ),
        (VaultStatus::Notes(3), "3 notes here.", "Use this folder"),
        (
            VaultStatus::Unusable("That folder is read-only.".to_owned()),
            "That folder is read-only.",
            "Use this folder",
        ),
    ] {
        let mut rt = runtime(setup_state(status.clone()));
        settle(&mut rt);
        assert_eq!(label_of(&rt, "vault-status"), line);
        assert_eq!(label_of(&rt, "vault-primary"), label);

        if !status.can_open() {
            click(&mut rt, "vault-primary");
            assert_eq!(
                rt.state.take_vault_intent(),
                None,
                "a disabled primary button does nothing"
            );
        }
    }
}

#[test]
fn one_note_is_singular_and_an_unusable_folder_has_no_primary_action() {
    let mut rt = runtime(setup_state(VaultStatus::Notes(1)));
    settle(&mut rt);
    assert_eq!(label_of(&rt, "vault-status"), "1 note here.");

    let mut rt = runtime(setup_state(VaultStatus::Unusable(
        "That is a file, not a folder.".to_owned(),
    )));
    settle(&mut rt);
    assert_eq!(label_of(&rt, "vault-primary"), "Use this folder");
    click(&mut rt, "vault-primary");
    assert_eq!(rt.state.take_vault_intent(), None);
}

#[test]
fn not_now_keeps_the_app_and_every_no_vault_surface_opens_the_same_dialog() {
    let mut rt = runtime(setup_state(VaultStatus::Missing));
    settle(&mut rt);

    click(&mut rt, "vault-not-now");
    settle(&mut rt);
    assert!(!rt.state.vault_dialog, "the dialog closed");
    assert!(rt.find("vault-dialog").is_none());
    assert!(!rt.state.vault_open, "the app is still open");

    // The empty list names the reason and offers the same dialog back.
    rt.state.route = Route::Notes;
    rt.invalidate();
    rt.frame(1.0 / 60.0);
    assert_eq!(label_of(&rt, "list-no-vault-title"), "No vault yet");
    click(&mut rt, "list-no-vault-choose");
    assert!(
        rt.state.vault_dialog && rt.find("vault-dialog").is_some(),
        "the list asks in the same dialog"
    );
    click(&mut rt, "vault-not-now");

    // Settings, with no vault, asks in it too.
    click(&mut rt, "nav-settings");
    settle(&mut rt);
    assert_eq!(label_of(&rt, "settings-vault"), "Not configured");
    click(&mut rt, "settings-choose-vault");
    assert!(
        rt.state.vault_dialog && rt.find("vault-dialog").is_some(),
        "Settings asks in the same dialog"
    );
}

#[test]
fn choosing_a_folder_uses_the_native_sheet_and_the_same_dialog() {
    let mut rt = runtime(setup_state(VaultStatus::Missing));
    settle(&mut rt);

    click(&mut rt, "vault-choose");
    let (kind, options) = rt.file_dialog().expect("the press asks for a folder");
    assert_eq!(kind, rune::FileDialogKind::Folder);
    assert_eq!(options.title, "Choose a notes folder");
    assert_eq!(
        options.directory.as_deref(),
        Some(std::path::Path::new(VAULT)),
        "the sheet starts where the dialog is looking"
    );

    rt.resolve_file_dialog(Some("/Users/you/Obsidian/Personal".into()))
        .unwrap();
    rt.frame(1.0 / 60.0);
    assert_eq!(
        rt.state.take_vault_intent(),
        Some(VaultIntent::Use("/Users/you/Obsidian/Personal".to_owned())),
        "a picked folder is used, not just remembered"
    );
    assert!(rt.state.vault_dialog, "it stays until the folder opens");
    assert!(rt.state.vault_setup.busy);

    // Cancelling changes nothing.
    let mut rt = runtime(setup_state(VaultStatus::Missing));
    settle(&mut rt);
    click(&mut rt, "vault-choose");
    rt.resolve_file_dialog(None).unwrap();
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.take_vault_intent(), None);
    assert!(!rt.state.vault_setup.busy);
    assert!(rt.state.vault_dialog);
}

#[test]
fn new_note_without_a_vault_asks_for_one_instead_of_complaining() {
    let mut state = State::with_vault(VAULT);
    state.ask_for_vault(VAULT, None);
    state.vault_checked(VAULT, VaultStatus::Missing);
    state.close_vault_dialog();
    let mut rt = runtime(state);

    rt.key(KeyEvent {
        key: Key::Command('n'),
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);

    assert!(rt.state.vault_dialog, "⌘N opens the dialog");
    assert_eq!(rt.state.action_error, None, "there is nothing to blame");
    assert_eq!(rt.state.take_action(), None, "no note is created");
}

#[test]
fn a_refusal_stays_in_the_dialog_with_the_reason_and_the_path() {
    let mut state = setup_state(VaultStatus::Missing);
    state.vault_failed("That folder is read-only.");
    let mut rt = runtime(state);
    settle(&mut rt);

    assert!(rt.state.vault_dialog, "a refusal does not close the dialog");
    assert!(!rt.state.vault_setup.busy, "Try again is possible");
    assert_eq!(label_of(&rt, "vault-notice"), "That folder is read-only.");
    assert_eq!(label_of(&rt, "vault-path"), VAULT, "the path is kept");
}

#[test]
fn not_now_drops_a_folder_that_was_mid_open() {
    let mut rt = runtime(setup_state(VaultStatus::Missing));
    settle(&mut rt);

    click(&mut rt, "vault-primary");
    assert!(rt.state.vault_pending(), "the app has work queued");

    click(&mut rt, "vault-not-now");
    assert_eq!(rt.state.take_vault_intent(), None);
    assert!(
        !rt.state.vault_pending(),
        "nothing opens after Not now, not even a queued folder"
    );
}

// ---------------------------------------------------------------- Home / Tasks

fn tasks_state() -> State {
    use notex_ui::{Now, ProjectRow, TaskRow};

    let task = |id: &str,
                title: &str,
                due_day: Option<&str>,
                project: Option<(&str, &str)>,
                done: bool,
                completed_day: Option<&str>| {
        let (list_id, list_name) = match project {
            Some((id, name)) => (Some(id.to_owned()), Some(name.to_owned())),
            None => (None, None),
        };
        TaskRow {
            id: id.to_owned(),
            title: title.to_owned(),
            notes: String::new(),
            done,
            list_id,
            list_name,
            list_color: Some("#3f8f6b".to_owned()),
            due_day: due_day.map(str::to_owned),
            completed_at: completed_day.map(|_| 1_763_500_800_000),
            completed_day: completed_day.map(str::to_owned),
        }
    };

    let mut state = vault_state();
    state.set_clock(Now {
        day: "2026-11-19".to_owned(),
        time: "09:41".to_owned(),
        date: "Thursday 19 November".to_owned(),
    });
    state.set_projects(vec![ProjectRow {
        id: "01L1".to_owned(),
        name: "Launch".to_owned(),
        color: Some("#3f8f6b".to_owned()),
    }]);
    state.set_notes(vec![
        NoteRow::new("01A", "Apple", &[]).modified(2),
        NoteRow::new("01B", "Banana", &[]).modified(1),
    ]);
    state.set_tasks(vec![
        task(
            "01T1",
            "Send invites",
            Some("2026-11-17"),
            Some(("01L1", "Launch")),
            false,
            None,
        ),
        task(
            "01T2",
            "Outline the launch note",
            Some("2026-11-19"),
            None,
            false,
            None,
        ),
        task(
            "01T3",
            "Book the room",
            Some("2026-11-22"),
            None,
            false,
            None,
        ),
        task("01T4", "Read the Rune notes", None, None, false, None),
        task(
            "01T5",
            "Set up the vault",
            None,
            None,
            true,
            Some("2026-11-18"),
        ),
    ]);
    state
}

#[test]
fn home_is_the_first_screen_and_draws_the_day_in_three_blocks() {
    let rt = runtime(tasks_state());

    assert_eq!(rt.state.route, Route::Home);
    assert_eq!(label_of(&rt, "home-time"), "09:41");
    assert_eq!(label_of(&rt, "home-date"), "Thursday 19 November");
    assert!(
        label_of(&rt, "home-summary").contains("4 open"),
        "{}",
        label_of(&rt, "home-summary")
    );

    // One block holds Todo, Reminders and Next up, in that order.
    assert!(rt.find("home-focus").is_some());
    assert!(rt.find("home-todo").is_some());
    assert!(rt.find("home-reminders-head").is_some());
    assert!(rt.find("home-next-head").is_some());
    assert!(rt.find("home-recent").is_some());

    // Overdue and today are in the todo block, upcoming in Next up.
    assert!(
        rt.find("home-task-01T1").is_some(),
        "the overdue task is on Home"
    );
    assert!(
        rt.find("home-task-01T2").is_some(),
        "today's task is on Home"
    );
    assert!(rt.find("home-task-01T3").is_some(), "next up is on Home");
}

#[test]
fn home_lists_recent_notes_newest_first() {
    let rt = runtime(tasks_state());
    let order: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|key| key.starts_with("home-note-"))
        .collect();
    assert_eq!(order, ["home-note-01A", "home-note-01B"]);
}

#[test]
fn the_sidebar_counts_open_tasks_and_has_no_projects_row() {
    let rt = runtime(tasks_state());
    assert_eq!(label_of(&rt, "nav-home"), "Home");
    assert_eq!(label_of(&rt, "nav-tasks"), "Tasks (4)");

    let keys: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|key| key.starts_with("nav-"))
        .collect();
    for gone in ["nav-inbox", "nav-today", "nav-upcoming", "nav-projects"] {
        assert!(
            !keys.iter().any(|key| key == gone),
            "{gone} is still in the sidebar"
        );
    }
}

#[test]
fn the_checklist_groups_by_when_a_task_is_due() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");

    assert!(rt.find("task-group-overdue").is_some());
    assert!(rt.find("task-group-today").is_some());
    assert!(rt.find("task-group-upcoming").is_some());
    assert!(rt.find("task-group-someday").is_some());
    assert_eq!(label_of(&rt, "task-group-overdue-count"), "1");
    assert!(
        rt.find("task-row-01T5").is_none(),
        "a done task is not in the checklist"
    );
    // The due chip reads as a person would say it.
    assert_eq!(label_of(&rt, "task-due-01T1"), "Due 2d late");
    assert_eq!(label_of(&rt, "task-due-01T3"), "Due Sun");
}

#[test]
fn typing_a_line_and_pressing_return_asks_for_a_task_and_keeps_the_field() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);

    click(&mut rt, "task-add-input");
    rt.paste("Send invites #Launch");
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Enter);

    assert_eq!(
        rt.state.take_action(),
        Some(Action::CreateTask {
            title: "Send invites".to_owned(),
            project: Some("Launch".to_owned()),
            due_day: None,
        })
    );
    assert_eq!(
        rt.state.add_task.text, "",
        "the field is cleared, not closed"
    );
    assert!(
        rt.find("task-add-input").is_some(),
        "Return leaves the caret in the add row"
    );
}

#[test]
fn an_empty_add_row_asks_for_nothing() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    click(&mut rt, "task-add-input");
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.take_action(), None);
}

#[test]
fn a_project_chip_narrows_the_list_and_adds_into_that_project() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    click(&mut rt, "task-filter-01L1");

    assert_eq!(rt.state.project_filter.as_deref(), Some("01L1"));
    assert!(rt.find("task-row-01T1").is_some());
    assert!(
        rt.find("task-row-01T2").is_none(),
        "another project is filtered out"
    );
    assert!(
        rt.find("task-group-today").is_none(),
        "empty groups disappear"
    );

    // A new line in a filtered list lands in the project it is showing.
    settle(&mut rt);
    click(&mut rt, "task-add-input");
    rt.paste("Book the venue");
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Enter);
    assert_eq!(
        rt.state.take_action(),
        Some(Action::CreateTask {
            title: "Book the venue".to_owned(),
            project: Some("Launch".to_owned()),
            due_day: None,
        })
    );
}

#[test]
fn a_new_project_is_a_chip_field_and_an_action() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);

    click(&mut rt, "task-filter-new");
    assert!(rt.find("task-project-input").is_some(), "the field opens");
    click(&mut rt, "task-project-input");
    rt.paste("Studio");
    rt.frame(1.0 / 60.0);
    press(&mut rt, Key::Enter);

    assert_eq!(
        rt.state.take_action(),
        Some(Action::CreateProject("Studio".to_owned()))
    );
    assert!(rt.find("task-project-input").is_none(), "the field closes");
}

#[test]
fn clicking_a_title_renames_it_in_place() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);

    click(&mut rt, "task-title-01T2");
    assert!(
        rt.find("task-edit-01T2").is_some(),
        "the row became a field"
    );

    press(&mut rt, Key::Escape);
    assert!(
        rt.find("task-edit-01T2").is_none(),
        "Escape puts the title back"
    );
    assert_eq!(rt.state.take_action(), None);

    click(&mut rt, "task-title-01T2");
    let edit = rt.state.task_edit.as_mut().unwrap();
    edit.edit.replace_text("Outline the note");
    press(&mut rt, Key::Enter);
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskTitle {
            id: "01T2".to_owned(),
            title: "Outline the note".to_owned(),
        })
    );
}

#[test]
fn checking_a_task_offers_eight_seconds_to_undo_it() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);

    click(&mut rt, "task-check-01T2");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskDone {
            id: "01T2".to_owned(),
            done: true,
        })
    );
    assert!(rt.find("task-undo").is_some(), "the toast is up");

    rt.state.undo_task();
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskDone {
            id: "01T2".to_owned(),
            done: false,
        })
    );
    assert!(rt.state.undo.is_none(), "the toast closes when it is used");
}

#[test]
fn the_undo_toast_expires_after_eight_seconds() {
    let mut state = tasks_state();
    state.route = Route::Tasks;
    state.toggle_task("01T2");
    assert!(state.undo.is_some());

    // The app ticks once a second, so eight seconds is eight seconds.
    let mut clock = 0.0;
    for _ in 0..7 {
        clock += 1.0;
        state.tick(clock);
    }
    assert!(state.undo.is_some(), "seven seconds is still time to undo");
    state.tick(clock + 1.0);
    assert!(state.undo.is_none(), "the toast is gone at eight");
}

#[test]
fn deleting_a_task_is_one_press_and_a_restore() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);
    click(&mut rt, "task-expand-01T2");

    click(&mut rt, "task-delete-01T2");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::DeleteTask("01T2".to_owned()))
    );
    rt.state.undo_task();
    assert_eq!(
        rt.state.take_action(),
        Some(Action::RestoreTask("01T2".to_owned()))
    );
}

#[test]
fn the_expanded_row_sets_a_due_day_and_a_project() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    settle(&mut rt);
    click(&mut rt, "task-expand-01T4");

    click(&mut rt, "task-due-1-01T4");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskDue {
            id: "01T4".to_owned(),
            due_day: Some("2026-11-20".to_owned()),
        })
    );

    click(&mut rt, "task-project-01L1-01T4");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskProject {
            id: "01T4".to_owned(),
            list_id: Some("01L1".to_owned()),
        })
    );
}

#[test]
fn the_archive_groups_done_tasks_by_the_day_they_were_done() {
    let mut state = tasks_state();
    state.route = Route::Tasks;
    state.archive = true;
    let rt = runtime(state);

    assert!(rt.find("archive-list").is_some());
    assert!(rt.find("archive-row-01T5").is_some());
    assert!(
        rt.find("archive-day-2026-11-18").is_some(),
        "the day heading is there"
    );
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(texts.iter().any(|t| t == "Yesterday"), "{texts:?}");
    assert!(
        rt.find("task-add-input").is_none(),
        "the archive is not an add screen"
    );
}

#[test]
fn reopening_from_the_archive_asks_to_undo_the_completion() {
    let mut rt = runtime(tasks_state());
    click(&mut rt, "nav-tasks");
    click(&mut rt, "tasks-archive");
    settle(&mut rt);

    click(&mut rt, "archive-restore-01T5");
    assert_eq!(
        rt.state.take_action(),
        Some(Action::SetTaskDone {
            id: "01T5".to_owned(),
            done: false,
        })
    );
}

#[test]
fn a_task_changed_replaces_the_row_without_reordering_the_list() {
    use notex_ui::TaskRow;

    let mut state = tasks_state();
    state.route = Route::Tasks;
    let order =
        |state: &State| -> Vec<String> { state.tasks.iter().map(|task| task.id.clone()).collect() };
    let before = order(&state);

    let mut changed: TaskRow = state.tasks.iter().find(|t| t.id == "01T2").unwrap().clone();
    changed.title = "Outline the note".to_owned();
    state.task_changed(changed);
    assert_eq!(order(&state), before, "a rename does not move the row");
    assert_eq!(
        state.tasks.iter().find(|t| t.id == "01T2").unwrap().title,
        "Outline the note"
    );

    state.task_gone("01T2");
    assert!(state.tasks.iter().all(|task| task.id != "01T2"));
}

#[test]
fn a_task_failure_is_a_sentence_above_the_list() {
    let mut state = tasks_state();
    state.route = Route::Tasks;
    state.task_failed("the store is no longer running");
    let rt = runtime(state);
    assert_eq!(
        label_of(&rt, "task-error-text"),
        "the store is no longer running"
    );
}

#[test]
fn every_task_scene_renders_its_screen() {
    for (name, scene) in notex_ui::SCENES {
        let mut state = vault_state();
        scene(&mut state);
        let rt = runtime(state);
        match *name {
            "home" => assert!(rt.find("home-clock").is_some(), "home"),
            "tasks" => assert!(rt.find("task-list").is_some(), "tasks"),
            "archive" => assert!(rt.find("archive-list").is_some(), "archive"),
            _ => {}
        }
    }
}
