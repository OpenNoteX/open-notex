//! The window: sidebar, list, detail.
//!
//! Nothing here reads the clock, the database or the filesystem. Handlers only
//! mutate [`State`], and the next frame is the layout of that state.

use rune::{
    Align, Cx, El, FileDialogOptions, Key, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text,
};
use rune_kit::shell::{
    RowStatus, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar,
};
use rune_kit::{
    Button, ButtonVariant, ControlSize, Dialog, IconButton, IconButtonVariant, Kit, Spinner,
};

use crate::home;
use crate::routes::{FOOTER, Layout, Route, SECTIONS};
use crate::state::{Draft, DraftStatus, Load, NoteBody, State};
use crate::tasks;
use crate::theme;

/// The window. `AppSpec::view`.
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let k = theme::kit(dark);

    // Nothing that needs the vault draws its own content until there is one.
    // Settings and Search are the exceptions: they have something to say with
    // no data at all (a folder to choose; a field to type in).
    let gate = if s.route == Route::Settings || s.route == Route::Search {
        None
    } else if !s.vault_open {
        Some(no_vault(&k))
    } else {
        match &s.load {
            Load::Loading => Some(loading(&k)),
            Load::Failed(message) => Some(failed_message(&k, message)),
            Load::Ready => None,
        }
    };

    // The route decides the layout now: Home is blocks, Tasks is one list,
    // Notes is the only list-with-detail, and the rest are single pages.
    let split = if let Some(pane) = gate {
        SplitView::new(pane)
    } else {
        match s.route.layout() {
            Layout::Dashboard => SplitView::new(home::home_pane(s, &k)),
            Layout::List if s.route == Route::Tasks => SplitView::new(tasks::tasks_pane(s, &k)),
            Layout::List => SplitView::new(page_pane(s, &k, dark)),
            Layout::ListDetail => SplitView::new(list_pane(s, &k)).inspector(
                &s.detail,
                |s: &mut State| &mut s.detail,
                detail_pane(s, &k),
            ),
            Layout::Page => SplitView::new(page_pane(s, &k, dark)),
        }
    };

    let base = split
        .sidebar(&s.sidebar, |s: &mut State| &mut s.sidebar, sidebar(s, &k))
        .toolbar(toolbar(s, &k))
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(8.0)
        .toolbar_in_panel()
        .sidebar_padding(16.0, 8.0)
        .sidebar_toggle(
            SidebarToggle::new()
                .size(ControlSize::Large)
                .past_lights(16.0)
                .edge(16.0)
                .gap(4.0)
                .in_header(),
        )
        .reduced_motion(cx.is_reduced_motion())
        .build(&k);

    let mut layer = Vec::new();
    delete_dialog(s, &k, &mut layer);
    vault_dialog(s, &k, &mut layer);
    if let Some(toast) = tasks::undo_toast(s, &k) {
        layer.push(toast);
    }
    rune_kit::root(base, layer)
}

/// The confirmation before a delete. Deleting a note moves its file out of the
/// vault, so it is never one stray press away.
fn delete_dialog(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let Some(id) = &s.confirm_delete else {
        return;
    };
    let title = s
        .notes
        .iter()
        .find(|note| &note.id == id)
        .map(|note| note.title.clone())
        .unwrap_or_else(|| "this note".to_owned());

    let actions = row()
        .key("delete-actions")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .justify_end()
        .child(
            Button::new("Cancel")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut State| s.cancel_delete())
                .build(k)
                .key("delete-cancel"),
        )
        .child(
            Button::new("Delete")
                .variant(ButtonVariant::Danger)
                .on_press(|s: &mut State| s.confirm_delete())
                .build(k)
                .key("delete-confirm"),
        );

    Dialog::new("delete-note", format!("Delete \u{201c}{title}\u{201d}?"))
        .description("The file moves to the trash. openNoteX stops listing it.")
        .width(400.0)
        .fit()
        .open(true)
        .on_close(|s: &mut State| s.cancel_delete())
        .child(actions)
        .build(k, layer);
}

