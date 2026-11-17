//! The window: sidebar, list, detail.
//!
//! Nothing here reads the clock, the database or the filesystem. Handlers only
//! mutate [`State`], and the next frame is the layout of that state.

use rune::{Align, Cx, El, KeyEvent, ScrollSpec, Tx, col, el, icon, row, text};
use rune_kit::markdown::Markdown;
use rune_kit::shell::{Sidebar, SidebarRow, SidebarSection, SidebarToggle, SplitView, Toolbar};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Spinner};

use crate::routes::{FOOTER, Route, SECTIONS};
use crate::state::{Load, NoteBody, State};
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

/// The open note: its title, its tags and its Markdown, rendered from the file.
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

    let body = match &s.body {
        NoteBody::Ready(body) => col()
            .key("detail-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .px(32.0)
            .pt(16.0)
            .pb(32.0)
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(
                Markdown::new(body_without_title(body))
                    .selectable(true)
                    .build(k)
                    .key("detail-body")
                    .w_full(),
            ),
        NoteBody::Loading => centered(
            "detail-loading",
            Spinner::new().size(16.0).build(k),
            "Reading the file…",
            k,
        ),
        NoteBody::Failed(error) => centered(
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
        NoteBody::None => centered(
            "detail-none",
            icon(s.route.icon(), 20.0).color(c.icon_faint),
            "Select a note to read it.",
            k,
        ),
    };

    col()
        .key("detail")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(head)
        .child(el().key("detail-rule").w_full().h(1.0).fill(c.border_weak))
        .child(body)
}

/// A note's first `# Heading` is its title, and the header above the body already
/// shows it. Drop that one heading so the title is not printed twice.
fn body_without_title(body: &str) -> &str {
    let body = body.trim_start_matches('\n');
    let Some(rest) = body.strip_prefix("# ") else {
        return body;
    };
    match rest.find('\n') {
        Some(end) => rest[end + 1..].trim_start_matches('\n'),
        None => "",
    }
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

#[cfg(test)]
mod tests {
    use super::body_without_title;

    #[test]
    fn only_a_leading_title_heading_is_dropped() {
        assert_eq!(body_without_title("# Title\n\nBody"), "Body");
        assert_eq!(body_without_title("\n# Title\nBody"), "Body");
        assert_eq!(body_without_title("# Only a title"), "");
        assert_eq!(
            body_without_title("Body\n# not a title"),
            "Body\n# not a title"
        );
        assert_eq!(
            body_without_title("## Subtitle\n\nBody"),
            "## Subtitle\n\nBody"
        );
    }
}
