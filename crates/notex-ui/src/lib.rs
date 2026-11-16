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
pub use state::{Load, State};

pub const TITLE: &str = "openNoteX";
pub const SIZE: (f32, f32) = (1120.0, 740.0);
pub const MIN_SIZE: (f32, f32) = (880.0, 560.0);

/// One named state: the name `--scene` takes, and the change it makes.
pub type Scene = (&'static str, fn(&mut State));

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