/// The one place a vault is chosen or created.
///
/// First launch, the empty list after "Not now", Settings and `⌘N` with no vault
/// all open this same dialog, so the copy, the validation and the way back
/// cannot drift apart.
fn vault_dialog(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let c = &k.c;
    let setup = &s.vault_setup;
    let status = setup.status.summary();
    // A refusal often repeats what the status already says ("That is a file, not
    // a folder."). One line is enough.
    let notice = setup.notice.as_deref().filter(|notice| *notice != status);

    let mut body = col()
        .key("vault-dialog-body")
        .w_full()
        .px(16.0)
        .pt(12.0)
        .pb(4.0)
        .gap(8.0)
        .child(
            text("openNoteX reads that folder, and nothing else is stored anywhere.")
                .key("vault-intro")
                .font(k.ty.text_xsmall)
                .color(c.text_muted)
                .wrap(),
        )
        .child(
            text("FOLDER")
                .key("vault-folder-label")
                .font(k.ty.text_xsmall)
                .color(c.text_faint),
        )
        .child(
            row()
                .key("vault-path-row")
                .w_full()
                .padding(8.0)
                .radius(k.radius.md)
                .fill(c.layer_02)
                .border(1.0, c.border_weak)
                .child(
                    text(setup.path.clone())
                        .key("vault-path")
                        .font(k.ty.small)
                        .color(c.text_base)
                        .truncate(),
                ),
        )
        .child(
            text(status)
                .key("vault-status")
                .font(k.ty.text_xsmall)
                .color(if setup.status.can_open() {
                    c.text_muted
                } else {
                    c.danger.fg
                })
                .wrap(),
        );

    if let Some(notice) = notice {
        body = body.child(
            text(notice.to_owned())
                .key("vault-notice")
                .font(k.ty.text_xsmall)
                .color(c.danger.fg)
                .wrap(),
        );
    }

    let busy = setup.busy;
    // The sheet opens where the dialog is looking, so the suggested folder is
    // one Return away. `start` is owned by the handler, which outlives the frame.
    let start = setup.path.clone();
    let choose = Button::new("Choose…")
        .variant(ButtonVariant::Outline)
        .control_size(ControlSize::Small)
        .disabled(busy)
        .on_press_tx(move |_s: &mut State, tx: &mut Tx| {
            let options = FileDialogOptions {
                title: "Choose a notes folder".to_owned(),
                directory: Some(std::path::PathBuf::from(start.clone())),
                ..FileDialogOptions::default()
            };
            tx.open_folder(options, |s: &mut State, result, _| match result {
                Ok(Some(path)) => s.choose_vault(path.to_string_lossy().into_owned()),
                Ok(None) => {}
                Err(error) => s.vault_failed(format!("The folder chooser did not open: {error}")),
            });
        })
        .build(k)
        .key("vault-choose");

    let primary = Button::new(setup.status.primary_label())
        .variant(ButtonVariant::Submit)
        .control_size(ControlSize::Small)
        .disabled(busy || !setup.status.can_open())
        .on_press(|s: &mut State| s.vault_primary())
        .build(k)
        .key("vault-primary");

    // macOS order: the way out on the left, the decision on the right.
    let actions = row()
        .key("vault-actions")
        .w_full()
        .px(16.0)
        .pb(16.0)
        .justify_between()
        .child(
            Button::new("Not now")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.close_vault_dialog())
                .build(k)
                .key("vault-not-now"),
        )
        .child(
            row()
                .key("vault-decide")
                .gap(8.0)
                .child(choose)
                .child(primary),
        );

    Dialog::new("vault", "Where should your notes live?")
        .description("Every note is a Markdown file in a folder you own.")
        .width(460.0)
        .fit()
        // A stray click must not leave a first-run app in limbo. "Not now" is
        // the deliberate exit.
        .click_outside(false)
        .open(s.vault_dialog)
        .on_close(|s: &mut State| s.close_vault_dialog())
        .child(body)
        .child(actions)
        .build(k, layer);
}

