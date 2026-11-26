//! The window's state.
//!
//! `view()` is a pure function of this struct: no database, no filesystem, no
//! clock. Everything here is cheap to clone and cheap to compare, because Rune
//! re-runs the view on every event.

use std::collections::VecDeque;
use std::ffi::OsString;

use notex_time::checklist::{self, Bucket};
use rune::{PointerPress, TextEdit, Tx};
use rune_kit::shell::PaneState;

use crate::routes::Route;

/// How long the editor waits after the last keystroke before saving.
pub const AUTOSAVE_IDLE: f64 = 0.7;
/// The longest a continuous run of typing goes unsaved.
pub const AUTOSAVE_MAX: f64 = 5.0;

/// What the data behind the current route looks like.
///
/// The skeleton has no store yet, so every route is `Ready` with no items. The
/// other two variants exist now because empty, loading and failed states are the
/// hardest thing to retrofit once a screen has content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Load {
    #[default]
    Ready,
    Loading,
    Failed(String),
}

/// How long the undo toast holds a completed or deleted task.
pub const UNDO_SECONDS: f64 = 8.0;

/// Where a typed line goes back to when the store refuses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddTarget {
    /// The add row at the top of Tasks.
    Main,
    /// The add-step field under this task.
    Step(String),
}

/// A task typed in the add row that the store has not answered yet. The row is
/// already in the checklist under `temp_id`; the answer replaces it in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAdd {
    /// The row's id until the store's ULID arrives. Never sent anywhere.
    pub temp_id: String,
    /// The line exactly as typed, so a refusal can put it back in the field.
    pub text: String,
    /// Which field the line came from.
    pub target: AddTarget,
}

/// The add-a-step field under one task in the tree.
pub struct StepInput {
    pub parent_id: String,
    pub edit: TextEdit,
}

/// A row in the notes list. The body is not here: it is read on demand, so a long
/// vault never sits in memory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NoteRow {
    pub id: String,
    pub title: String,
    /// Frontmatter tags, without the leading `#`.
    pub tags: Vec<String>,
    /// The file's modification time, in nanoseconds. Home's "Recent notes" is
    /// this, not the time the index last looked at the file.
    pub modified: i64,
}

impl NoteRow {
    pub fn new(id: impl Into<String>, title: impl Into<String>, tags: &[&str]) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            modified: 0,
        }
    }

    /// The file's mtime, for sorting Home's recent notes.
    pub fn modified(mut self, ns: i64) -> Self {
        self.modified = ns;
        self
    }

    /// The second line of the row: `work · review`, or the empty string.
    pub fn tag_line(&self) -> String {
        self.tags.join(" · ")
    }
}

/// One task in the checklist, joined with the project it is filed into.
///
/// The shell does not depend on the store: the app maps `notex_store::Task`
/// into this. `completed_day` is mapped there too, so the view never reads a
/// timezone or a clock.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub done: bool,
    pub list_id: Option<String>,
    pub list_name: Option<String>,
    pub list_color: Option<String>,
    /// `YYYY-MM-DD`, or `None` for a task with no date.
    pub due_day: Option<String>,
    pub completed_at: Option<i64>,
    /// The local day `completed_at` falls on, for the archive's groups.
    pub completed_day: Option<String>,
    /// The task this is a step of. A step is an ordinary task row: it has the
    /// same title, notes and done flag, and it only differs in where the
    /// checklist puts it.
    pub parent_id: Option<String>,
}

impl TaskRow {
    /// The due chip, when the task has a date and this is a real day.
    pub fn due(&self, today: &str) -> Option<checklist::Due> {
        self.due_day
            .as_deref()
            .map(|day| checklist::due(day, today))
    }

    /// A step is a task with a parent, so the checklist hides it from the top
    /// level and the counts.
    pub fn is_step(&self) -> bool {
        self.parent_id.is_some()
    }
}

/// A project, in filter-chip order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectRow {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

/// The local clock Home draws. The app fills it from `poll`; the view only reads
/// it, so a frame never asks the system what time it is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Now {
    /// `YYYY-MM-DD`, the day tasks are grouped against.
    pub day: String,
    /// `14:32`.
    pub time: String,
    /// `Thursday 19 November`.
    pub date: String,
}

/// One task being renamed in place.
pub struct TaskEdit {
    pub id: String,
    pub edit: TextEdit,
}

/// One task's notes, open under the expanded row.
pub struct TaskNote {
    pub id: String,
    pub edit: TextEdit,
    /// The last text sent; the row shows "Saved" until it changes again.
    pub saving: bool,
}

/// The eight-second way back from a completion, a delete or a project delete.
#[derive(Debug, Clone, PartialEq)]
pub enum Undo {
    /// A completed task (`done`) or a deleted one (`!done`).
    Task {
        id: String,
        done: bool,
        /// The steps this completion closed, at every depth. Undo reopens
        /// them, so a check that took three steps back takes them back too.
        closed: Vec<String>,
        message: String,
        /// Seconds left. `tick` counts this down and drops the toast at zero.
        left: f64,
    },
    /// A deleted project and the ids it released, so undo can refile them.
    Project {
        id: String,
        task_ids: Vec<String>,
        message: String,
        detail: String,
        left: f64,
    },
}

impl Undo {
    pub fn message(&self) -> &str {
        match self {
            Undo::Task { message, .. } | Undo::Project { message, .. } => message,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Undo::Task { .. } => "Undo for a few seconds.",
            Undo::Project { detail, .. } => detail,
        }
    }

    pub fn left(&self) -> f64 {
        match self {
            Undo::Task { left, .. } | Undo::Project { left, .. } => *left,
        }
    }

    fn left_mut(&mut self) -> &mut f64 {
        match self {
            Undo::Task { left, .. } | Undo::Project { left, .. } => left,
        }
    }
}

/// One project's chip menu: Rename, Delete project.
#[derive(Debug, Clone, PartialEq)]
pub struct ChipMenu {
    pub id: String,
    /// Where a secondary press landed; `None` when the chip's `⋯` opened it, so
    /// the menu anchors under the chip instead of under the pointer.
    pub point: Option<PointerPress>,
}

/// One project being renamed in place on its chip.
pub struct ProjectEdit {
    pub id: String,
    pub edit: TextEdit,
}

/// What the detail pane holds for the selected note.
///
/// `Ready` means the editor has the body: [`State::draft`] owns the text, so it
/// is never stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NoteBody {
    #[default]
    None,
    Loading,
    Ready,
    Failed(String),
}

/// The open note's editor. One per selected note; dropped when the note closes.
pub struct Draft {
    pub id: String,
    /// The file's whole Markdown source, frontmatter excluded. `# Title` is part
    /// of it, so editing the heading is how a note is renamed.
    pub edit: TextEdit,
    /// The file hash the text was read from. A different file on disk is a
    /// conflict, not something to overwrite.
    pub base_hash: u64,
    /// Why the last save was refused because the file changed underneath.
    pub conflict: Option<String>,
    /// Why the last save failed (a read-only vault, a full disk).
    pub error: Option<String>,
    /// Bumped on every user edit, echoed by the worker on save.
    token: u64,
    /// The highest token the worker has confirmed. Dirty is `token > acked`.
    acked: u64,
    /// Seconds since the last keystroke, and since the last save.
    idle: f64,
    since_save: f64,
    /// A save is out; no second one goes until it answers.
    saving: bool,
    /// The next save overwrites whatever is on disk (the user chose "Overwrite").
    force: bool,
}

/// What the editor's status line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftStatus {
    Conflict,
    Failed,
    Saving,
    Edited,
    Saved,
}

impl Draft {
    pub fn new(id: &str, body: String, base_hash: u64) -> Self {
        Self {
            id: id.to_owned(),
            edit: TextEdit::new(body),
            base_hash,
            conflict: None,
            error: None,
            token: 0,
            acked: 0,
            idle: AUTOSAVE_IDLE,
            since_save: 0.0,
            saving: false,
            force: false,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.token > self.acked
    }

    pub fn status(&self) -> DraftStatus {
        if self.conflict.is_some() {
            DraftStatus::Conflict
        } else if self.error.is_some() {
            DraftStatus::Failed
        } else if self.saving {
            DraftStatus::Saving
        } else if self.is_dirty() {
            DraftStatus::Edited
        } else {
            DraftStatus::Saved
        }
    }
}

/// A save the app should send. The view never sends it; `poll` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    pub id: String,
    pub body: String,
    pub base_hash: u64,
    pub token: u64,
    pub force: bool,
}

