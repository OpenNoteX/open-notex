//! Choosing a vault: launch resolution, what a folder is, and opening one.
//!
//! The pure functions take their directories as arguments, so every case here
//! runs in a temp home. The one end-to-end test drives the real worker through
//! [`open_vault`], which is what `poll` calls in the app.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use notex_store::Notify;
use notex_ui::{Load, NoteBody, State, VaultIntent, VaultStatus};
use open_notex::{Session, open_vault, pump, vault};

struct Quiet;

impl Notify for Quiet {
    fn notify(&self) {}
}

/// A temp home, with the app's data folder outside it.
struct Home {
    _dir: tempfile::TempDir,
    home: std::path::PathBuf,
    support: std::path::PathBuf,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        fs::create_dir_all(&home).unwrap();
        let support = dir.path().join("support");
        Self {
            _dir: dir,
            home,
            support,
        }
    }

    fn db(&self) -> std::path::PathBuf {
        self.support.join("notex.db")
    }

    fn suggested(&self) -> std::path::PathBuf {
        vault::suggested_vault(&self.home)
    }
}

fn write_note(path: &Path, title: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("# {title}\n\nbody\n")).unwrap();
}

/// Pump the worker until `done`, with the deadline every app test uses.
fn settle(state: &mut State, session: &Session, what: &str, done: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        pump(state, session.handle());
        if done(state) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!(
        "{what} never finished: load={:?} notes={} body={:?}",
        state.load,
        state.notes.len(),
        state.body
    );
}

fn open_state() -> State {
    let mut state = State::with_vault("/nowhere");
    state.ask_for_vault("/nowhere", None);
    state
}

#[test]
fn a_launch_with_no_config_and_no_default_folder_asks() {
    let home = Home::new();
    let launched = vault::resolve(&home.home, &home.support);
    assert_eq!(
        launched,
        vault::Launch::Ask {
            path: home.suggested(),
            notice: None,
        }
    );
}

#[test]
fn an_existing_suggested_folder_opens_without_asking() {
    let home = Home::new();
    fs::create_dir_all(home.suggested()).unwrap();
    assert_eq!(
        vault::resolve(&home.home, &home.support),
        vault::Launch::Open(home.suggested())
    );
}

#[test]
fn the_saved_folder_wins_over_the_suggested_one() {
    let home = Home::new();
    let saved = home.home.join("Obsidian").join("Personal");
    fs::create_dir_all(&saved).unwrap();
    fs::create_dir_all(home.suggested()).unwrap();
    vault::remember(&home.support, &saved).unwrap();

    assert_eq!(
        vault::resolve(&home.home, &home.support),
        vault::Launch::Open(saved)
    );
}

#[test]
fn a_saved_folder_that_is_gone_is_asked_for_and_never_replaced() {
    let home = Home::new();
    let saved = home.home.join("Volumes").join("Archive");
    vault::remember(&home.support, &saved).unwrap();
    // The suggested folder exists, and must not be used instead: that would
    // silently switch vaults when a drive is unplugged.
    fs::create_dir_all(home.suggested()).unwrap();

    match vault::resolve(&home.home, &home.support) {
        vault::Launch::Ask { path, notice } => {
            assert_eq!(path, saved);
            assert!(
                notice.unwrap().contains("drive connected"),
                "the reason names the likely cause"
            );
        }
        other => panic!("expected an ask, got {other:?}"),
    }
}

#[test]
fn a_saved_path_that_is_a_file_is_asked_for_with_the_file_intact() {
    let home = Home::new();
    let saved = home.home.join("notes.md");
    fs::write(&saved, "# not a vault\n").unwrap();
    vault::remember(&home.support, &saved).unwrap();

    assert_eq!(
        vault::resolve(&home.home, &home.support),
        vault::Launch::Ask {
            path: saved,
            notice: None,
        }
    );
}

