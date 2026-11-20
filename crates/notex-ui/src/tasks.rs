//! The Tasks screen: one checklist, projects as a filter.
//!
//! A row is a checkbox, a title, a due chip and its project. Clicking the title
//! renames it in place; the chevron opens its notes, due buttons and project
//! picks; the trash soft-deletes it and the toast holds the way back. Done tasks
//! move to the archive, grouped by the day they were completed.

use notex_time::checklist::{self, Bucket};
use rune::{Align, Color, El, Key, KeyEvent, ScrollSpec, col, el, icon, row, text, text_input};
use rune_kit::{
    Button, ButtonVariant, Checkbox, ControlSize, IconButton, IconButtonVariant, Kit, Toast,
    ToastRegion,
};

use crate::home::parse_color;
use crate::state::{State, TaskRow};

/// The whole screen: header, filter chips, the add row and the list.
pub fn tasks_pane(s: &State, k: &Kit) -> El {
    let c = &k.c;
    col()
        .key("tasks-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(header(s, k))
        .child(
            el().key("tasks-divider")
                .w_full()
                .h(1.0)
                .fill(c.border_weak),
        )
        .children(s.task_error.as_deref().map(|error| error_line(error, k)))
        // The archive is a place to look and reopen, not a place to add: its
        // chips and add row would be about a list it is not showing.
        .children((!s.archive).then(|| filters(s, k)))
        .children((!s.archive).then(|| add_row(s, k)))
        .child(if s.archive {
            archive(s, k)
        } else {
            checklist(s, k)
        })
}

fn header(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let open = s.open_task_count();
    let done = s.done_task_count();
    let subtitle = if s.archive {
        match done {
            0 => "Nothing archived".to_owned(),
            1 => "1 done".to_owned(),
            count => format!("{count} done"),
        }
    } else if s.tasks.is_empty() {
        "No tasks yet".to_owned()
    } else if done == 0 {
        format!("{open} open")
    } else {
        format!("{open} open \u{b7} {done} done")
    };
    row()
        .key("tasks-header")
        .w_full()
        .px(16.0)
        .pt(6.0)
        .pb(8.0)
        .gap(8.0)
        .center_y()
        .child(
            text(subtitle)
                .key("tasks-subtitle")
                .font(k.ty.caption())
                .color(c.text_faint)
                .truncate()
                .grow(1.0),
        )
        .child(
            Button::new(if s.archive {
                "Back to tasks"
            } else {
                "Archive"
            })
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .icon(&crate::icons::ARCHIVE)
            .on_press(|s: &mut State| s.archive = !s.archive)
            .build(k)
            .key("tasks-archive"),
        )
}

/// A create or write the worker refused, above the chips.
fn error_line(message: &str, k: &Kit) -> El {
    row()
        .key("task-error")
        .w_full()
        .px(16.0)
        .py(6.0)
        .gap(6.0)
        .child(icon(&crate::icons::WARNING, 12.0).color(k.c.danger.fg))
        .child(
            text(message.to_owned())
                .key("task-error-text")
                .font(k.ty.caption())
                .color(k.c.danger.fg)
                .wrap(),
        )
}

fn filters(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let mut line = row()
        .key("task-filters")
        .w_full()
        .px(16.0)
        .pt(8.0)
        .pb(6.0)
        .gap(6.0)
        .flex_wrap()
        .child(chip(
            "task-filter-all",
            "All".to_owned(),
            s.project_filter.is_none(),
            None,
            k,
            |s: &mut State| s.set_project_filter(None),
        ));

    for project in &s.projects {
        let id = project.id.clone();
        line = line.child(chip(
            format!("task-filter-{}", project.id),
            project.name.clone(),
            s.project_filter.as_deref() == Some(project.id.as_str()),
            project.color.as_deref().and_then(parse_color),
            k,
            move |s: &mut State| s.set_project_filter(Some(id.clone())),
        ));
    }

    match &s.add_project {
        Some(edit) => {
            let field = text_input(edit, k.ty.text_xsmall)
                .single_line()
                .key("task-project-input")
                .label("New project name")
                .w(130.0)
                .autofocus()
                .color(c.text_base)
                .caret(c.text_base, k.selection())
                .placeholder("Project name".to_owned(), c.text_faint)
                .on_edit(|s: &mut State| s.add_project.as_mut().unwrap())
                .on_submit(|s: &mut State, _| s.commit_project())
                .on_input_key(|s: &mut State, ev: &KeyEvent, _| {
                    if ev.key == Key::Escape {
                        s.cancel_project();
                        true
                    } else {
                        false
                    }
                });
            line = line.child(field);
        }
        None => {
            line = line.child(chip(
                "task-filter-new",
                "+ Project".to_owned(),
                false,
                None,
                k,
                |s: &mut State| s.begin_project(),
            ));
        }
    }
    line
}

/// A filter chip: a pill with an optional project dot.
fn chip(
    key: impl Into<String>,
    label: String,
    selected: bool,
    dot: Option<Color>,
    k: &Kit,
    press: impl Fn(&mut State) + 'static,
) -> El {
    let c = &k.c;
    let mut pill = row()
        .key(key)
        .label(label.clone())
        .h(22.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(11.0)
        .focusable(true)
        .focus_selected(selected)
        .fill(if selected {
            c.pressed
        } else {
            Color::TRANSPARENT
        })
        .border(
            1.0,
            if selected {
                c.border_weak
            } else {
                Color::TRANSPARENT
            },
        )
        .child(
            text(label.clone())
                .font(k.ty.text_xsmall)
                .color(if selected { c.text_base } else { c.text_muted }),
        );
    if let Some(color) = dot {
        pill = pill.child(el().size(6.0, 6.0).radius(3.0).fill(color));
    }
    k.interactive(pill).on_press(press)
}

/// The borderless add row. Return adds the task and leaves the caret in the
/// field, so a run of tasks is a run of lines.
fn add_row(s: &State, k: &Kit) -> El {
    let c = &k.c;
    let field = text_input(&s.add_task, k.ty.ui_compact)
        .single_line()
        .key("task-add-input")
        .label("Add a task")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .color(c.text_base)
        .caret(c.text_base, k.selection())
        .placeholder("Add a task\u{2026}  #project".to_owned(), c.text_faint)
        .on_edit(|s: &mut State| &mut s.add_task)
        .on_submit(|s: &mut State, _| s.add_task_now());
    row()
        .key("task-add")
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(8.0)
        .center_y()
        .child(icon(&crate::icons::PLUS, 14.0).color(c.icon_faint))
        .child(field)
}

fn checklist(s: &State, k: &Kit) -> El {
    let groups: Vec<(Bucket, Vec<&TaskRow>)> = Bucket::ALL
        .into_iter()
        .map(|bucket| (bucket, s.tasks_in(bucket)))
        .filter(|(_, rows)| !rows.is_empty())
        .collect();

    if groups.is_empty() {
        return empty_list(s, k);
    }

    let mut list = col()
        .key("task-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(8.0)
        .pb(24.0)
        .gap(1.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    for (bucket, rows) in groups {
        list = list.child(group_head(bucket, rows.len(), k));
        for task in rows {
            list = list.child(task_row(s, task, k));
        }
    }
    list
}

fn empty_list(s: &State, k: &Kit) -> El {
    let filtered = s.project_filter.is_some();
    let (title, hint) = if filtered {
        ("Nothing in this project", "Add a task above, or pick All.")
    } else if s.tasks.is_empty() {
        (
            "No tasks yet",
            "Add a task above. Type #project to file it.",
        )
    } else {
        (
            "Everything is done",
            "Completed tasks are in the archive, with a way back.",
        )
    };
    col()
        .key("task-list-empty")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .center()
        .gap(8.0)
        .child(icon(&crate::icons::TASKS, 20.0).color(k.c.icon_faint))
        .child(text(title).font(k.ty.small).color(k.c.text_faint))
        .child(
            text(hint)
                .key("task-list-empty-hint")
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint)
                .align(Align::Center)
                .wrap()
                .max_w(280.0),
        )
}

fn group_head(bucket: Bucket, count: usize, k: &Kit) -> El {
    row()
        .key(format!("task-group-{}", bucket.key()))
        .w_full()
        .px(8.0)
        .pt(12.0)
        .pb(4.0)
        .gap(6.0)
        .center_y()
        .child(
            text(bucket.title())
                .font(k.ty.small_medium)
                .color(k.c.text_faint),
        )
        .child(
            text(count.to_string())
                .key(format!("task-group-{}-count", bucket.key()))
                .font(k.ty.small)
                .color(k.c.text_faint),
        )
}

fn task_row(s: &State, task: &TaskRow, k: &Kit) -> El {
    let c = &k.c;
    let editing = s.task_edit.as_ref().filter(|edit| edit.id == task.id);
    let expanded = s.expanded.as_deref() == Some(task.id.as_str());

    let check = task.id.clone();
    let mut line = row()
        .key(format!("task-row-{}", task.id))
        .w_full()
        .min_h(30.0)
        .px(8.0)
        .py(3.0)
        .gap(8.0)
        .center_y()
        .radius(k.radius.md)
        .hover_group()
        .child(
            Checkbox::new(task.done)
                .label(format!("Mark \u{201c}{}\u{201d} done", task.title))
                .hide_label()
                .on_toggle(move |s: &mut State| s.toggle_task(&check))
                .build(k)
                .key(format!("task-check-{}", task.id)),
        );

    line = match editing {
        Some(edit) => line.child(
            text_input(&edit.edit, k.ty.ui)
                .single_line()
                .key(format!("task-edit-{}", task.id))
                .label("Task title")
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .autofocus()
                .color(c.text_base)
                .caret(c.text_base, k.selection())
                .on_edit(|s: &mut State| &mut s.task_edit.as_mut().unwrap().edit)
                .on_submit(|s: &mut State, _| s.commit_edit())
                .on_input_key(|s: &mut State, ev: &KeyEvent, _| {
                    if ev.key == Key::Escape {
                        s.cancel_edit();
                        true
                    } else {
                        false
                    }
                }),
        ),
        None => {
            let title_id = task.id.clone();
            line.child(
                text(task.title.clone())
                    .key(format!("task-title-{}", task.id))
                    .font(k.ty.ui)
                    .color(c.text_base)
                    .truncate()
                    .on_press(move |s: &mut State| s.begin_edit(&title_id)),
            )
        }
    };

    if let Some(due) = task.due(&s.now.day) {
        let id = task.id.clone();
        line = line.child(
            row()
                .key(format!("task-due-{}", task.id))
                .h(20.0)
                .px(6.0)
                .center_y()
                .radius(10.0)
                .label(format!("Due {}", due.text))
                .focusable(true)
                .hover(c.hover)
                .on_press(move |s: &mut State| s.toggle_expanded(&id))
                .child(text(due.text).font(k.ty.caption()).color(if due.late {
                    c.danger.fg
                } else {
                    c.text_faint
                })),
        );
    }

    if let (Some(name), Some(id)) = (&task.list_name, &task.list_id) {
        let filter = id.clone();
        line = line.child(
            row()
                .key(format!("task-project-{}", task.id))
                .h(20.0)
                .px(4.0)
                .gap(4.0)
                .center_y()
                .radius(10.0)
                .label(format!("Filter by {name}"))
                .focusable(true)
                .hover(c.hover)
                .on_press(move |s: &mut State| s.set_project_filter(Some(filter.clone())))
                .child(
                    el().size(6.0, 6.0).radius(3.0).fill(
                        task.list_color
                            .as_deref()
                            .and_then(parse_color)
                            .unwrap_or(c.bg_accent),
                    ),
                )
                .child(
                    text(name.clone())
                        .font(k.ty.text_xsmall)
                        .color(c.text_faint),
                ),
        );
    }

    let expand = task.id.clone();
    let remove = task.id.clone();
    let actions = row()
        .child(el().grow(1.0))
        .key(format!("task-actions-{}", task.id))
        .gap(2.0)
        .center_y()
        .group_opacity(0.0, 1.0)
        .child(
            IconButton::new(if expanded {
                k.glyph.chevron_up
            } else {
                k.glyph.chevron_down
            })
            .variant(IconButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .label(if expanded {
                "Hide details"
            } else {
                "Show details"
            })
            .on_press(move |s: &mut State| s.toggle_expanded(&expand))
            .build(k)
            .key(format!("task-expand-{}", task.id)),
        )
        .child(
            IconButton::new(&crate::icons::TRASH)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Delete task")
                .on_press(move |s: &mut State| s.delete_task(&remove))
                .build(k)
                .key(format!("task-delete-{}", task.id)),
        );
    line = line.child(actions);

    if expanded {
        col()
            .key(format!("task-expanded-{}", task.id))
            .w_full()
            .gap(2.0)
            .child(line)
            .child(expanded_body(s, task, k))
    } else {
        line
    }
}

/// Notes, dates and the project picker, under one row.
fn expanded_body(s: &State, task: &TaskRow, k: &Kit) -> El {
    let c = &k.c;
    let mut body = col()
        .key(format!("task-details-{}", task.id))
        .w_full()
        .pb(10.0)
        .pl(32.0)
        .pr(8.0)
        .gap(8.0)
        .child(text("DUE").font(k.ty.text_xsmall).color(c.text_faint));

    let mut due_row = row()
        .key(format!("task-due-row-{}", task.id))
        .gap(6.0)
        .flex_wrap();
    for (label, days) in [("Today", 0), ("Tomorrow", 1), ("Next week", 7)] {
        let Some(day) = checklist::add_days(&s.now.day, days) else {
            continue;
        };
        let id = task.id.clone();
        due_row = due_row.child(
            Button::new(label)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut State| s.set_due(&id, Some(day.clone())))
                .build(k)
                .key(format!("task-due-{}-{}", days, task.id)),
        );
    }
    if task.due_day.is_some() {
        let id = task.id.clone();
        due_row = due_row.child(
            Button::new("Clear")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut State| s.set_due(&id, None))
                .build(k)
                .key(format!("task-due-clear-{}", task.id)),
        );
    }
    body = body.child(due_row).child(
        text("PROJECT")
            .key(format!("task-project-label-{}", task.id))
            .font(k.ty.text_xsmall)
            .color(c.text_faint),
    );

    let mut project_row = row()
        .key(format!("task-project-row-{}", task.id))
        .gap(6.0)
        .flex_wrap();
    let all = task.id.clone();
    project_row = project_row.child(
        Button::new("No project")
            .variant(if task.list_id.is_none() {
                ButtonVariant::Outline
            } else {
                ButtonVariant::Ghost
            })
            .control_size(ControlSize::Small)
            .on_press(move |s: &mut State| s.set_project(&all, None))
            .build(k)
            .key(format!("task-project-none-{}", task.id)),
    );
    for project in &s.projects {
        let id = task.id.clone();
        let list = project.id.clone();
        project_row = project_row.child(
            Button::new(project.name.clone())
                .variant(if task.list_id.as_deref() == Some(project.id.as_str()) {
                    ButtonVariant::Outline
                } else {
                    ButtonVariant::Ghost
                })
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut State| s.set_project(&id, Some(list.clone())))
                .build(k)
                .key(format!("task-project-{}-{}", project.id, task.id)),
        );
    }
    body = body.child(project_row);

    if let Some(note) = s.task_note.as_ref().filter(|note| note.id == task.id) {
        body = body
            .child(
                text("NOTES")
                    .key(format!("task-notes-label-{}", task.id))
                    .font(k.ty.text_xsmall)
                    .color(c.text_faint),
            )
            .child(
                text_input(&note.edit, k.ty.ui_compact)
                    .key(format!("task-notes-{}", task.id))
                    .label("Task notes")
                    .w_full()
                    .h(34.0)
                    .color(c.text_base)
                    .caret(c.text_base, k.selection())
                    .placeholder("A line of context\u{2026}".to_owned(), c.text_faint)
                    .on_edit(|s: &mut State| &mut s.task_note.as_mut().unwrap().edit),
            )
            .children(notes_actions(task, note, k));
    }
    body
}

/// The status and the save button, only while there is something to save. An
/// untouched field shows nothing, so the row stays quiet.
fn notes_actions(task: &TaskRow, note: &crate::state::TaskNote, k: &Kit) -> Option<El> {
    if !note.saving && note.edit.text == task.notes {
        return None;
    }
    Some(
        row()
            .key(format!("task-notes-actions-{}", task.id))
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                text(if note.saving {
                    "Saving\u{2026}"
                } else {
                    "Not saved"
                })
                .font(k.ty.text_xsmall)
                .color(k.c.text_faint),
            )
            .child(el().grow(1.0))
            .child(
                Button::new("Save note")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .on_press(|s: &mut State| s.save_task_note())
                    .build(k)
                    .key(format!("task-notes-save-{}", task.id)),
            ),
    )
}