/// What the app should do next. Drained by `poll`, never by the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Save(SaveRequest),
    Open(String),
    /// Make a new note in the vault.
    Create,
    /// Move a note's file to the trash and drop it from the list.
    Delete(String),
    /// Ask GitHub for the latest release. Starts a process, so it is an action
    /// rather than something the view does.
    CheckForUpdates,
    /// Open a release page in the browser, by version.
    ShowRelease(String),
    /// Add a task, creating its project when the name is new.
    CreateTask {
        title: String,
        project: Option<String>,
        due_day: Option<String>,
    },
    SetTaskTitle {
        id: String,
        title: String,
    },
    SetTaskNotes {
        id: String,
        notes: String,
    },
    /// Set or clear the all-day due date. `None` clears it.
    SetTaskDue {
        id: String,
        due_day: Option<String>,
    },
    SetTaskProject {
        id: String,
        list_id: Option<String>,
    },
    SetTaskDone {
        id: String,
        done: bool,
    },
    /// Add a step under a task; the step inherits the task's project.
    CreateSubtask {
        parent_id: String,
        title: String,
    },
    /// Soft delete; the undo toast puts the same row back.
    DeleteTask(String),
    RestoreTask(String),
    CreateProject(String),
    /// Rename a project in place. A name another project holds is refused.
    SetProjectName {
        id: String,
        name: String,
    },
    /// Delete a project; its tasks are released to no project.
    DeleteProject(String),
    /// Undo a project delete and refile the tasks it released.
    RestoreProject {
        id: String,
        task_ids: Vec<String>,
    },
}

/// Where the update check stands. Settings shows it, and only a release newer
/// than this build gets a banner: a check that failed is a line of text, not an
/// alarm, because the app works exactly the same without one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Update {
    /// Nothing has been asked yet. The default, because this app calls out to
    /// nothing unless a person asks it to.
    #[default]
    Unchecked,
    Checking,
    /// A release newer than this build, as `0.1.0`.
    Available(String),
    /// This build is the latest release.
    Current,
    /// The latest release is older than this build: a development build, or a
    /// release that was withdrawn. Nothing to offer, and saying "up to date"
    /// would be the wrong sentence.
    Ahead,
    /// Why the check could not answer, in a sentence.
    Failed(String),
}

impl Update {
    /// The line under "Updates" in Settings.
    pub fn summary(&self) -> String {
        match self {
            Update::Unchecked => "Not checked".to_owned(),
            Update::Checking => "Checking…".to_owned(),
            Update::Available(version) => format!("{version} is available"),
            Update::Current => "Up to date".to_owned(),
            Update::Ahead => "Ahead of the latest release".to_owned(),
            Update::Failed(message) => message.clone(),
        }
    }

    /// The version a banner should offer, when there is one.
    pub fn offered(&self) -> Option<&str> {
        match self {
            Update::Available(version) => Some(version),
            _ => None,
        }
    }
}

/// What the folder the vault dialog is showing actually is.
///
/// The dialog is the only place that asks, so it is also the only place that has
/// to name the answer. `Unusable` carries the sentence the person reads, because
/// only the layer that looked at the disk knows why it said no.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum VaultStatus {
    #[default]
    Missing,
    Empty,
    Notes(usize),
    Unusable(String),
}

impl VaultStatus {
    /// The line under the path.
    pub fn summary(&self) -> String {
        match self {
            VaultStatus::Missing => "This folder does not exist yet.".to_owned(),
            VaultStatus::Empty => "This folder is empty. It becomes your vault.".to_owned(),
            VaultStatus::Notes(1) => "1 note here.".to_owned(),
            VaultStatus::Notes(count) => format!("{count} notes here."),
            VaultStatus::Unusable(reason) => reason.clone(),
        }
    }

    /// The primary button. Only a folder that does not exist is created; a
    /// folder that is there is used as it is.
    pub fn primary_label(&self) -> &'static str {
        match self {
            VaultStatus::Missing => "Create folder",
            VaultStatus::Empty | VaultStatus::Notes(_) | VaultStatus::Unusable(_) => {
                "Use this folder"
            }
        }
    }

    /// Whether a press on the primary button can do anything.
    pub fn can_open(&self) -> bool {
        !matches!(self, VaultStatus::Unusable(_))
    }
}

/// The vault dialog's own state. `path` is what it would use, so the default is
/// always a real value, even after "Not now".
pub struct VaultSetup {
    pub path: String,
    pub status: VaultStatus,
    /// Why the last attempt failed, in a sentence. Shown beside the status line.
    pub notice: Option<String>,
    /// An open or create is out; the buttons wait.
    pub busy: bool,
    /// The app has not looked at `path` since it changed.
    pub needs_check: bool,
}

impl VaultSetup {
    fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            status: VaultStatus::Missing,
            notice: None,
            busy: false,
            needs_check: false,
        }
    }
}

/// What the app should do about the vault. Separate from [`Action`] because
/// opening a vault replaces the session; `poll` owns both, and the view neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultIntent {
    /// Make the folder, then open it. The primary button for a missing path.
    Create(String),
    /// Open a folder that is already there.
    Use(String),
}

pub struct State {
    /// What the content pane is showing.
    pub route: Route,
    /// Sidebar width and open state, owned by the app as rune-kit requires.
    pub sidebar: PaneState,
    /// The detail pane's width and open state.
    pub detail: PaneState,
    pub load: Load,
    /// The notes list, alphabetical, from the index.
    pub notes: Vec<NoteRow>,
    /// The local clock Home draws, filled by `poll`.
    pub now: Now,
    /// The checklist, in store order (open first, then by day).
    pub tasks: Vec<TaskRow>,
    /// Every project, in chip order.
    pub projects: Vec<ProjectRow>,
    /// `Some(project id)` narrows the checklist; `None` is "All".
    pub project_filter: Option<String>,
    /// The add row's text. Kept in `State`, so the view does not own a field.
    pub add_task: TextEdit,
    /// The `+ Project` field, open while it is `Some`.
    pub add_project: Option<TextEdit>,
    /// The project whose chip menu is open, and where it opened.
    pub chip_menu: Option<ChipMenu>,
    /// The one project being renamed in place.
    pub project_edit: Option<ProjectEdit>,
    /// The one task being renamed in place.
    pub task_edit: Option<TaskEdit>,
    /// The expanded row's notes field.
    pub task_note: Option<TaskNote>,
    /// The add-step field under one expanded task.
    pub step_input: Option<StepInput>,
    /// The row whose notes and dates are open under it.
    pub expanded: Option<String>,
    /// The archive is showing done tasks instead of the checklist.
    pub archive: bool,
    /// The last completion or delete, while its toast is up.
    pub undo: Option<Undo>,
    /// Why the last task write failed, in a sentence.
    pub task_error: Option<String>,
    /// Task commands waiting for `take_action`. Handlers push; the view never sends.
    pending: VecDeque<Action>,
    /// Tasks typed in the add row whose `CreateTask` the store has not answered,
    /// oldest first. The checklist shows them under a temporary id.
    pub pending_adds: VecDeque<PendingAdd>,
    /// Counts temporary ids, so two queued adds never share a row key.
    next_temp: u64,
    /// The selected note's id; `None` when the list has no selection.
    pub selected: Option<String>,
    /// The selected note's body, or why it is not here yet.
    pub body: NoteBody,
    /// The open note's editor, once the body has arrived.
    pub draft: Option<Draft>,
    /// A row the user picked while the editor had unsaved text. Opened after the
    /// save lands, so typing is never thrown away.
    pub pending_open: Option<String>,
    /// New note and delete also wait for a dirty draft, for the same reason: the
    /// note being left must reach its file before the screen changes.
    pending_new: bool,
    pending_delete: Option<String>,
    /// The note a confirmation dialog is asking about.
    pub confirm_delete: Option<String>,
    /// The last create or delete that a worker refused, for the list pane.
    pub action_error: Option<String>,
    /// Where the update check stands. `Unchecked` until someone asks.
    pub update: Update,
    /// A check was asked for and has not been sent yet.
    update_requested: bool,
    /// A release page was asked for, by version.
    release_requested: Option<String>,
    /// `⌘S` or the Save button asked for a save now.
    save_requested: bool,
    /// The last `poll` clock, for `tick`. Seconds since the app started.
    clock: f64,
    /// Where notes live, for display. The app opens it; the UI only shows it.
    pub vault: String,
    /// A vault is open, so creating, deleting and listing can happen. False only
    /// between launch and the person choosing or creating a folder.
    pub vault_open: bool,
    /// The setup dialog is up. One flag for first launch, the empty list, Settings
    /// and any action that needs a vault.
    pub vault_dialog: bool,
    /// The dialog's path, status and last refusal.
    pub vault_setup: VaultSetup,
    /// An open or create the app has not handled yet.
    vault_intent: Option<VaultIntent>,
    /// Build identity, shown in Settings.
    pub version: &'static str,
}

impl State {
    pub fn new() -> Self {
        Self::with_vault(default_vault())
    }

