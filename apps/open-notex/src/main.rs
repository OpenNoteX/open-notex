//! openNoteX: notes, tasks, habits and reminders in one local-first window.
//!
//! The binary owns the window; `open_notex::setup` starts the store worker and
//! `open_notex::poll` drains it. Rune owns the command line: `--snap`, `--tree`,
//! `--scene`, `--size`, `--scale` and `--theme` are its flags, so `cargo xtask
//! snap` drives the same binary a person runs. `--vault` and `--route` are ours
//! and arrive through `rune::extra_args`.

use notex_ui::{MIN_SIZE, SCENES, SIZE, State, TITLE, on_key, view};
use open_notex::{flush, poll, setup};
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
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        ..AppSpec::new(TITLE, SIZE, State::from_args, view)
    };

    App::new(spec)
        .chrome(chrome)
        // Closing the red button keeps the app, its state and an unsaved draft
        // alive, as every other Mac app does; ⌘Q still flushes through on_quit.
        // A closed window holds no GPU memory.
        .keep_running()
        .release_gpu_when_closed()
        .on_close(flush)
        .on_quit(flush)
        .run();
}