fn archive(s: &State, k: &Kit) -> El {
    let done = s.archived();
    if done.is_empty() {
        return col()
            .key("archive-empty")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(8.0)
            .child(icon(&crate::icons::ARCHIVE, 20.0).color(k.c.icon_faint))
            .child(
                text("Nothing archived yet")
                    .font(k.ty.small)
                    .color(k.c.text_faint),
            )
            .child(
                text("Completing a task moves it here, grouped by the day it was done.")
                    .key("archive-empty-hint")
                    .font(k.ty.text_xsmall)
                    .color(k.c.text_faint)
                    .align(Align::Center)
                    .wrap()
                    .max_w(280.0),
            );
    }

    let mut list = col()
        .key("archive-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(8.0)
        .pb(24.0)
        .gap(1.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    let mut current: Option<String> = None;
    for task in done {
        let day = task
            .completed_day
            .as_deref()
            .map(|day| checklist::day_label(day, &s.now.day))
            .unwrap_or_else(|| "Earlier".to_owned());
        if current.as_deref() != Some(day.as_str()) {
            list = list.child(
                row()
                    .w_full()
                    .key(format!(
                        "archive-day-{}",
                        task.completed_day.as_deref().unwrap_or("earlier")
                    ))
                    .px(8.0)
                    .pt(12.0)
                    .pb(4.0)
                    .child(
                        text(day.clone())
                            .font(k.ty.small_medium)
                            .color(k.c.text_faint),
                    ),
            );
            current = Some(day);
        }
        list = list.child(archive_row(task, k));
    }
    list
}

fn archive_row(task: &TaskRow, k: &Kit) -> El {
    let c = &k.c;
    let check = task.id.clone();
    let restore = task.id.clone();
    row()
        .key(format!("archive-row-{}", task.id))
        .w_full()
        .min_h(30.0)
        .px(8.0)
        .py(3.0)
        .gap(8.0)
        .center_y()
        .radius(k.radius.md)
        .hover_group()
        .child(
            Checkbox::new(true)
                .label(format!("Reopen \u{201c}{}\u{201d}", task.title))
                .hide_label()
                .on_toggle(move |s: &mut State| s.toggle_task(&check))
                .build(k)
                .key(format!("archive-check-{}", task.id)),
        )
        .child(
            text(task.title.clone())
                .key(format!("archive-title-{}", task.id))
                .font(k.ty.ui)
                .color(c.text_muted)
                .truncate()
                .grow(1.0),
        )
        .child(
            IconButton::new(&crate::icons::RESTORE)
                .variant(IconButtonVariant::GhostMuted)
                .control_size(ControlSize::Small)
                .label("Reopen task")
                .on_press(move |s: &mut State| s.toggle_task(&restore))
                .build(k)
                .key(format!("archive-restore-{}", task.id)),
        )
}

/// The eight-second toast, when one is up. Mounted in the window layer so it is
/// not clipped by the list.
pub fn undo_toast(s: &State, k: &Kit) -> Option<El> {
    let undo = s.undo.as_ref()?;
    let toast = Toast::new("task-undo", undo.message.clone())
        .description("Undo for a few seconds.")
        .action(k, "Undo", false, |s: &mut State, _| s.undo_task())
        .on_close(|s: &mut State| s.dismiss_undo())
        .build(k);
    Some(ToastRegion::new().toast(toast).build(k))
}
