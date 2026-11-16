//! openNoteX: notes, tasks, habits and reminders in one local-first window.
//!
//! The binary owns the shell's lifetime and nothing else yet. The store, the
//! vault watcher and the reminder timer arrive in M1 and M4; the seam they will
//! use (`notex_store::StoreHandle`) already exists and is tested.
//!
//! Rune owns the command line: `--snap`, `--tree`, `--scene`, `--size`,
//! `--scale` and `--theme` are its flags, so `cargo xtask snap` drives the same
//! binary a person runs. `--vault` and `--route` are ours and arrive through
//! `rune::extra_args`.

use notex_ui::{MIN_SIZE, SCENES, SIZE, State, TITLE, on_key, view};
use rune::{App, AppSpec, Chrome};

fn main() {
    // Full-size content with the traffic lights over the sidebar's title bar row,
    // and drag regions so the content under them stays clickable.
    let chrome = Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 25.0)),
        drag_regions: true,
    };

    let spec = AppSpec {
        min_size: Some(MIN_SIZE),
        scenes: SCENES,
        on_key: Some(on_key),
        ..AppSpec::new(TITLE, SIZE, State::from_args, view)
    };

    App::new(spec).chrome(chrome).run();
}
