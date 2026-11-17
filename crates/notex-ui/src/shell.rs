//! The window: sidebar, list, detail.
//!
//! Nothing here reads the clock, the database or the filesystem. Handlers only
//! mutate [`State`], and the next frame is the layout of that state.

use rune::{Align, Cx, El, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::shell::{
    RowStatus, Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Spinner};

use crate::routes::{FOOTER, Route, SECTIONS};
use crate::state::{Draft, DraftStatus, Load, NoteBody, State};
use crate::theme;

/// The window. `AppSpec::view`.
pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let k = theme::kit(dark);

    // A list screen has a detail pane; a single-pane screen does not.
    let split = if s.route.has_detail() {
        SplitView::new(list_pane(s, &k)).inspector(
            &s.detail,
            |s: &mut State| &mut s.detail,
            detail_pane(s, &k),
        )
    } else {
        SplitView::new(page_pane(s, &k, dark))
    };

    split
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
        .build(&k)
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
    SidebarRow::new(route.label())
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
                .child(
                    text(subtitle)
                        .key("list-subtitle")
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint)
                        .truncate(),
                ),
        )
        .child(el().key("list-divider").w_full().h(1.0).fill(c.border_weak))
        .child(match &s.load {
            Load::Ready if s.route == Route::Notes && !s.notes.is_empty() => note_list(s, k),
            Load::Ready => empty_list(s, k),
            Load::Loading => loading(k),
            Load::Failed(message) => failed_message(k, message),
        })
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
            text(title)
                .key("detail-title")
                .font(k.ty.ui_medium)
                .color(c.text_base)
                .wrap(),
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
            text(s.route.empty_title())
                .key("detail-title")
                .font(k.ty.ui_medium)
                .color(c.text_base)
                .align(Align::Center)
                .wrap()
                .max_w(360.0),
        )
        .child(
            text(s.route.empty_hint())
                .key("detail-hint")
                .font(k.ty.small)
                .color(c.text_muted)
                .align(Align::Center)
                .wrap()
                .max_w(340.0),
        );

    if s.route == Route::Notes {
        body = body.child(
            text(format!("Vault · {}", s.vault))
                .key("detail-vault")
                .font(k.ty.text_xsmall)
                .color(c.text_faint)
                .truncate()
                .max_w(340.0),
        );
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
    col()
        .key("settings")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(32.0)
        .items_center()
        .child(
            col()
                .w_full()
                .max_w(520.0)
                .gap(10.0)
                .child(text("Workspace").font(k.ty.ui_medium).color(c.text_base))
                .child(setting_row(k, "Vault", &s.vault))
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
                .child(
                    text("Read-only for now. These become editable in Settings.")
                        .key("settings-note")
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint)
                        .wrap(),
                ),
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