/// Keys the shell answers itself: the sidebar toggle, search and settings.
/// Anything else falls through to the focused control.
pub fn on_key(s: &mut State, ev: &KeyEvent, _: &mut Tx) -> bool {
    if rune_kit::shell::is_sidebar_toggle(ev) {
        s.sidebar.toggle();
        return true;
    }
    if ev.is_command('k') {
        s.route = Route::Search;
        return true;
    }
    if ev.is_command(',') {
        s.route = Route::Settings;
        return true;
    }
    if ev.is_command('s') {
        s.request_save();
        return true;
    }
    if ev.is_command('n') {
        s.request_create();
        return true;
    }
    if ev.mods.cmd && matches!(ev.key, Key::Backspace | Key::Delete) {
        s.request_delete();
        return true;
    }
    false
}

fn sidebar(s: &State, k: &Kit) -> El {
    let mut sb = Sidebar::new()
        .label("Sections")
        .inset(0.0)
        .header_padding(0.0, 10.0)
        .row_gap(2.0);

    for section in SECTIONS {
        let rows: Vec<El> = section
            .routes
            .iter()
            .map(|route| nav_row(s, *route, k))
            .collect();
        sb = if section.title.is_empty() {
            rows.into_iter().fold(sb, |sb, row| sb.child(row))
        } else {
            sb.section(
                SidebarSection::new(section.title)
                    .key(section.title.to_lowercase())
                    .rows(rows),
            )
        };
    }

    FOOTER
        .iter()
        .fold(sb, |sb, route| sb.footer(nav_row(s, *route, k)))
        .build(k)
}

fn nav_row(s: &State, route: Route, k: &Kit) -> El {
    // The checklist's count is the one number the sidebar carries: it is what a
    // person checks before opening the list.
    let label = match route {
        Route::Tasks if s.open_task_count() > 0 => format!("Tasks ({})", s.open_task_count()),
        _ => route.label().to_owned(),
    };
    SidebarRow::new(label)
        .icon(route.icon())
        .selected(s.route == route)
        .on_select(move |s: &mut State, _| s.route = route)
        .build(k)
        .key(format!("nav-{}", route.key()))
}

fn toolbar(s: &State, k: &Kit) -> Toolbar {
    let mut bar = Toolbar::new().title(s.route.label());
    if s.route != Route::Search {
        bar = bar.trailing(
            IconButton::new(&crate::icons::SEARCH)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Search")
                .help("Search notes, tasks and habits")
                .shortcut("⌘K")
                .on_press(|s: &mut State| s.route = Route::Search)
                .build(k)
                .key("open-search"),
        );
    }
    bar
}

/// The middle pane: a subtitle and whatever the route currently holds.
fn list_pane(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let subtitle = match s.route {
        Route::Notes if !s.notes.is_empty() => match s.notes.len() {
            1 => "1 note".to_owned(),
            count => format!("{count} notes"),
        },
        route => route.subtitle().to_owned(),
    };
    col()
        .key("list-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(
            row()
                .key("list-header")
                .w_full()
                .px(16.0)
                .pt(6.0)
                .pb(8.0)
                .gap(8.0)
                .center_y()
                .child(
                    text(subtitle)
                        .key("list-subtitle")
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint)
                        .truncate(),
                )
                .child(el().grow(1.0))
                .child(new_note_button(s, k)),
        )
        .child(el().key("list-divider").w_full().h(1.0).fill(c.border_weak))
        .children(s.action_error.as_ref().map(|error| action_error(error, k)))
        .child(if s.vault_open {
            match &s.load {
                Load::Ready if s.route == Route::Notes && !s.notes.is_empty() => note_list(s, k),
                Load::Ready => empty_list(s, k),
                Load::Loading => loading(k),
                Load::Failed(message) => failed_message(k, message),
            }
        } else {
            // No vault means no notes on any list route. Saying "Nothing yet"
            // there would blame the vault for being empty.
            no_vault(k)
        })
}

