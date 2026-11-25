//! Home: the clock, today's checklist, reminders and recent notes.
//!
//! Three blocks, top to bottom, as wide as the window: the time; one block with
//! Todo (its overdue rows first), Reminders and Next up; then Recent notes.
//! Everything here reads [`State`] and nothing else.

use notex_time::checklist::Bucket;
use rune::{Align, Color, El, ScrollSpec, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Kit};

use crate::routes::Route;
use crate::state::{NoteRow, State, TaskRow};

/// The one scrolling column Home is drawn in.
pub fn home_pane(s: &State, k: &Kit) -> El {
    col()
        .key("home-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .items_center()
        .child(
            col()
                .key("home")
                .w_full()
                .max_w(760.0)
                .padding(24.0)
                .gap(14.0)
                .child(clock_block(s, k))
                .child(focus_block(s, k))
                .child(recent_block(s, k)),
        )
}

/// The full-width time block. It is the first thing on screen, because "what
/// time is it" is the question a home screen answers before anything else.
fn clock_block(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let time = if s.now.time.is_empty() {
        "--:--".to_owned()
    } else {
        s.now.time.clone()
    };
    let date = if s.now.date.is_empty() {
        "Today".to_owned()
    } else {
        s.now.date.clone()
    };
    let open = s.open_task_count();
    let today = s
        .open_tasks()
        .filter(|task| s.bucket_of(task) == Bucket::Today)
        .count();
    let summary = match (open, today) {
        (0, _) => "Nothing open".to_owned(),
        (_, 0) => format!("{open} open"),
        (_, 1) => format!("{open} open \u{b7} 1 due today"),
        (_, count) => format!("{open} open \u{b7} {count} due today"),
    };

    row()
        .key("home-clock")
        .w_full()
        .padding(20.0)
        .gap(16.0)
        .center_y()
        .radius(k.radius.lg)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .child(
            text(time)
                .key("home-time")
                .font(rune::TextStyle {
                    size: 34.0,
                    weight: 600.0,
                    ..k.ty.ui
                })
                .color(c.text_base),
        )
        .child(
            col()
                .key("home-date-block")
                .gap(2.0)
                .child(
                    text(date)
                        .key("home-date")
                        .font(k.ty.small)
                        .color(c.text_muted),
                )
                .child(
                    text(summary)
                        .key("home-summary")
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint),
                ),
        )
        .child(el().grow(1.0))
        .child(
            Button::new("Open tasks")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut State| s.route = Route::Tasks)
                .build(k)
                .key("home-open-tasks"),
        )
}

/// Todo, Reminders and Next up, in one block, separated by hairlines.
fn focus_block(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let today = s.now.day.as_str();
    let overdue: Vec<&TaskRow> = s
        .open_tasks()
        .filter(|task| s.bucket_of(task) == Bucket::Overdue)
        .collect();
    let due_today: Vec<&TaskRow> = s
        .open_tasks()
        .filter(|task| s.bucket_of(task) == Bucket::Today)
        .collect();
    let next: Vec<&TaskRow> = s
        .open_tasks()
        .filter(|task| s.bucket_of(task) == Bucket::Upcoming)
        .take(3)
        .collect();

    let todo_count = overdue.len() + due_today.len();
    let mut todo = col().key("home-todo").w_full().gap(2.0);
    for task in overdue.iter().take(3) {
        todo = todo.child(home_task(task, today, k));
    }
    for task in due_today.iter().take(4) {
        todo = todo.child(home_task(task, today, k));
    }
    if todo_count == 0 {
        todo = todo.child(empty_line(
            if s.open_task_count() == 0 {
                "No tasks yet. Add one in Tasks."
            } else {
                "Nothing due today."
            },
            k,
        ));
    }

    let mut block = col()
        .key("home-focus")
        .w_full()
        .padding(16.0)
        .gap(10.0)
        .radius(k.radius.lg)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .child(section(
            "Todo",
            "home-todo-head",
            if todo_count > 0 {
                Some(todo_count.to_string())
            } else {
                None
            },
            Some(
                Button::new("View all")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut State| s.route = Route::Tasks)
                    .build(k)
                    .key("home-todo-all"),
            ),
            k,
        ))
        .child(todo)
        .child(rule("home-rule-reminders", k))
        .child(section("Reminders", "home-reminders-head", None, None, k))
        .child(empty_line(
            "Nothing here yet. Reminders arrive in a later build.",
            k,
        ))
        .child(rule("home-rule-next", k))
        .child(section("Next up", "home-next-head", None, None, k));

    if next.is_empty() {
        block = block.child(empty_line("Nothing scheduled.", k));
    } else {
        let mut list = col().key("home-next").w_full().gap(2.0);
        for task in next {
            list = list.child(home_task(task, today, k));
        }
        block = block.child(list);
    }
    block
}