#[test]
fn a_config_that_cannot_be_read_asks_instead_of_guessing() {
    let home = Home::new();
    fs::create_dir_all(&home.support).unwrap();
    fs::write(vault::config_path(&home.support), "{ not json").unwrap();

    match vault::resolve(&home.home, &home.support) {
        vault::Launch::Ask { path, notice } => {
            assert_eq!(path, home.suggested());
            assert!(
                notice.unwrap().contains("could not be read"),
                "the reason is a sentence"
            );
        }
        other => panic!("expected an ask, got {other:?}"),
    }
}

#[test]
fn the_choice_is_remembered_beside_the_database() {
    let home = Home::new();
    let chosen = home.home.join("Notes");
    vault::remember(&home.support, &chosen).unwrap();

    assert_eq!(
        vault::read_config(&home.support),
        Ok(Some(chosen)),
        "a round trip keeps the path"
    );
    assert_eq!(
        vault::read_config(&home.home),
        Ok(None),
        "no config, no choice"
    );
    assert!(
        vault::config_path(&home.support).starts_with(&home.support),
        "the config lives beside the database, never in the vault"
    );
}

#[test]
fn inspect_names_missing_empty_and_counted_folders() {
    let home = Home::new();
    let missing = home.home.join("later");
    assert_eq!(
        vault::inspect(&missing.display().to_string(), &home.support),
        VaultStatus::Missing
    );

    let empty = home.home.join("empty");
    fs::create_dir_all(&empty).unwrap();
    assert_eq!(
        vault::inspect(&empty.display().to_string(), &home.support),
        VaultStatus::Empty
    );

    write_note(&empty.join("a.md"), "A");
    write_note(&empty.join("folder").join("b.md"), "B");
    fs::write(empty.join("ignored.txt"), "not a note").unwrap();
    assert_eq!(
        vault::inspect(&empty.display().to_string(), &home.support),
        VaultStatus::Notes(2),
        "the count is what the index would see, recursively"
    );
}

#[test]
fn inspect_refuses_a_file_a_relative_path_and_the_apps_own_folder() {
    let home = Home::new();
    let file = home.home.join("notes.md");
    fs::write(&file, "x").unwrap();
    assert_eq!(
        vault::inspect(&file.display().to_string(), &home.support),
        VaultStatus::Unusable("That is a file, not a folder.".to_owned())
    );

    assert_eq!(
        vault::inspect("Documents/Notes", &home.support),
        VaultStatus::Unusable("That is not a full path. It has to start with /.".to_owned())
    );

    assert_eq!(
        vault::inspect(&home.support.display().to_string(), &home.support),
        VaultStatus::Unusable(
            "That folder holds openNoteX's own data. Pick another one.".to_owned()
        )
    );

    let inside = home.support.join("recovery");
    assert!(matches!(
        vault::inspect(&inside.display().to_string(), &home.support),
        VaultStatus::Unusable(_)
    ));
}

#[cfg(unix)]
#[test]
fn inspect_and_prepare_refuse_a_read_only_folder() {
    use std::os::unix::fs::PermissionsExt;

    let home = Home::new();
    let folder = home.home.join("locked");
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();

    let inspected = vault::inspect(&folder.display().to_string(), &home.support);
    let prepared = vault::prepare(&folder, &home.support, false);

    // Put the mode back before the temp dir is dropped.
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        inspected,
        VaultStatus::Unusable("That folder is read-only.".to_owned())
    );
    assert_eq!(prepared, Err("That folder is read-only.".to_owned()));
}

#[test]
fn prepare_creates_only_when_asked_and_only_outside_the_apps_folder() {
    let home = Home::new();
    let folder = home.home.join("Documents").join("Notes");

    assert_eq!(
        vault::prepare(&folder, &home.support, false),
        Err("This folder does not exist yet.".to_owned())
    );
    assert!(vault::prepare(&folder, &home.support, true).is_ok());
    assert!(folder.is_dir(), "Create folder made the folder");

    assert!(matches!(
        vault::prepare(&home.support.join("vault"), &home.support, true),
        Err(ref reason) if reason.contains("openNoteX's own data")
    ));
    assert!(
        !home.support.join("vault").exists(),
        "a refused create writes nothing"
    );
}