/// `⌘N` and the note list's plus. A list route without notes still needs it,
/// so it is in the header rather than over the list.
fn new_note_button(s: &State, k: &Kit) -> El {
    if s.route != Route::Notes {
        return el().key("new-note-none").w(0.0).h(0.0);
    }
    IconButton::new(&crate::icons::PLUS)
        .variant(IconButtonVariant::GhostMuted)
        .control_size(ControlSize::Small)
        .label("New note")
        .help("New note (⌘N)")
        .on_press(|s: &mut State| s.request_create())
        .build(k)
        .key("new-note")
}

/// A create or delete the worker refused, under the list header.
fn action_error(message: &str, k: &Kit) -> El {
    row()
        .key("list-action-error")
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(6.0)
        .child(icon(&crate::icons::WARNING, 12.0).color(k.c.danger.fg))
        .child(
            text(message)
                .key("list-action-error-text")
                .font(k.ty.caption())
                .color(k.c.danger.fg)
                .wrap(),
        )
}

/// The notes list: one row per Markdown file, alphabetical by title.
fn note_list(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let rows: Vec<El> = s
        .notes
        .iter()
        .map(|note| {
            let id = note.id.clone();
            let tags = note.tag_line();
            let mut row = SidebarRow::new(&note.title)
                .icon(&crate::icons::NOTES)
                .selected(s.selected.as_deref() == Some(note.id.as_str()))
                .on_select(move |s: &mut State, _| s.select_note(&id));
            if !tags.is_empty() {
                row = row.subtitle(tags);
            }
            // A dot on the row whose editor holds unsaved text, so a dirty note
            // is visible without opening it.
            if s.draft
                .as_ref()
                .is_some_and(|draft| draft.id == note.id && draft.is_dirty())
            {
                row = row.status(RowStatus::Other(
                    el().key(format!("list-dirty-{}", note.id))
                        .w(6.0)
                        .h(6.0)
                        .radius(3.0)
                        .fill(c.text_muted),
                ));
            }
            row.build(k).key(format!("list-note-{}", note.id))
        })
        .collect();

    col()
        .key("list-notes")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .py(6.0)
        .px(8.0)
        .gap(2.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .children(rows)
}

fn empty_list(s: &State, k: &Kit) -> El {
    let (title, hint) = if s.route == Route::Notes {
        (
            "No notes yet",
            Some("Markdown files in your vault appear here."),
        )
    } else {
        ("Nothing yet", None)
    };

    let mut body = col()
        .key("list-empty")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(8.0)
        .child(icon(s.route.icon(), 20.0).color(k.c.icon_faint))
        .child(text(title).font(k.ty.small).color(k.c.text_faint));

    if let Some(hint) = hint {
        body = body.child(
            text(hint)
                .key("list-empty-hint")
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint)
                .align(Align::Center),
        );
    }
    body
}

/// The list pane before a folder is chosen. One button, because the dialog does
/// the work and asking twice would be two answers to one question.
fn no_vault(k: &Kit) -> El {
    let c = &k.c;
    col()
        .key("list-no-vault")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(20.0)
        .center()
        .gap(8.0)
        .child(icon(&crate::icons::HOME, 20.0).color(c.icon_faint))
        .child(
            text("No vault yet")
                .key("list-no-vault-title")
                .font(k.ty.small)
                .color(c.text_faint),
        )
        .child(
            text("openNoteX needs a folder for your Markdown files.")
                .key("list-no-vault-hint")
                .font(k.ty.text_xsmall)
                .color(c.text_faint)
                .align(Align::Center)
                .wrap()
                .max_w(240.0),
        )
        .child(
            Button::new("Choose vault…")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.open_vault_dialog())
                .build(k)
                .key("list-no-vault-choose"),
        )
}