/// Recent notes, by the file's modification time. The note list is alphabetical;
/// this block is the one place recency is the order.
fn recent_block(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let notes = s.recent_notes(4);
    let block = col()
        .key("home-recent")
        .w_full()
        .padding(16.0)
        .gap(8.0)
        .radius(k.radius.lg)
        .fill(c.layer_01)
        .border(1.0, c.border_weak)
        .child(section(
            "Recent notes",
            "home-recent-head",
            None,
            Some(
                Button::new("View all")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut State| s.route = Route::Notes)
                    .build(k)
                    .key("home-recent-all"),
            ),
            k,
        ));

    if notes.is_empty() {
        return block.child(empty_line(
            if s.vault_open {
                "No notes yet."
            } else {
                "Choose a vault to see your notes."
            },
            k,
        ));
    }

    let mut list = col().key("home-notes").w_full().gap(2.0);
    for note in notes {
        list = list.child(home_note(note, k));
    }
    block.child(list)
}

fn home_note(note: &NoteRow, k: &Kit) -> El {
    let id = note.id.clone();
    let tags = note.tag_line();
    let mut line = col()
        .key(format!("home-note-{}", note.id))
        .w_full()
        .px(8.0)
        .py(5.0)
        .gap(1.0)
        .radius(k.radius.sm)
        .label(note.title.clone())
        .focusable(true)
        .hover_group()
        .on_press_tx(move |s: &mut State, tx| {
            s.route = Route::Notes;
            s.select_note(&id);
            s.flush(tx);
        });
    line = line.child(
        text(note.title.clone())
            .font(k.ty.ui)
            .color(k.c.text_base)
            .truncate(),
    );
    if !tags.is_empty() {
        line = line.child(
            text(tags)
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint)
                .truncate(),
        );
    }
    k.interactive(line)
}

fn home_task(task: &TaskRow, today: &str, k: &Kit) -> El {
    let c = &k.c;
    let open_id = task.id.clone();
    let check_id = task.id.clone();
    let mut line = row()
        .key(format!("home-task-{}", task.id))
        .w_full()
        .px(8.0)
        .py(3.0)
        .gap(8.0)
        .center_y()
        .radius(k.radius.sm)
        .label(format!("Open \u{201c}{}\u{201d}", task.title))
        .focusable(true)
        .hover_group()
        .on_press(move |s: &mut State| s.show_task(&open_id))
        .child(
            Checkbox::new(task.done)
                .label(format!("Mark \u{201c}{}\u{201d} done", task.title))
                .hide_label()
                .on_toggle_tx(move |s: &mut State, tx| {
                    s.toggle_task(&check_id);
                    s.flush(tx);
                })
                .build(k)
                .key(format!("home-check-{}", task.id)),
        )
        .child(
            text(task.title.clone())
                .key(format!("home-task-title-{}", task.id))
                .font(k.ty.ui)
                .color(c.text_base)
                .truncate()
                .grow(1.0),
        );
    if let Some(due) = task.due(today) {
        line = line.child(
            text(due.text)
                .key(format!("home-due-{}", task.id))
                .font(k.ty.text_xsmall)
                .color(if due.late { c.danger.fg } else { c.text_faint }),
        );
    }
    if let Some(name) = &task.list_name {
        line = line.child(
            row()
                .key(format!("home-project-{}", task.id))
                .gap(4.0)
                .center_y()
                .child(el().size(6.0, 6.0).radius(3.0).fill(project_color(task, k)))
                .child(
                    text(name.clone())
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint),
                ),
        );
    }
    k.interactive(line)
}

/// A project's colour dot: its stored colour when that parses, else the accent.
fn project_color(task: &TaskRow, k: &Kit) -> Color {
    task.list_color
        .as_deref()
        .and_then(parse_color)
        .unwrap_or(k.c.bg_accent)
}

/// `#rrggbb`, the spelling the store keeps.
pub fn parse_color(value: &str) -> Option<Color> {
    let color = rune::color::hex(value)?;
    // The store only ever writes opaque project colours.
    (color.a >= 1.0).then_some(color)
}

/// A block's heading: a title, an optional count and an optional action.
fn section(title: &str, key: &str, count: Option<String>, action: Option<El>, k: &Kit) -> El {
    let c = &k.c;
    let mut line = row().key(key).w_full().gap(8.0).center_y().child(
        text(title.to_owned())
            .font(k.ty.ui_medium)
            .color(c.text_base),
    );
    if let Some(count) = count {
        line = line.child(
            row()
                .key(format!("{key}-count"))
                .h(16.0)
                .px(5.0)
                .center()
                .radius(8.0)
                .fill(c.layer_03)
                .child(text(count).font(k.ty.small_medium).color(c.text_muted)),
        );
    }
    line = line.child(el().grow(1.0));
    if let Some(action) = action {
        line = line.child(action);
    }
    line
}

fn rule(key: &str, k: &Kit) -> El {
    el().key(key).w_full().h(1.0).fill(k.c.border_weak)
}

fn empty_line(message: &str, k: &Kit) -> El {
    text(message.to_owned())
        .font(k.ty.small)
        .color(k.c.text_faint)
        .wrap()
        .px(8.0)
        .py(4.0)
        .align(Align::Start)
}