#[test]
fn create_folder_makes_the_vault_remember_it_and_open_it() {
    let home = Home::new();
    let folder = home.suggested();
    let mut state = open_state();

    let session = open_vault(
        &mut state,
        VaultIntent::Create(folder.display().to_string()),
        &home.support,
        &home.db(),
        Quiet,
    )
    .expect("the folder is created and opened");

    assert!(folder.is_dir());
    assert_eq!(vault::read_config(&home.support), Ok(Some(folder.clone())));
    assert!(state.vault_open, "the app has a vault now");
    assert!(!state.vault_dialog, "the dialog closed");
    assert_eq!(state.vault, folder.display().to_string());
    assert_eq!(state.load, Load::Loading);

    settle(&mut state, &session, "the first index", |state| {
        state.load == Load::Ready
    });

    // The acceptance case: Create folder, then ⌘N writes a real Markdown file.
    state.request_create();
    settle(&mut state, &session, "the first note", |state| {
        state.notes.len() == 1 && matches!(state.body, NoteBody::Ready)
    });

    let files: Vec<_> = fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    assert_eq!(files.len(), 1, "one note, one file: {files:?}");
    let text = fs::read_to_string(&files[0]).unwrap();
    assert!(text.contains("# Untitled"), "{text}");
}

#[test]
fn an_existing_folder_is_used_and_its_notes_are_indexed() {
    let home = Home::new();
    let folder = home.home.join("Obsidian").join("Personal");
    write_note(&folder.join("hello.md"), "Hello");
    write_note(&folder.join("nested").join("world.md"), "World");
    let mut state = open_state();

    let session = open_vault(
        &mut state,
        VaultIntent::Use(folder.display().to_string()),
        &home.support,
        &home.db(),
        Quiet,
    )
    .expect("an existing folder opens");

    settle(&mut state, &session, "the notes", |state| {
        state.load == Load::Ready && state.notes.len() == 2
    });
    let titles: Vec<_> = state.notes.iter().map(|note| note.title.as_str()).collect();
    assert_eq!(titles, ["Hello", "World"], "the folder is indexed as it is");
    assert_eq!(vault::read_config(&home.support), Ok(Some(folder)));
}

#[cfg(unix)]
#[test]
fn a_refused_folder_leaves_the_dialog_up_with_the_reason() {
    use std::os::unix::fs::PermissionsExt;

    let home = Home::new();
    let folder = home.home.join("locked");
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();

    let mut state = open_state();
    let refused = open_vault(
        &mut state,
        VaultIntent::Use(folder.display().to_string()),
        &home.support,
        &home.db(),
        Quiet,
    );

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();

    assert!(refused.is_err(), "a read-only folder is not opened");
    assert!(state.vault_dialog, "the dialog stays up");
    assert!(!state.vault_open);
    assert!(!state.vault_setup.busy, "Try again is possible");
    assert_eq!(
        state.vault_setup.notice.as_deref(),
        Some("That folder is read-only.")
    );
    assert_eq!(
        vault::read_config(&home.support),
        Ok(None),
        "a refused vault is not remembered"
    );
}

#[test]
fn a_folder_inside_the_apps_data_is_refused_before_anything_is_written() {
    let home = Home::new();
    let mut state = open_state();
    let inside = home.support.join("vault");

    let refused = open_vault(
        &mut state,
        VaultIntent::Create(inside.display().to_string()),
        &home.support,
        &home.db(),
        Quiet,
    );

    assert!(refused.is_err());
    assert!(
        state
            .vault_setup
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("openNoteX's own data"))
    );
    assert!(!inside.exists(), "nothing was created");
    assert_eq!(vault::read_config(&home.support), Ok(None));
}