fn loading(k: &Kit) -> El {
    col()
        .key("list-loading")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(10.0)
        .child(Spinner::new().size(16.0).build(k))
        .child(
            text("Opening your vault…")
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
}

fn failed_message(k: &Kit, message: &str) -> El {
    let c = &k.c;
    col()
        .key("list-failed")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(20.0)
        .center()
        .gap(10.0)
        .child(icon(&crate::icons::WARNING, 22.0).color(c.danger.fg))
        .child(
            text("This view could not load")
                .font(k.ty.ui_medium)
                .color(c.text_base)
                .align(Align::Center),
        )
        .child(
            text(message.to_owned())
                .key("list-error")
                .font(k.ty.small)
                .color(c.text_muted)
                .wrap()
                .align(Align::Center)
                .max_w(260.0),
        )
        .child(
            Button::new("Dismiss")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.load = Load::Ready)
                .build(k)
                .key("dismiss-error"),
        )
}

/// The right pane: the open note, or what would be here and why it is not.
fn detail_pane(s: &State, k: &Kit) -> El {
    if s.route == Route::Notes && s.selected.is_some() {
        note_detail(s, k)
    } else {
        empty_detail(s, k)
    }
}

/// The open note: its title, its tags, its status and the Markdown file itself.
fn note_detail(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let selected = s.selected.as_deref().unwrap_or_default();
    let note = s.notes.iter().find(|note| note.id == selected);
    let title = note.map_or("Note", |note| note.title.as_str()).to_owned();
    let tags = note
        .map(crate::state::NoteRow::tag_line)
        .unwrap_or_default();

    let mut head = col()
        .key("detail-head")
        .w_full()
        .px(32.0)
        .pt(24.0)
        .pb(12.0)
        .gap(4.0)
        .child(
            row()
                .key("detail-title-row")
                .w_full()
                .gap(8.0)
                .items_start()
                .child(
                    text(title)
                        .key("detail-title")
                        .font(k.ty.ui_medium)
                        .color(c.text_base)
                        .wrap()
                        .grow(1.0),
                )
                .child(
                    IconButton::new(&crate::icons::TRASH)
                        .variant(IconButtonVariant::GhostMuted)
                        .control_size(ControlSize::Small)
                        .label("Delete note")
                        .help("Delete note (⌘⌫)")
                        .on_press(|s: &mut State| s.request_delete())
                        .build(k)
                        .key("delete-note"),
                ),
        );
    if !tags.is_empty() {
        head = head.child(
            text(tags)
                .key("detail-tags")
                .font(k.ty.caption())
                .color(c.text_faint)
                .truncate(),
        );
    }
    if let Some(draft) = &s.draft {
        head = head.child(status_row(draft, k));
    }

    let body = match (&s.body, &s.draft) {
        (_, Some(draft)) => editor(draft, k),
        (NoteBody::Loading, _) => centered(
            "detail-loading",
            Spinner::new().size(16.0).build(k),
            "Reading the file…",
            k,
        ),
        (NoteBody::Failed(error), _) => centered(
            "detail-failed",
            icon(&crate::icons::WARNING, 22.0).color(c.danger.fg),
            error,
            k,
        )
        .child(
            text("Pick another note, or fix the file and reopen it.")
                .key("detail-error-hint")
                .font(k.ty.text_xsmall)
                .color(c.text_faint)
                .align(Align::Center)
                .wrap()
                .max_w(300.0),
        ),
        (NoteBody::Ready | NoteBody::None, _) => centered(
            "detail-none",
            icon(s.route.icon(), 20.0).color(c.icon_faint),
            "Select a note to read it.",
            k,
        ),
    };

    let mut pane = col().key("detail").w_full().h_full().min_h(0.0).child(head);
    if let Some(draft) = &s.draft {
        let banner = draft
            .conflict
            .as_deref()
            .map(|message| conflict_banner(message, k))
            .or_else(|| {
                draft
                    .error
                    .as_deref()
                    .map(|message| error_banner(message, k))
            });
        if let Some(banner) = banner {
            pane = pane.child(
                col()
                    .key("detail-banner")
                    .w_full()
                    .px(16.0)
                    .py(8.0)
                    .child(banner),
            );
        }
    }
    pane.child(el().key("detail-rule").w_full().h(1.0).fill(c.border_weak))
        .child(body)
}