    pub fn with_vault(vault: impl Into<String>) -> Self {
        let vault = vault.into();
        Self {
            route: Route::Home,
            sidebar: PaneState::new(216.0).range(180.0, 320.0),
            detail: PaneState::new(440.0).range(320.0, 720.0),
            load: Load::Ready,
            notes: Vec::new(),
            now: Now::default(),
            tasks: Vec::new(),
            projects: Vec::new(),
            project_filter: None,
            add_task: TextEdit::new(""),
            add_project: None,
            chip_menu: None,
            project_edit: None,
            task_edit: None,
            task_note: None,
            step_input: None,
            expanded: None,
            archive: false,
            undo: None,
            task_error: None,
            pending: VecDeque::new(),
            pending_adds: VecDeque::new(),
            next_temp: 0,
            selected: None,
            body: NoteBody::None,
            draft: None,
            pending_open: None,
            pending_new: false,
            pending_delete: None,
            confirm_delete: None,
            action_error: None,
            update: Update::Unchecked,
            update_requested: false,
            release_requested: None,
            save_requested: false,
            clock: 0.0,
            vault: vault.clone(),
            vault_open: true,
            vault_dialog: false,
            vault_setup: VaultSetup::new(vault),
            vault_intent: None,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// Replace the list. A selection whose note disappeared (deleted or renamed
    /// elsewhere) is cleared rather than left pointing at nothing.
    pub fn set_notes(&mut self, notes: Vec<NoteRow>) {
        // A confirmation for a note that is not in the list has nothing to ask
        // about: the row went away while the dialog was open.
        if self
            .confirm_delete
            .as_ref()
            .is_some_and(|id| !notes.iter().any(|note| &note.id == id))
        {
            self.confirm_delete = None;
        }
        if let Some(selected) = &self.selected
            && !notes.iter().any(|note| &note.id == selected)
        {
            self.selected = None;
            self.body = NoteBody::None;
            self.draft = None;
            self.pending_open = None;
        }
        self.notes = notes;
    }

    // ---------------------------------------------------------------- tasks

    /// The clock Home draws. Called by `poll`; repeated calls with the same
    /// strings are cheap and change nothing.
    pub fn set_clock(&mut self, now: Now) -> bool {
        if self.now == now {
            return false;
        }
        self.now = now;
        true
    }

    /// Replace the checklist. Editors for rows that are gone are dropped, so a
    /// delete from another window cannot leave an input bound to nothing.
    pub fn set_tasks(&mut self, tasks: Vec<TaskRow>) {
        self.tasks = tasks;
        self.prune_task_editors();
    }

    pub fn set_projects(&mut self, projects: Vec<ProjectRow>) {
        if let Some(id) = &self.project_filter
            && !projects.iter().any(|project| &project.id == id)
        {
            self.project_filter = None;
        }
        self.projects = projects;
    }

    /// A task after a write. The row is replaced in place, so the list does not
    /// jump while it is being read.
    pub fn task_changed(&mut self, task: TaskRow) {
        if !self.tasks.iter().any(|row| row.id == task.id)
            && let Some(pending) = self.pending_adds.pop_front()
        {
            // The store's answer to a queued create: its row takes the optimistic
            // row's place, so the checklist never shows the same task twice.
            let real_id = task.id.clone();
            match self.tasks.iter_mut().find(|row| row.id == pending.temp_id) {
                Some(row) => *row = task,
                // The row went away before its answer (a refusal, a race): the
                // answer is still a real task, so it arrives.
                None => self.tasks.push(task),
            }
            // A completion that ran before this answer may have put the
            // temporary id in the toast. Undo has to name the real row.
            if let Some(Undo::Task { closed, .. }) = &mut self.undo {
                for id in closed.iter_mut() {
                    if *id == pending.temp_id {
                        *id = real_id.clone();
                    }
                }
            }
        } else if let Some(row) = self.tasks.iter_mut().find(|row| row.id == task.id) {
            *row = task;
        } else {
            self.tasks.push(task);
        }
        self.sort_tasks();
        // The notes field says "Saving…" until the write that carried its text
        // comes back; the same text means this is that answer.
        if let Some(note) = &mut self.task_note
            && let Some(row) = self.tasks.iter().find(|row| row.id == note.id)
            && row.notes == note.edit.text
        {
            note.saving = false;
        }
    }

    /// A row that the store has not answered yet. Its temporary id is not one
    /// the store knows, so the controls on it wait for the real row.
    pub fn is_pending_add(&self, id: &str) -> bool {
        self.pending_adds.iter().any(|add| add.temp_id == id)
    }

    /// The store refused a new task. Its row goes and the line comes back to
    /// the field it was typed in, so nothing typed has to be typed again.
    pub fn add_failed(&mut self, error: impl Into<String>) {
        if let Some(pending) = self.pending_adds.pop_front() {
            self.tasks.retain(|task| task.id != pending.temp_id);
            match pending.target {
                AddTarget::Main => self.add_task.replace_text(pending.text),
                AddTarget::Step(parent_id) => {
                    let mut edit = TextEdit::new(pending.text);
                    edit.focused = true;
                    self.step_input = Some(StepInput { parent_id, edit });
                }
            }
            self.prune_task_editors();
        }
        self.task_error = Some(error.into());
    }

    /// The row is gone (deleted here or elsewhere).
    pub fn task_gone(&mut self, id: &str) {
        self.tasks.retain(|task| task.id != id);
        if self.task_edit.as_ref().is_some_and(|edit| edit.id == id) {
            self.task_edit = None;
        }
        if self.expanded.as_deref() == Some(id) {
            self.expanded = None;
            self.task_note = None;
        }
    }

    pub fn list_changed(&mut self, project: ProjectRow) {
        if self
            .project_edit
            .as_ref()
            .is_some_and(|edit| edit.id == project.id)
        {
            self.project_edit = None;
        }
        // Rows carry their project's name and dot, so a rename shows on every
        // task filed in it without waiting for a relist.
        for task in &mut self.tasks {
            if task.list_id.as_deref() == Some(project.id.as_str()) {
                task.list_name = Some(project.name.clone());
                task.list_color = project.color.clone();
            }
        }
        match self.projects.iter_mut().find(|row| row.id == project.id) {
            Some(row) => *row = project,
            None => self.projects.push(project),
        }
        self.projects.sort_by_key(|row| row.name.to_lowercase());
    }

    pub fn task_failed(&mut self, error: impl Into<String>) {
        self.task_error = Some(error.into());
    }

    /// Open tasks, in store order. Steps are not top-level tasks: they show
    /// under the row they belong to.
    pub fn open_tasks(&self) -> impl Iterator<Item = &TaskRow> {
        self.tasks
            .iter()
            .filter(|task| !task.done && !task.is_step())
    }

    pub fn open_task_count(&self) -> usize {
        self.open_tasks().count()
    }

    pub fn done_task_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|task| task.done && !task.is_step())
            .count()
    }