/// "Saved · 2 minutes ago" and, while there is something to write, a Save button.
fn status_row(draft: &Draft, k: &Kit) -> El {
    let c = &k.c;
    let (label, color) = match draft.status() {
        DraftStatus::Conflict => ("Changed on disk", c.danger.fg),
        DraftStatus::Failed => ("Not saved", c.danger.fg),
        DraftStatus::Saving => ("Saving…", c.text_faint),
        DraftStatus::Edited => ("Edited", c.text_muted),
        DraftStatus::Saved => ("Saved", c.text_faint),
    };
    let mut line = row().key("detail-status").w_full().gap(8.0).child(
        text(label)
            .key("detail-status-label")
            .font(k.ty.text_xsmall)
            .color(color),
    );
    // A conflicting save is blocked until the user picks a way out, so the
    // button would do nothing; the banner's two buttons are the choice.
    if (draft.is_dirty() || draft.error.is_some()) && draft.conflict.is_none() {
        line = line.child(
            Button::new("Save")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.request_save())
                .build(k)
                .key("save-note"),
        );
    }
    line
}

/// The editor: the file's Markdown, as it is on disk.
fn editor(draft: &Draft, k: &Kit) -> El {
    let c = &k.c;
    let style = rune::TextStyle {
        line_height: (13.0f32 * 1.35 * 2.0).round() / 2.0,
        ..k.ty.ui
    };
    let field = rune::text_input(&draft.edit, style)
        .key("detail-editor")
        .w_full()
        .color(c.text_base)
        .caret(c.text_base, k.selection())
        .placeholder("Start writing…".to_owned(), c.text_faint)
        .spellcheck(true)
        .on_edit(|s: &mut State| &mut s.draft.as_mut().unwrap().edit)
        .on_input_change(|s: &mut State, _| s.draft_touched())
        .on_input_key(|s: &mut State, ev: &KeyEvent, _| {
            if ev.is_command('s') {
                s.request_save();
                true
            } else {
                false
            }
        });

    col()
        .key("detail-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(28.0)
        .pt(8.0)
        .pb(32.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(field)
}

fn conflict_banner(message: &str, k: &Kit) -> El {
    let c = &k.c;
    row()
        .key("detail-conflict")
        .w_full()
        .padding(10.0)
        .gap(8.0)
        .radius(8.0)
        .fill(c.layer_02)
        .child(icon(&crate::icons::WARNING, 16.0).color(c.danger.fg))
        .child(
            text(message.to_owned())
                .key("conflict-message")
                .font(k.ty.text_xsmall)
                .color(c.text_base)
                .wrap()
                .grow(1.0),
        )
        .child(
            Button::new("Reload")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.request_reload())
                .build(k)
                .key("conflict-reload"),
        )
        .child(
            Button::new("Overwrite")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.request_overwrite())
                .build(k)
                .key("conflict-overwrite"),
        )
}

fn error_banner(message: &str, k: &Kit) -> El {
    let c = &k.c;
    row()
        .key("detail-save-failed")
        .w_full()
        .padding(10.0)
        .gap(8.0)
        .radius(8.0)
        .fill(c.layer_02)
        .child(icon(&crate::icons::WARNING, 16.0).color(c.danger.fg))
        .child(
            text(message.to_owned())
                .key("save-error")
                .font(k.ty.text_xsmall)
                .color(c.text_base)
                .wrap()
                .grow(1.0),
        )
        .child(
            Button::new("Try again")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.request_save())
                .build(k)
                .key("save-retry"),
        )
}

/// A centered detail-pane message with one glyph and one line.
fn centered(key: &str, lead: El, message: &str, k: &Kit) -> El {
    col()
        .key(key)
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(10.0)
        .child(lead)
        .child(
            text(message.to_owned())
                .key("detail-message")
                .font(k.ty.small)
                .color(k.c.text_faint)
                .align(Align::Center)
                .wrap()
                .max_w(300.0),
        )
}

/// The empty detail pane: what would be here, and why it is not.
fn empty_detail(s: &State, k: &Kit) -> El {
    let c = &k.c;
    // "No notes yet" blames a vault that is not there. With no vault, say so.
    let (title, hint) = if s.route == Route::Notes && !s.vault_open {
        (
            "No vault yet",
            "Choose a folder and its Markdown files appear here.",
        )
    } else {
        (s.route.empty_title(), s.route.empty_hint())
    };
    let mut body = col()
        .key("detail")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(32.0)
        .center()
        .gap(10.0)
        .child(badge(s.route, k))
        .child(
            text(title)
                .key("detail-title")
                .font(k.ty.ui_medium)
                .color(c.text_base)
                .align(Align::Center)
                .wrap()
                .max_w(360.0),
        )
        .child(
            text(hint)
                .key("detail-hint")
                .font(k.ty.small)
                .color(c.text_muted)
                .align(Align::Center)
                .wrap()
                .max_w(340.0),
        );

    if s.route == Route::Notes {
        if s.vault_open {
            body = body.child(
                text(format!("Vault · {}", s.vault))
                    .key("detail-vault")
                    .font(k.ty.text_xsmall)
                    .color(c.text_faint)
                    .truncate()
                    .max_w(340.0),
            );
        } else {
            body = body.child(
                Button::new("Choose vault…")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut State| s.open_vault_dialog())
                    .build(k)
                    .key("detail-choose-vault"),
            );
        }
    }

    body
}

fn badge(route: Route, k: &Kit) -> El {
    col()
        .key("detail-badge")
        .size(44.0, 44.0)
        .radius(k.radius.lg)
        .center()
        .fill(k.alpha(k.c.bg_accent, 0.12))
        .child(icon(route.icon(), 20.0).color(k.c.bg_accent))
}

/// A single-pane screen: Search and Settings.
fn page_pane(s: &State, k: &Kit, dark: bool) -> El {
    if s.route == Route::Settings {
        settings(s, k, dark)
    } else {
        empty_page(s, k)
    }
}

fn empty_page(s: &State, k: &Kit) -> El {
    let c = &k.c;
    col()
        .key("page-empty")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(32.0)
        .center()
        .gap(10.0)
        .child(badge(s.route, k))
        .child(
            text(s.route.empty_title())
                .key("page-title")
                .font(k.ty.ui_medium)
                .color(c.text_base)
                .align(Align::Center)
                .wrap()
                .max_w(420.0),
        )
        .child(
            text(s.route.empty_hint())
                .key("page-hint")
                .font(k.ty.small)
                .color(c.text_muted)
                .align(Align::Center)
                .wrap()
                .max_w(420.0),
        )
}