    /// The steps of one task, in the order they were added.
    pub fn steps_of(&self, parent_id: &str) -> Vec<&TaskRow> {
        self.tasks
            .iter()
            .filter(|task| task.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    /// How many rows deep a task sits: a task is 1, its steps 2, their steps
    /// 3. Walks `parent_id` so a step can hold steps up to
    /// [`notex_core::MAX_TASK_DEPTH`], and stops if the data ever cycles.
    pub fn depth_of(&self, id: &str) -> usize {
        let mut depth = 0;
        let mut cursor = Some(id.to_owned());
        while let Some(current) = cursor {
            depth += 1;
            if depth > notex_core::MAX_TASK_DEPTH {
                break;
            }
            cursor = self
                .tasks
                .iter()
                .find(|task| task.id == current)
                .and_then(|task| task.parent_id.clone());
        }
        depth
    }

    /// How many of a task's steps are still open, for the row's `2 of 3` line.
    pub fn open_steps_of(&self, parent_id: &str) -> usize {
        self.steps_of(parent_id)
            .into_iter()
            .filter(|step| !step.done)
            .count()
    }

    /// The checklist after the project chip: open tasks only.
    pub fn visible_tasks(&self) -> impl Iterator<Item = &TaskRow> {
        let filter = self.project_filter.as_deref();
        self.open_tasks()
            .filter(move |task| filter.is_none_or(|id| task.list_id.as_deref() == Some(id)))
    }

    /// Which group a task belongs to today.
    pub fn bucket_of(&self, task: &TaskRow) -> Bucket {
        checklist::bucket(task.due_day.as_deref(), &self.now.day)
    }

    /// Open tasks in one group of the checklist.
    pub fn tasks_in(&self, bucket: Bucket) -> Vec<&TaskRow> {
        self.visible_tasks()
            .filter(|task| self.bucket_of(task) == bucket)
            .collect()
    }

    /// Done tasks, newest first, for the archive.
    pub fn archived(&self) -> Vec<&TaskRow> {
        let mut done: Vec<&TaskRow> = self
            .tasks
            .iter()
            .filter(|task| task.done && !task.is_step())
            .collect();
        done.sort_by(|a, b| {
            b.completed_at
                .cmp(&a.completed_at)
                .then_with(|| a.title.cmp(&b.title))
        });
        done
    }

    /// The most recently written notes, newest first. Empty when the index has
    /// no notes or no mtimes (a test that did not set them).
    pub fn recent_notes(&self, limit: usize) -> Vec<&NoteRow> {
        let mut notes: Vec<&NoteRow> = self.notes.iter().collect();
        notes.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.title.cmp(&b.title))
        });
        notes.truncate(limit);
        notes
    }

    /// Return in the add row. The line is parsed, the row shows at once, the
    /// field is cleared and the command is queued. An empty line does nothing.
    pub fn add_task_now(&mut self) {
        let text = self.add_task.text.clone();
        let Some(input) = notex_core::parse_task_input(&text) else {
            return;
        };
        let project = input.project.or_else(|| {
            // A filtered checklist adds into the project it is showing.
            self.project_filter.as_ref().and_then(|id| {
                self.projects
                    .iter()
                    .find(|project| &project.id == id)
                    .map(|project| project.name.clone())
            })
        });
        // The row is here before the store answers, so Return reads as done.
        // `task_changed` swaps the store's row in under the same place, and
        // `add_failed` takes this one away and puts the line back.
        // A project that already exists gets its chip at once; one the command
        // is about to create has no id to show until the answer.
        let known = project.as_ref().and_then(|name| {
            self.projects
                .iter()
                .find(|project| project.name.eq_ignore_ascii_case(name))
        });
        let (list_id, list_color) = match known {
            Some(project) => (Some(project.id.clone()), project.color.clone()),
            None => (None, None),
        };
        let temp_id = format!("new-{}", self.next_temp);
        self.next_temp += 1;
        self.tasks.push(TaskRow {
            id: temp_id.clone(),
            title: input.title.clone(),
            notes: String::new(),
            done: false,
            list_id,
            list_name: project.clone(),
            list_color,
            due_day: None,
            completed_at: None,
            completed_day: None,
            parent_id: None,
        });
        self.pending_adds.push_back(PendingAdd {
            temp_id,
            text,
            target: AddTarget::Main,
        });
        self.sort_tasks();
        self.add_task.replace_text("");
        self.task_error = None;
        self.pending.push_back(Action::CreateTask {
            title: input.title,
            project,
            due_day: None,
        });
    }

    /// Open the add-step field under one task. Only one field is open at a
    /// time, so two rows never fight for the keyboard, and a tree stops at
    /// [`notex_core::MAX_TASK_DEPTH`] levels.
    pub fn begin_step(&mut self, parent_id: &str) {
        if self.is_pending_add(parent_id) {
            return;
        }
        if self.depth_of(parent_id) >= notex_core::MAX_TASK_DEPTH {
            return;
        }
        if self
            .step_input
            .as_ref()
            .is_some_and(|step| step.parent_id == parent_id)
        {
            return;
        }
        let mut edit = TextEdit::new("");
        edit.focused = true;
        self.step_input = Some(StepInput {
            parent_id: parent_id.to_owned(),
            edit,
        });
    }

    pub fn cancel_step(&mut self) {
        self.step_input = None;
    }

    /// Return in a step's field. The step shows under its task at once, the
    /// field stays open and empty, and the command is queued. A step takes no
    /// project and no date: it belongs to its task.
    pub fn add_step_now(&mut self) {
        let Some(step) = &self.step_input else {
            return;
        };
        let parent_id = step.parent_id.clone();
        let title = step.edit.text.trim().to_owned();
        if title.is_empty() {
            return;
        }
        let Some(parent) = self.tasks.iter().find(|task| task.id == parent_id).cloned() else {
            self.step_input = None;
            return;
        };
        if self.depth_of(&parent_id) >= notex_core::MAX_TASK_DEPTH {
            return;
        }
        let temp_id = format!("new-{}", self.next_temp);
        self.next_temp += 1;
        self.tasks.push(TaskRow {
            id: temp_id.clone(),
            title: title.clone(),
            notes: String::new(),
            done: false,
            // A step is filed with its task, so the chip and the filter agree.
            list_id: parent.list_id.clone(),
            list_name: parent.list_name.clone(),
            list_color: parent.list_color.clone(),
            due_day: None,
            completed_at: None,
            completed_day: None,
            parent_id: Some(parent_id.clone()),
        });
        self.pending_adds.push_back(PendingAdd {
            temp_id,
            text: title.clone(),
            target: AddTarget::Step(parent_id.clone()),
        });
        self.sort_tasks();
        if let Some(step) = &mut self.step_input {
            step.edit.replace_text("");
        }
        self.task_error = None;
        self.pending
            .push_back(Action::CreateSubtask { parent_id, title });
    }

    /// The `+ Project` chip: open its one-line field.
    pub fn begin_project(&mut self) {
        self.add_project.get_or_insert_with(|| TextEdit::new(""));
    }

    pub fn cancel_project(&mut self) {
        self.add_project = None;
    }

    /// Return in the `+ Project` field.
    pub fn commit_project(&mut self) {
        let Some(edit) = &self.add_project else {
            return;
        };
        let name = edit.text.trim().to_owned();
        if name.is_empty() {
            self.add_project = None;
            return;
        }
        self.pending.push_back(Action::CreateProject(name));
        // The chips answer with `ListChanged`; the field closes now, not later,
        // so the next Return in the add row is a task.
        self.add_project = None;
    }

    pub fn set_project_filter(&mut self, id: Option<String>) {
        self.project_filter = id;
    }

    /// Move to a route. Leaving Tasks drops a chip menu or a rename field, so
    /// coming back never shows one the person had forgotten about.
    pub fn go(&mut self, route: Route) {
        if self.route != route {
            self.chip_menu = None;
            self.project_edit = None;
        }
        self.route = route;
    }

    /// Show or hide the archive. The chip row is not on screen in it, so an
    /// open chip menu goes with it.
    pub fn toggle_archive(&mut self) {
        self.archive = !self.archive;
        self.chip_menu = None;
        self.project_edit = None;
    }

    /// Open a project's chip menu: under the chip from `⋯`, or at the pointer
    /// from a secondary press.
    pub fn open_chip_menu(&mut self, id: &str, point: Option<PointerPress>) {
        self.project_edit = None;
        self.chip_menu = Some(ChipMenu {
            id: id.to_owned(),
            point,
        });
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = None;
    }

    /// Rename from the chip menu: the chip itself becomes a field, with the
    /// caret at the end and the old name selected, so typing replaces it.
    pub fn begin_project_edit(&mut self, id: &str) {
        let Some(project) = self.projects.iter().find(|project| project.id == id) else {
            return;
        };
        let mut edit = TextEdit::new(project.name.clone());
        edit.focused = true;
        // The old name is selected, so typing replaces it.
        edit.anchor = 0;
        edit.focus = edit.text.len();
        self.chip_menu = None;
        self.project_edit = Some(ProjectEdit {
            id: id.to_owned(),
            edit,
        });
    }

    pub fn cancel_project_edit(&mut self) {
        self.project_edit = None;
    }

    /// Return in a chip's field. The field stays open until the store answers,
    /// so a refused name (one another project holds) leaves it there to correct.
    pub fn commit_project_edit(&mut self) {
        let Some(project) = &self.project_edit else {
            return;
        };
        let name = project.edit.text.trim().to_owned();
        if name.is_empty() {
            self.task_error = Some("A project needs a name.".to_owned());
            return;
        }
        self.task_error = None;
        if self
            .projects
            .iter()
            .any(|other| other.id == project.id && other.name == name)
        {
            // Nothing to write: the chip already says this.
            self.project_edit = None;
            return;
        }
        self.pending.push_back(Action::SetProjectName {
            id: project.id.clone(),
            name,
        });
    }

    /// Delete a project from its chip menu. No dialog: the tasks are released,
    /// never deleted, and the toast puts the project and their filing back.
    pub fn delete_project(&mut self, id: &str) {
        let Some(project) = self.projects.iter().find(|project| project.id == id) else {
            return;
        };
        let task_ids: Vec<String> = self
            .tasks
            .iter()
            .filter(|task| task.list_id.as_deref() == Some(id))
            .map(|task| task.id.clone())
            .collect();
        self.task_error = None;
        self.chip_menu = None;
        self.project_edit = None;
        self.undo = Some(Undo::Project {
            id: id.to_owned(),
            detail: match task_ids.len() {
                0 => "No tasks were filed in it.".to_owned(),
                1 => "1 task moved to No project".to_owned(),
                count => format!("{count} tasks moved to No project"),
            },
            message: format!("\u{201c}{}\u{201d} deleted", project.name),
            task_ids,
            left: UNDO_SECONDS,
        });
        self.pending.push_back(Action::DeleteProject(id.to_owned()));
    }

    /// The store deleted a project: drop the chip, leave the filter on All and
    /// unfile the tasks it released.
    pub fn project_gone(&mut self, id: &str) {
        self.projects.retain(|project| project.id != id);
        if self.project_filter.as_deref() == Some(id) {
            self.project_filter = None;
        }
        if self.chip_menu.as_ref().is_some_and(|menu| menu.id == id) {
            self.chip_menu = None;
        }
        if self.project_edit.as_ref().is_some_and(|edit| edit.id == id) {
            self.project_edit = None;
        }
        for task in &mut self.tasks {
            if task.list_id.as_deref() == Some(id) {
                task.list_id = None;
                task.list_name = None;
                task.list_color = None;
            }
        }
    }

    /// A project came back from a delete. The tasks the store refiled get their
    /// project id back here, so the next frame shows them filed again.
    pub fn list_restored(&mut self, project: ProjectRow, task_ids: &[String]) {
        for task in &mut self.tasks {
            if task.list_id.is_none() && task_ids.iter().any(|id| id == &task.id) {
                task.list_id = Some(project.id.clone());
            }
        }
        self.list_changed(project);
    }

    /// Click the title: rename it in place, with the caret where it belongs.
    pub fn begin_edit(&mut self, id: &str) {
        if self.is_pending_add(id) {
            return;
        }
        let Some(task) = self.tasks.iter().find(|task| task.id == id) else {
            return;
        };
        let mut edit = TextEdit::new(task.title.clone());
        edit.focused = true;
        self.task_edit = Some(TaskEdit {
            id: id.to_owned(),
            edit,
        });
    }

    /// Return in the rename field. An empty title, or the same title, is not a
    /// write: the row stays as it was.
    pub fn commit_edit(&mut self) {
        let Some(edit) = self.task_edit.take() else {
            return;
        };
        if self.is_pending_add(&edit.id) {
            return;
        }
        let title = edit.edit.text.trim().to_owned();
        let Some(task) = self.tasks.iter().find(|task| task.id == edit.id) else {
            return;
        };
        if title.is_empty() || title == task.title {
            return;
        }
        self.task_error = None;
        self.pending
            .push_back(Action::SetTaskTitle { id: edit.id, title });
    }

    pub fn cancel_edit(&mut self) {
        self.task_edit = None;
    }

    /// Open a row's notes and dates, or close them. The step field lives under
    /// the row, not in these details, so it stays open either way.
    pub fn toggle_expanded(&mut self, id: &str) {
        if self.expanded.as_deref() == Some(id) {
            self.expanded = None;
            self.task_note = None;
            return;
        }
        self.expanded = Some(id.to_owned());
        self.task_note = self
            .tasks
            .iter()
            .find(|task| task.id == id)
            .map(|task| TaskNote {
                id: task.id.clone(),
                edit: TextEdit::new(task.notes.clone()),
                saving: false,
            });
    }

    /// `Save note` under an expanded row.
    pub fn save_task_note(&mut self) {
        let note_id = match &self.task_note {
            Some(note) if self.is_pending_add(&note.id) => return,
            Some(note) => note.id.clone(),
            None => return,
        };
        let note = self.task_note.as_mut().expect("checked above");
        note.saving = true;
        self.pending.push_back(Action::SetTaskNotes {
            id: note_id,
            notes: note.edit.text.clone(),
        });
    }

    /// Check a task. Completing shows the eight-second way back; reopening is
    /// immediate.
    pub fn toggle_task(&mut self, id: &str) {
        if self.is_pending_add(id) {
            return;
        }
        let Some(task) = self.tasks.iter().find(|task| task.id == id).cloned() else {
            return;
        };
        self.task_error = None;
        if task.done {
            self.pending.push_back(Action::SetTaskDone {
                id: id.to_owned(),
                done: false,
            });
            return;
        }
        // Completing a task closes its open steps, and theirs, in the same
        // write. The toast remembers them so undo takes the whole check back.
        let mut closed = Vec::new();
        let mut queue: VecDeque<String> = VecDeque::from([id.to_owned()]);
        while let Some(parent) = queue.pop_front() {
            for child in self.steps_of(&parent) {
                if !child.done {
                    closed.push(child.id.clone());
                }
                queue.push_back(child.id.clone());
            }
        }
        self.undo = Some(Undo::Task {
            id: id.to_owned(),
            done: true,
            closed,
            message: format!("\u{201c}{}\u{201d} done", task.title),
            left: UNDO_SECONDS,
        });
        self.pending.push_back(Action::SetTaskDone {
            id: id.to_owned(),
            done: true,
        });
    }

    /// Delete a task. There is no dialog: the row is soft-deleted, and the toast
    /// puts the same row back.
    pub fn delete_task(&mut self, id: &str) {
        if self.is_pending_add(id) {
            return;
        }
        let Some(task) = self.tasks.iter().find(|task| task.id == id).cloned() else {
            return;
        };
        self.task_error = None;
        self.undo = Some(Undo::Task {
            id: id.to_owned(),
            done: false,
            closed: Vec::new(),
            message: format!("\u{201c}{}\u{201d} deleted", task.title),
            left: UNDO_SECONDS,
        });
        self.pending.push_back(Action::DeleteTask(id.to_owned()));
    }

    /// The toast's Undo button.
    pub fn undo_task(&mut self) {
        match self.undo.take() {
            Some(Undo::Task {
                id,
                done: true,
                closed,
                ..
            }) => {
                self.pending
                    .push_back(Action::SetTaskDone { id, done: false });
                for step in closed {
                    self.pending.push_back(Action::SetTaskDone {
                        id: step,
                        done: false,
                    });
                }
            }
            Some(Undo::Task { id, .. }) => self.pending.push_back(Action::RestoreTask(id)),
            Some(Undo::Project { id, task_ids, .. }) => self
                .pending
                .push_back(Action::RestoreProject { id, task_ids }),
            None => {}
        }
    }

    pub fn dismiss_undo(&mut self) {
        self.undo = None;
    }

    /// The due buttons under an expanded row. `Some(day)` sets, `None` clears.
    pub fn set_due(&mut self, id: &str, due_day: Option<String>) {
        if self.is_pending_add(id) {
            return;
        }
        self.task_error = None;
        self.pending.push_back(Action::SetTaskDue {
            id: id.to_owned(),
            due_day,
        });
    }

    /// Move a task to a project, or to no project.
    pub fn set_project(&mut self, id: &str, list_id: Option<String>) {
        if self.is_pending_add(id) {
            return;
        }
        self.task_error = None;
        // A step is filed where its task is, and the store moves both in one
        // write; the rows on screen follow now rather than a frame later.
        let project = list_id.as_ref().and_then(|list| {
            self.projects
                .iter()
                .find(|project| &project.id == list)
                .map(|project| (project.name.clone(), project.color.clone()))
        });
        for task in &mut self.tasks {
            if task.parent_id.as_deref() == Some(id) {
                task.list_id = list_id.clone();
                task.list_name = project.as_ref().map(|(name, _)| name.clone());
                task.list_color = project.as_ref().and_then(|(_, color)| color.clone());
            }
        }
        self.pending.push_back(Action::SetTaskProject {
            id: id.to_owned(),
            list_id,
        });
    }

    /// Open a task from Home: the checklist, with that row's notes open.
    pub fn show_task(&mut self, id: &str) {
        self.route = Route::Tasks;
        self.archive = false;
        if self.expanded.as_deref() != Some(id) {
            self.toggle_expanded(id);
        }
    }

    /// Drop editors whose row the list no longer has.
    fn prune_task_editors(&mut self) {
        if let Some(edit) = &self.task_edit
            && !self.tasks.iter().any(|task| task.id == edit.id)
        {
            self.task_edit = None;
        }
        if let Some(id) = &self.expanded
            && !self.tasks.iter().any(|task| task.id == *id)
        {
            self.expanded = None;
            self.task_note = None;
        }
        if let Some(step) = &self.step_input
            && !self.tasks.iter().any(|task| task.id == step.parent_id)
        {
            self.step_input = None;
        }
    }

    fn sort_tasks(&mut self) {
        self.tasks.sort_by(|a, b| {
            // Steps keep the order they were added in. Their block's place is
            // the parent's; sorting them by title would scramble a checklist
            // someone wrote in an order.
            if a.parent_id.is_some() && a.parent_id == b.parent_id {
                return std::cmp::Ordering::Equal;
            }
            a.done.cmp(&b.done).then_with(|| {
                a.due_day
                    .is_none()
                    .cmp(&b.due_day.is_none())
                    .then_with(|| a.due_day.cmp(&b.due_day))
                    .then_with(|| a.title.cmp(&b.title))
            })
        });
    }

    /// Select a note. If the editor has unsaved text, the switch waits for the
    /// save: the open is queued and the old draft stays on screen until then.
    pub fn select_note(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        if self.draft.as_ref().is_some_and(Draft::is_dirty) {
            self.pending_open = Some(id.to_owned());
            return;
        }
        self.open_now(id);
    }

    fn open_now(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
        self.body = NoteBody::Loading;
        self.draft = None;
        self.pending_open = Some(id.to_owned());
    }

    /// A body arrived. A stale one (the selection moved on) is dropped.
    pub fn show_body(&mut self, id: &str, body: impl Into<String>, hash: u64) {
        if self.selected.as_deref() != Some(id) {
            return;
        }
        self.pending_open = None;
        self.body = NoteBody::Ready;
        self.draft = Some(Draft::new(id, body.into(), hash));
    }

    pub fn fail_body(&mut self, id: &str, error: String) {
        if self.selected.as_deref() == Some(id) {
            self.pending_open = None;
            self.draft = None;
            self.body = NoteBody::Failed(error);
        }
    }

    /// The editor's text changed. Called from the field's `on_change` handler.
    /// The token moves so a save that is already in flight cannot mark newer
    /// text clean.
    pub fn draft_touched(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.token += 1;
            draft.idle = 0.0;
        }
    }

    /// `⌘S`, the Save button, Retry and the close hook. Asking again is a retry,
    /// so a previous refusal does not block it; a conflict still waits for the
    /// user's Reload or Overwrite.
    pub fn request_save(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.error = None;
        }
        self.save_requested = true;
    }

    /// After a conflict: write our draft over the file on disk.
    pub fn request_overwrite(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.conflict = None;
            draft.error = None;
            draft.force = true;
        }
        self.save_requested = true;
    }

    /// After a conflict: drop the draft and read the file again.
    pub fn request_reload(&mut self) {
        if let Some(draft) = &self.draft {
            let id = draft.id.clone();
            self.open_now(&id);
        }
    }

    /// The "Check now" button. Asking twice while a check is running is not a
    /// second request: the answer is already on its way.
    pub fn request_update_check(&mut self) {
        if matches!(self.update, Update::Checking) {
            return;
        }
        self.update = Update::Checking;
        self.update_requested = true;
    }

    /// The check answered with a release newer than this build. The version is
    /// stored as text, because that is what a banner shows and a link needs.
    pub fn update_available(&mut self, version: impl Into<String>) {
        self.update = Update::Available(version.into());
    }

    /// The check answered that this build is the latest release.
    pub fn update_current(&mut self) {
        self.update = Update::Current;
    }

    /// The check answered with a release older than this build.
    pub fn update_ahead(&mut self) {
        self.update = Update::Ahead;
    }

    /// The check could not answer. A sentence, not a code: it is shown as is.
    pub fn update_failed(&mut self, message: impl Into<String>) {
        self.update = Update::Failed(message.into());
    }

    /// The banner's "View release" button.
    pub fn request_release(&mut self, version: impl Into<String>) {
        self.release_requested = Some(version.into());
    }

    /// The open note's file changed on disk, and the draft holds nothing that
    /// is not saved: read the note again so the editor shows what is there now.
    ///
    /// A dirty draft is left exactly as it is — its next save reports the
    /// conflict and the person chooses — and so is one that already explains a
    /// conflict, or the explanation would vanish under the banner.
    pub fn refresh_from_disk(&mut self, notes: &[String]) -> bool {
        let Some(draft) = &self.draft else {
            return false;
        };
        if !notes.iter().any(|id| id == &draft.id) {
            return false;
        }
        if draft.is_dirty() || draft.conflict.is_some() || draft.error.is_some() {
            return false;
        }
        let id = draft.id.clone();
        self.open_now(&id);
        true
    }

    /// Drop everything queued behind a save. Called when the window closes: the
    /// note being left stays open, and nothing fires after the screen is gone.
    pub fn cancel_pending(&mut self) {
        self.pending_open = None;
        self.pending_new = false;
        self.pending_delete = None;
        self.confirm_delete = None;
        self.vault_intent = None;
        self.vault_setup.busy = false;
    }

    /// Launch found no usable vault. The dialog opens on `path`, keeping the
    /// saved one when it is there, so nothing falls back silently.
    pub fn ask_for_vault(&mut self, path: impl Into<String>, notice: Option<String>) {
        self.vault = path.into();
        self.vault_open = false;
        self.vault_dialog = true;
        self.vault_setup = VaultSetup::new(self.vault.clone());
        self.vault_setup.notice = notice;
        self.vault_setup.needs_check = true;
    }

    /// Every surface that says "no vault" calls this, so there is one dialog and
    /// one set of rules behind all of them.
    pub fn open_vault_dialog(&mut self) {
        self.vault_dialog = true;
        self.vault_setup.path = self.vault.clone();
        self.vault_setup.notice = None;
        self.vault_setup.busy = false;
        self.vault_setup.needs_check = true;
    }

    /// "Not now". The app stays open; the way back is the same dialog. A folder
    /// that was mid-open is dropped too: saying no must not open one anyway.
    pub fn close_vault_dialog(&mut self) {
        self.vault_dialog = false;
        self.vault_setup.busy = false;
        self.vault_intent = None;
    }

    /// The app looked at the folder. A stale answer (the path moved on while the
    /// disk was being read) is dropped.
    pub fn vault_checked(&mut self, path: &str, status: VaultStatus) {
        if self.vault_setup.path == path {
            self.vault_setup.status = status;
            self.vault_setup.needs_check = false;
        }
    }

    /// The dialog's primary button. A missing folder is created; anything else
    /// is used as it is. An unusable one has a disabled button, so this cannot
    /// be reached for it, but the guard keeps the rule in one place.
    pub fn vault_primary(&mut self) {
        if self.vault_setup.busy || !self.vault_setup.status.can_open() {
            return;
        }
        let path = self.vault_setup.path.clone();
        self.vault_setup.notice = None;
        self.vault_setup.busy = true;
        let intent = match &self.vault_setup.status {
            VaultStatus::Missing => VaultIntent::Create(path),
            _ => VaultIntent::Use(path),
        };
        self.vault_intent = Some(intent);
    }

    /// The system folder sheet answered with a folder. It is used straight away:
    /// picking one and then pressing "Use" again would be two answers to one
    /// question. A refusal comes back as [`State::vault_failed`].
    pub fn choose_vault(&mut self, path: impl Into<String>) {
        let path = path.into();
        self.vault_setup.path = path.clone();
        self.vault_setup.notice = None;
        self.vault_setup.busy = true;
        self.vault_setup.needs_check = true;
        self.vault_intent = Some(VaultIntent::Use(path));
    }

    /// The folder could not be created, remembered or opened. The dialog stays
    /// up with the reason, and the path is kept so Try again is one press.
    pub fn vault_failed(&mut self, reason: impl Into<String>) {
        self.vault_setup.busy = false;
        self.vault_setup.notice = Some(reason.into());
        self.vault_setup.needs_check = true;
    }

    /// The vault is open: the dialog closes and the list loads itself.
    pub fn vault_opened(&mut self, path: impl Into<String>) {
        self.vault = path.into();
        self.vault_open = true;
        self.vault_dialog = false;
        self.vault_intent = None;
        self.vault_setup = VaultSetup::new(self.vault.clone());
        self.load = Load::Loading;
        self.action_error = None;
        // The old vault's rows are not this vault's rows.
        self.notes.clear();
        self.selected = None;
        self.body = NoteBody::None;
        self.draft = None;
        self.pending_open = None;
        self.pending_new = false;
        self.pending_delete = None;
        self.confirm_delete = None;
    }

    /// The next open or create the app should run, if any. `poll` takes it;
    /// `pump` and [`Action`] never see it.
    pub fn take_vault_intent(&mut self) -> Option<VaultIntent> {
        self.vault_intent.take()
    }

    /// Whether the app has vault work this frame: a folder to look at, or an
    /// open/create to run. `poll` skips the disk entirely when this is false.
    pub fn vault_pending(&self) -> bool {
        self.vault_setup.needs_check || self.vault_intent.is_some()
    }

    /// New note (`⌘N` and the list's button). A dirty draft is written first, so
    /// the note being left is never abandoned. Without a vault there is nowhere
    /// to write, so the honest answer is the setup dialog, not an error.
    pub fn request_create(&mut self) {
        if !self.vault_open {
            if !self.vault_dialog {
                self.open_vault_dialog();
            }
            return;
        }
        self.action_error = None;
        self.pending_new = true;
        if self.draft.as_ref().is_some_and(Draft::is_dirty) {
            self.request_save();
        }
    }

    /// Ask before deleting the selected note. Nothing happens until
    /// [`State::confirm_delete`].
    pub fn request_delete(&mut self) {
        if !self.vault_open {
            if !self.vault_dialog {
                self.open_vault_dialog();
            }
            return;
        }
        if let Some(id) = &self.selected {
            self.action_error = None;
            self.confirm_delete = Some(id.clone());
        }
    }

    pub fn cancel_delete(&mut self) {
        self.confirm_delete = None;
    }

    /// The user said yes. A dirty draft is written first; if that save fails the
    /// delete is dropped rather than throwing the text away.
    pub fn confirm_delete(&mut self) {
        let Some(id) = self.confirm_delete.take() else {
            return;
        };
        self.action_error = None;
        self.pending_delete = Some(id.clone());
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| draft.id == id && draft.is_dirty())
        {
            self.request_save();
        }
    }

    /// A new note arrived from the worker: show it, ready to type into.
    pub fn created(&mut self, id: &str, body: String, hash: u64) {
        self.pending_new = false;
        self.action_error = None;
        self.selected = Some(id.to_owned());
        self.show_body(id, body, hash);
    }

    /// A note is gone from the vault: its row, and the editor if it was open.
    pub fn deleted(&mut self, id: &str) {
        self.pending_delete = None;
        if self.confirm_delete.as_deref() == Some(id) {
            self.confirm_delete = None;
        }
        if self.pending_open.as_deref() == Some(id) {
            self.pending_open = None;
        }
        self.notes.retain(|note| note.id != id);
        if self.selected.as_deref() == Some(id) {
            self.selected = None;
            self.body = NoteBody::None;
            self.draft = None;
        }
        self.action_error = None;
    }

    pub fn create_failed(&mut self, error: impl Into<String>) {
        self.pending_new = false;
        self.action_error = Some(error.into());
    }

    pub fn delete_failed(&mut self, id: &str, error: impl Into<String>) {
        if self.pending_delete.as_deref() == Some(id) {
            self.pending_delete = None;
        }
        self.action_error = Some(error.into());
    }

    /// A save landed. `token` is the edit revision that was written, so text
    /// typed while the save was in flight stays dirty.
    pub fn saved(&mut self, id: &str, token: u64, hash: u64, title: String, tags: Vec<String>) {
        if let Some(row) = self.notes.iter_mut().find(|note| note.id == id) {
            row.title = title;
            row.tags = tags;
        }
        self.sort_notes();

        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.base_hash = hash;
            draft.acked = draft.acked.max(token);
            draft.saving = false;
            draft.conflict = None;
            draft.error = None;
            draft.since_save = 0.0;
        }
    }

    /// The file changed on disk since the draft was read.
    pub fn save_conflict(&mut self, id: &str, message: impl Into<String>) {
        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.saving = false;
            draft.conflict = Some(message.into());
        }
        // A delete behind a refused save is dropped: the file on disk is not
        // ours to remove.
        self.pending_delete = None;
    }

    pub fn save_failed(&mut self, id: &str, error: impl Into<String>) {
        if let Some(draft) = &mut self.draft
            && draft.id == id
        {
            draft.saving = false;
            draft.error = Some(error.into());
        }
        self.pending_delete = None;
        self.pending_new = false;
    }

    /// Advance the editor's clocks. `now` is `poll`'s clock: seconds since the
    /// app started. Returns true when a save is now due, so `poll` draws a frame
    /// and schedules the next wake.
    pub fn tick(&mut self, now: f64) -> bool {
        let dt = (now - self.clock).max(0.0);
        self.clock = now;
        if let Some(draft) = &mut self.draft {
            draft.idle += dt;
            draft.since_save += dt;
        }
        // The undo toast is the only thing on the task side that waits.
        let mut expired = false;
        if let Some(undo) = &mut self.undo {
            *undo.left_mut() -= dt;
            expired = undo.left() <= 0.0;
        }
        if expired {
            self.undo = None;
        }
        expired || self.next_wake().is_some_and(|wait| wait <= 0.0)
    }

    /// How long `poll` should sleep before asking for a save or dropping an
    /// expired undo, if either is waiting.
    pub fn next_wake(&self) -> Option<f64> {
        let draft = self.draft.as_ref().and_then(|draft| {
            if !draft.is_dirty()
                || draft.saving
                || draft.conflict.is_some()
                || draft.error.is_some()
            {
                return None;
            }
            Some(
                (AUTOSAVE_IDLE - draft.idle)
                    .min(AUTOSAVE_MAX - draft.since_save)
                    .max(0.0),
            )
        });
        let undo = self.undo.as_ref().map(|undo| undo.left().max(0.0));
        match (draft, undo) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(wait), None) | (None, Some(wait)) => Some(wait),
            (None, None) => None,
        }
    }

    /// Ask for the next frame as soon as the loop can give one.
    ///
    /// `poll` sends what the handlers queued, and it runs at the *start* of a
    /// frame — before the handler that queues. So without this a command typed
    /// in a field waits for the next pointer or key event to leave the app.
    /// Called by every handler that can queue work.
    pub fn flush(&mut self, tx: &mut Tx) {
        if self.has_queued() {
            tx.wake_after(std::time::Duration::ZERO);
        }
    }

    /// Whether `take_action` has something to return on the next poll. The
    /// autosave delay is not here: `next_wake` schedules that one itself.
    fn has_queued(&self) -> bool {
        !self.pending.is_empty()
            || self.pending_open.is_some()
            || self.pending_new
            || self.pending_delete.is_some()
            || self.save_requested
            || self.update_requested
            || self.release_requested.is_some()
            || self.vault_intent.is_some()
    }

    /// The next command the app should send. Called from `poll` after `tick`.
    pub fn take_action(&mut self) -> Option<Action> {
        // A click on a task is one-shot and must not wait behind anything.
        if let Some(action) = self.pending.pop_front() {
            return Some(action);
        }
        // The update requests come before the editor: they are one-shot, and a
        // check must not wait behind a draft that is waiting out its idle delay.
        if let Some(version) = std::mem::take(&mut self.release_requested) {
            return Some(Action::ShowRelease(version));
        }
        if std::mem::take(&mut self.update_requested) {
            return Some(Action::CheckForUpdates);
        }

        if let Some(draft) = &mut self.draft
            && draft.conflict.is_none()
            && draft.error.is_none()
            && draft.is_dirty()
            && !draft.saving
        {
            let due = draft.idle >= AUTOSAVE_IDLE || draft.since_save >= AUTOSAVE_MAX;
            if due || std::mem::take(&mut self.save_requested) {
                draft.saving = true;
                draft.idle = 0.0;
                draft.since_save = 0.0;
                // Overwrite is the user's answer about one version of the file,
                // not a standing permission: a later save, and a retry after a
                // failed overwrite, asks the hash guard again.
                let force = std::mem::take(&mut draft.force);
                return Some(Action::Save(SaveRequest {
                    id: draft.id.clone(),
                    body: draft.edit.text.clone(),
                    base_hash: draft.base_hash,
                    token: draft.token,
                    force,
                }));
            }
        }
        self.save_requested = false;

        let clean = self.draft.as_ref().is_none_or(|draft| !draft.is_dirty());
        if clean && let Some(id) = self.pending_delete.take() {
            if self.selected.as_deref() == Some(&id) {
                self.selected = None;
                self.body = NoteBody::None;
                self.draft = None;
            }
            self.pending_open = None;
            return Some(Action::Delete(id));
        }
        if clean && std::mem::take(&mut self.pending_new) {
            self.pending_open = None;
            return Some(Action::Create);
        }

        if self.draft.as_ref().is_none_or(|draft| !draft.is_dirty())
            && let Some(id) = self.pending_open.take()
        {
            // The switch is now safe: the old note is saved or untouched.
            self.selected = Some(id.clone());
            self.body = NoteBody::Loading;
            self.draft = None;
            return Some(Action::Open(id));
        }
        None
    }

    fn sort_notes(&mut self) {
        self.notes.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    /// `init` for the app: `--vault DIR` and `--route NAME` from the command line,
    /// with the rest of Rune's flags left to Rune.
    pub fn from_args() -> Self {
        let args = rune::extra_args();
        let mut state = Self::new();
        if let Some(vault) = arg(args, "vault") {
            state.vault = vault;
        }
        if let Some(route) = arg(args, "route").and_then(|name| Route::parse(&name)) {
            state.route = route;
        }
        state
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// `~/Documents/Notes`. Resolved here, not in the view, and only once.
fn default_vault() -> String {
    let home = std::env::var_os("HOME").unwrap_or_else(|| OsString::from("."));
    std::path::PathBuf::from(home)
        .join("Documents")
        .join("Notes")
        .to_string_lossy()
        .into_owned()
}

/// The value of `--name value` or `--name=value` among Rune's extra arguments.
pub fn arg(args: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    if let Some(index) = args.iter().position(|a| a == &flag) {
        return args.get(index + 1).cloned();
    }
    args.iter()
        .find_map(|a| a.strip_prefix(&format!("{flag}=")).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_vault_is_under_documents() {
        let vault = default_vault();
        assert!(vault.ends_with("Documents/Notes"), "{vault}");
        assert!(vault.starts_with('/'), "an absolute path: {vault}");
    }

    #[test]
    fn flags_are_read_in_both_spellings() {
        let spaced = vec!["--vault".to_owned(), "/tmp/v".to_owned()];
        let equals = vec!["--vault=/tmp/w".to_owned()];
        assert_eq!(arg(&spaced, "vault").as_deref(), Some("/tmp/v"));
        assert_eq!(arg(&equals, "vault").as_deref(), Some("/tmp/w"));
        assert_eq!(arg(&[], "vault"), None);
    }

    #[test]
    fn panes_start_open_at_sensible_widths() {
        let state = State::new();
        assert!(state.sidebar.is_open());
        assert!(state.detail.is_open());
        assert!(state.sidebar.width() > 180.0);
        assert!(state.detail.width() > 320.0);
    }

    fn state_with_two_notes() -> State {
        let mut state = State::new();
        state.set_notes(vec![
            NoteRow::new("01A", "Alpha", &["work"]),
            NoteRow::new("01B", "Beta", &[]),
        ]);
        state
    }

    fn open_alpha(state: &mut State) {
        state.select_note("01A");
        assert_eq!(state.take_action(), Some(Action::Open("01A".to_owned())));
        state.show_body("01A", "# Alpha\n\nold\n", 11);
    }

    fn edit(state: &mut State, text: &str) {
        state.draft.as_mut().unwrap().edit.replace_text(text);
        state.draft_touched();
    }

    fn next_save(state: &mut State) -> SaveRequest {
        state.tick(state.clock + 0.8);
        match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected a save, got {other:?}"),
        }
    }

    #[test]
    fn the_state_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<State>();
        assert_send_sync::<Draft>();
    }

    #[test]
    fn an_edit_saves_the_whole_body_after_the_idle_delay() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nnew\n");
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Edited);
        assert!(state.take_action().is_none(), "not due yet");

        let request = next_save(&mut state);
        assert_eq!(request.id, "01A");
        assert_eq!(request.body, "# Alpha\n\nnew\n");
        assert_eq!(request.base_hash, 11);
        assert_eq!(request.token, 1, "the save carries the edit revision");
        assert!(!request.force);
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Saving);

        // Nothing more goes out while that save is in flight.
        state.tick(state.clock + 10.0);
        assert!(state.take_action().is_none());
    }

    #[test]
    fn typing_during_a_save_keeps_the_draft_dirty() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nfirst\n");
        let first = next_save(&mut state);

        edit(&mut state, "# Alpha\n\nsecond\n");
        state.saved("01A", first.token, 22, "Alpha".to_owned(), vec![]);

        assert!(
            state.draft.as_ref().unwrap().is_dirty(),
            "the ack is older than the text"
        );
        let second = next_save(&mut state);
        assert_eq!(second.body, "# Alpha\n\nsecond\n");
        assert_eq!(second.base_hash, 22, "the saved file is the new base");

        state.saved("01A", second.token, 33, "Alpha".to_owned(), vec![]);
        assert!(!state.draft.as_ref().unwrap().is_dirty());
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Saved);
    }

    #[test]
    fn a_dirty_switch_waits_for_the_save_then_opens() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nchanged\n");

        state.select_note("01B");
        assert_eq!(state.selected.as_deref(), Some("01A"), "still on Alpha");
        assert_eq!(state.pending_open.as_deref(), Some("01B"));

        state.request_save();
        let save = match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected the flush, got {other:?}"),
        };
        assert!(state.take_action().is_none(), "the open waits for the ack");

        state.saved("01A", save.token, 44, "Alpha".to_owned(), vec![]);
        assert_eq!(
            state.take_action(),
            Some(Action::Open("01B".to_owned())),
            "the save landed, so the switch can happen"
        );
        assert_eq!(state.selected.as_deref(), Some("01B"));
        assert!(state.draft.is_none());
        assert_eq!(state.body, NoteBody::Loading);
    }

    #[test]
    fn a_clean_switch_opens_immediately() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        state.select_note("01B");
        assert_eq!(state.take_action(), Some(Action::Open("01B".to_owned())));
    }

    #[test]
    fn a_new_note_waits_for_the_draft_being_left() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");

        state.request_create();
        // The save first, so the note being left reaches its file.
        let first = state.take_action();
        assert!(matches!(first, Some(Action::Save(_))), "{first:?}");
        assert_eq!(state.take_action(), None, "the new note waits");

        state.saved("01A", 1, 12, "Alpha".to_owned(), vec!["work".to_owned()]);
        assert_eq!(state.take_action(), Some(Action::Create));
    }

    #[test]
    fn a_new_note_on_a_clean_editor_is_asked_for_at_once() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);

        state.request_create();
        assert_eq!(state.take_action(), Some(Action::Create));
    }

    #[test]
    fn creating_shows_the_new_note_ready_to_type() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);

        state.created("01C", "# Untitled\n".to_owned(), 5);
        assert_eq!(state.selected.as_deref(), Some("01C"));
        assert_eq!(state.body, NoteBody::Ready);
        let draft = state.draft.as_ref().unwrap();
        assert_eq!(draft.id, "01C");
        assert_eq!(draft.edit.text, "# Untitled\n");
        assert!(!draft.is_dirty(), "a fresh note has nothing to save");
    }

    #[test]
    fn a_delete_asks_first_and_then_removes_the_note() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);

        state.request_delete();
        assert_eq!(state.confirm_delete.as_deref(), Some("01A"));
        assert_eq!(
            state.take_action(),
            None,
            "nothing is deleted before the answer"
        );

        state.cancel_delete();
        assert!(state.confirm_delete.is_none());
        assert_eq!(state.take_action(), None, "a cancelled delete does nothing");

        state.request_delete();
        state.confirm_delete();
        assert_eq!(state.take_action(), Some(Action::Delete("01A".to_owned())));
        assert!(state.selected.is_none(), "the editor is closed with it");
        assert!(state.draft.is_none());

        state.deleted("01A");
        let ids: Vec<&str> = state.notes.iter().map(|note| note.id.as_str()).collect();
        assert_eq!(ids, vec!["01B"]);
    }

    #[test]
    fn a_delete_of_a_dirty_note_saves_it_before_it_goes() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");

        state.request_delete();
        state.confirm_delete();
        let action = state.take_action();
        assert!(
            matches!(action, Some(Action::Save(_))),
            "the text is saved first: {action:?}"
        );
        assert_eq!(state.take_action(), None, "the delete waits for the save");

        state.saved("01A", 1, 12, "Alpha".to_owned(), vec!["work".to_owned()]);
        assert_eq!(state.take_action(), Some(Action::Delete("01A".to_owned())));
    }

    #[test]
    fn a_delete_dialog_closes_when_its_note_leaves_the_list() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        state.request_delete();
        assert_eq!(state.confirm_delete.as_deref(), Some("01A"));

        state.set_notes(vec![NoteRow::new("01B", "Beta", &[])]);
        assert!(
            state.confirm_delete.is_none(),
            "the note is gone, so is the question"
        );
    }

    #[test]
    fn a_delete_is_dropped_when_the_save_it_waited_for_fails() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        state.request_delete();
        state.confirm_delete();
        assert!(matches!(state.take_action(), Some(Action::Save(_))));

        state.save_failed("01A", "io: the disk is full");
        assert_eq!(
            state.take_action(),
            None,
            "the file is still the one on disk"
        );
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Failed);
    }

    #[test]
    fn a_refused_create_is_explained_in_the_list() {
        let mut state = state_with_two_notes();
        state.request_create();
        assert_eq!(state.take_action(), Some(Action::Create));

        state.create_failed("the vault is not open yet");
        assert_eq!(
            state.action_error.as_deref(),
            Some("the vault is not open yet")
        );

        // Asking again clears the old message.
        state.request_create();
        assert!(state.action_error.is_none());
    }

    #[test]
    fn retry_after_a_failed_save_sends_the_draft_again() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        let first = next_save(&mut state);
        state.save_failed("01A", "io: the disk is full");
        assert_eq!(state.draft.as_ref().unwrap().status(), DraftStatus::Failed);

        state.tick(state.clock + 20.0);
        assert!(
            state.take_action().is_none(),
            "a failure never retries itself"
        );

        // The Retry button, ⌘S and the close hook all ask again.
        state.request_save();
        let retry = match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected a retry, got {other:?}"),
        };
        assert_eq!(retry.body, "# Alpha\n\nmine\n");
        assert_eq!(retry.token, first.token, "the same edit, sent again");
    }

    #[test]
    fn overwrite_is_one_save_not_a_standing_permission() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        next_save(&mut state);
        state.save_conflict("01A", "changed on disk");

        state.request_overwrite();
        let first = next_save(&mut state);
        assert!(first.force, "the user chose to overwrite");

        // That save fails, so the file on disk may have changed again since the
        // user looked at it: the next attempt must ask the guard.
        state.save_failed("01A", "io: the disk is full");
        state.request_save();
        let retry = next_save(&mut state);
        assert!(
            !retry.force,
            "an overwrite covers the version the user saw, not every later one"
        );
        assert_eq!(retry.body, "# Alpha\n\nmine\n");
    }

    #[test]
    fn asking_to_save_does_not_clear_a_conflict() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        next_save(&mut state);
        state.save_conflict("01A", "changed on disk");

        state.request_save();
        assert!(state.take_action().is_none(), "still waiting on the user");
        assert_eq!(
            state.draft.as_ref().unwrap().status(),
            DraftStatus::Conflict
        );
    }

    #[test]
    fn a_conflict_blocks_autosave_until_the_user_chooses() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        next_save(&mut state);
        state.save_conflict("01A", "changed on disk");
        assert_eq!(
            state.draft.as_ref().unwrap().status(),
            DraftStatus::Conflict
        );

        state.tick(state.clock + 20.0);
        assert!(state.take_action().is_none(), "a conflict never retries");

        state.request_overwrite();
        let request = match state.take_action() {
            Some(Action::Save(request)) => request,
            other => panic!("expected the overwrite, got {other:?}"),
        };
        assert!(request.force);
        assert_eq!(request.body, "# Alpha\n\nmine\n");
    }

    #[test]
    fn reloading_after_a_conflict_drops_the_draft_and_reads_again() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nmine\n");
        state.save_conflict("01A", "changed on disk");

        state.request_reload();
        assert_eq!(state.take_action(), Some(Action::Open("01A".to_owned())));
        assert!(state.draft.is_none());
        assert_eq!(state.body, NoteBody::Loading);
    }

    #[test]
    fn a_saved_title_updates_the_row_and_the_order() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Zed\n\nold\n");
        let save = next_save(&mut state);
        state.saved(
            "01A",
            save.token,
            55,
            "Zed".to_owned(),
            vec!["work".to_owned()],
        );

        assert_eq!(state.notes[0].title, "Beta", "Beta sorts before Zed");
        assert_eq!(state.notes[1].title, "Zed");
        assert_eq!(state.notes[1].tags, vec!["work".to_owned()]);
    }

    #[test]
    fn a_missing_note_drops_its_draft() {
        let mut state = state_with_two_notes();
        open_alpha(&mut state);
        edit(&mut state, "# Alpha\n\nlost\n");

        state.set_notes(vec![NoteRow::new("01B", "Beta", &[])]);
        assert!(state.draft.is_none());
        assert_eq!(state.selected, None);
        assert_eq!(state.body, NoteBody::None);
    }

    #[test]
    fn a_late_body_for_another_note_is_dropped() {
        let mut state = state_with_two_notes();
        state.select_note("01A");
        state.select_note("01B");
        state.show_body("01A", "# Alpha\n\nstale\n", 1);
        assert!(state.draft.is_none(), "the selection had moved on");
    }
}