fn settings(s: &State, k: &Kit, dark: bool) -> El {
    let c = &k.c;
    let mut panel = col().w_full().max_w(520.0).gap(10.0);

    // A newer release goes first: it is news, and it belongs where the eye lands
    // rather than below the version it is newer than.
    if let Some(version) = s.update.offered() {
        panel = panel.child(update_banner(version, k));
    }

    panel = panel
        .child(text("Workspace").font(k.ty.ui_medium).color(c.text_base))
        .child(vault_setting(s, k))
        .child(setting_row(
            k,
            "Appearance",
            if dark {
                "Dark, following the system"
            } else {
                "Light, following the system"
            },
        ))
        .child(setting_row(k, "Version", s.version))
        .child(update_row(s, k))
        .child(
            text("Read-only for now. These become editable in Settings.")
                .key("settings-note")
                .font(k.ty.text_xsmall)
                .color(c.text_faint)
                .wrap(),
        );

    col()
        .key("settings")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(32.0)
        .items_center()
        .child(panel)
}

/// "openNoteX 0.1.0 is available", with the one thing a person can do about it.
/// The app cannot install an update yet (that is M6c), so the button is honest
/// about what it does: it opens the release page.
fn update_banner(version: &str, k: &Kit) -> El {
    let c = &k.c;
    // The button outlives this frame, so it captures the version by value.
    let target = version.to_owned();
    let open = target.clone();
    col()
        .key("update-banner")
        .w_full()
        .padding(14.0)
        .gap(6.0)
        .radius(k.radius.md)
        .fill(c.layer_02)
        .border(1.0, c.border_weak)
        .child(
            text(format!("openNoteX {version} is available"))
                .key("update-title")
                .font(k.ty.ui_medium)
                .color(c.text_base),
        )
        .child(
            text("This build keeps working. Updates install from the release page.")
                .key("update-hint")
                .font(k.ty.text_xsmall)
                .color(c.text_muted)
                .wrap(),
        )
        .child(
            row().gap(8.0).child(
                Button::new(format!("View openNoteX {target}"))
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut State| s.request_release(open.clone()))
                    .build(k)
                    .key("update-view"),
            ),
        )
}

/// The update check, as a setting: what it last said, and the button that asks.
/// Nothing is asked on launch, which is why the default reads "Not checked".
fn update_row(s: &State, k: &Kit) -> El {
    let c = &k.c;
    row()
        .w_full()
        .py(10.0)
        .px(14.0)
        .gap(12.0)
        .radius(k.radius.md)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .center_y()
        .child(text("Updates").font(k.ty.ui).color(c.text_base))
        .child(
            text(s.update.summary())
                .key("update-status")
                .font(k.ty.small)
                .color(c.text_muted)
                .align(Align::End)
                .truncate()
                .grow(1.0),
        )
        .child(
            Button::new("Check now")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.request_update_check())
                .build(k)
                .key("update-check"),
        )
}

/// Settings' Vault row. A working vault is a fact to read; no vault is a
/// question with the same dialog behind it as first launch.
fn vault_setting(s: &State, k: &Kit) -> El {
    if s.vault_open {
        return setting_row(k, "Vault", &s.vault);
    }
    let c = &k.c;
    row()
        .key("settings-vault-row")
        .w_full()
        .py(10.0)
        .px(14.0)
        .gap(12.0)
        .radius(k.radius.md)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .center_y()
        .justify_between()
        .child(
            col()
                .gap(2.0)
                .child(text("Vault").font(k.ty.ui).color(c.text_base))
                .child(
                    text("Not configured")
                        .key("settings-vault")
                        .font(k.ty.small)
                        .color(c.text_faint),
                ),
        )
        .child(
            Button::new("Choose vault…")
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.open_vault_dialog())
                .build(k)
                .key("settings-choose-vault"),
        )
}

fn setting_row(k: &Kit, title: &str, value: &str) -> El {
    let c = &k.c;
    row()
        .w_full()
        .py(10.0)
        .px(14.0)
        .gap(12.0)
        .radius(k.radius.md)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .center_y()
        .justify_between()
        .child(text(title.to_owned()).font(k.ty.ui).color(c.text_base))
        .child(
            text(value.to_owned())
                .font(k.ty.small)
                .color(c.text_muted)
                .truncate(),
        )
}
