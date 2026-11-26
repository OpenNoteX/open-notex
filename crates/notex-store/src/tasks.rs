//! Tasks and their projects.
//!
//! Tasks are structured data: SQLite is the owner, so unlike a note body there
//! is no file to rebuild from. Every mutation writes its oplog row in the same
//! transaction (rule 5), which is what undo and a future export read.
//!
//! A **project** is a row in `lists`. There is no third container: a task has a
//! title, an optional project, an optional day, and a done flag. That is the
//! whole model a checklist needs.

use notex_core::ItemId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::db::StoreError;
use crate::oplog;

/// One task, joined with the project it is filed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub done: bool,
    pub list_id: Option<String>,
    pub list_name: Option<String>,
    pub list_color: Option<String>,
    /// `YYYY-MM-DD` for an all-day task. Timed due dates are a later slice.
    pub due_day: Option<String>,
    pub completed_at: Option<i64>,
    pub created_at: i64,
    pub rev: i64,
    /// The task this is a step of. A step is a task row, so it has the same
    /// title, notes and done flag; it only differs in where the UI puts it.
    pub parent_id: Option<String>,
}

/// A project. `position` is the order the filter chips are shown in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub position: i64,
}

/// A new task plus the project it created, when `#name` was new.
pub struct Created {
    pub task: Task,
    pub list: Option<List>,
}

/// The answer to completing a task: the row, plus the steps that completion
/// closed. Only steps that were open are in `closed_steps`, so undoing the
/// completion reopens exactly what it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub task: Task,
    pub closed_steps: Vec<Task>,
}

/// A deleted project and the tasks it released. The ids are what undo needs to
/// put each task back where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListGone {
    pub list: List,
    pub task_ids: Vec<String>,
}

const SELECT: &str = "\
SELECT t.id, t.title, t.notes, t.status, t.list_id, l.name, l.color,
       t.due_day, t.completed_at, t.created_at, t.rev, t.parent_id
FROM tasks t
LEFT JOIN lists l ON l.id = t.list_id AND l.deleted_at IS NULL";

/// Every task that is not deleted, open first, then by day and age.
pub fn tasks(conn: &Connection) -> Result<Vec<Task>, StoreError> {
    let sql = format!(
        "{SELECT} WHERE t.deleted_at IS NULL ORDER BY (t.status = 'done'), t.due_day IS NULL, t.due_day, t.created_at, t.rowid"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Every project that is not deleted, in sidebar order.
pub fn lists(conn: &Connection) -> Result<Vec<List>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, color, position FROM lists
         WHERE deleted_at IS NULL ORDER BY position, name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(List {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            position: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn task(conn: &Connection, id: &str) -> Result<Option<Task>, StoreError> {
    let sql = format!("{SELECT} WHERE t.id = ?1 AND t.deleted_at IS NULL");
    Ok(conn.query_row(&sql, [id], row_to_task).optional()?)
}

/// Create a task. `project` is the name typed after `#` in the add row: an
/// existing project is reused (case-insensitively), a new one is created in the
/// same transaction, so a task never arrives with a dangling project.
pub fn create(
    conn: &mut Connection,
    title: &str,
    project: Option<&str>,
    due_day: Option<&str>,
    now: i64,
    device_id: &str,
) -> Result<Created, StoreError> {
    let tx = conn.transaction()?;
    let mut created_list = None;
    let list_id = match project.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => {
            let (list, fresh) = get_or_create_list(&tx, name, now, device_id)?;
            if fresh {
                created_list = Some(list.clone());
            }
            Some(list.id)
        }
        None => None,
    };

    let id = ItemId::new().to_string();
    tx.execute(
        "INSERT INTO tasks(id, list_id, title, status, due_day, due_all_day,
                           created_at, updated_at, sort_key)
         VALUES (?1, ?2, ?3, 'open', ?4, ?5, ?6, ?6, '')",
        params![
            id,
            list_id,
            title,
            due_day,
            i32::from(due_day.is_some()),
            now
        ],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("title", title)]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", &id, "create")
        },
    )?;
    let task = task(&tx, &id)?.expect("the task just inserted");
    tx.commit()?;
    Ok(Created {
        task,
        list: created_list,
    })
}

/// Add a step under a task. A step is an ordinary task row with `parent_id`
/// set: it inherits the parent's project (so filtering and export agree) and
/// has no due day of its own. A tree stops at [`notex_core::MAX_TASK_DEPTH`]
/// levels, and only an open task takes steps.
pub fn create_subtask(
    conn: &mut Connection,
    parent_id: &str,
    title: &str,
    now: i64,
    device_id: &str,
) -> Result<Task, StoreError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(StoreError::EmptyStepTitle);
    }
    let tx = conn.transaction()?;
    let Some(parent) = task(&tx, parent_id)? else {
        return Err(StoreError::MissingTask);
    };
    if depth_of(&tx, parent_id)? >= notex_core::MAX_TASK_DEPTH {
        return Err(StoreError::NestedStep);
    }
    if parent.done {
        return Err(StoreError::TaskIsDone);
    }
    let id = ItemId::new().to_string();
    tx.execute(
        "INSERT INTO tasks(id, parent_id, list_id, title, status,
                           created_at, updated_at, sort_key)
         VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, '')",
        params![id, parent_id, parent.list_id, title, now],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("title", title), ("parent_id", parent_id)]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", &id, "create")
        },
    )?;
    let step = task(&tx, &id)?.expect("the step just inserted");
    tx.commit()?;
    Ok(step)
}

/// Rename a task. `None` means the row is gone (deleted elsewhere).
pub fn set_title(
    conn: &mut Connection,
    id: &str,
    title: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    update(conn, id, "title", &[("title", title)], now, device_id)
}
pub fn set_notes(
    conn: &mut Connection,
    id: &str,
    notes: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    update(conn, id, "notes", &[("notes", notes)], now, device_id)
}

/// Move a task to a project, or to no project (`None`).
pub fn set_list(
    conn: &mut Connection,
    id: &str,
    list_id: Option<&str>,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE tasks SET list_id = ?2, updated_at = ?3, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, list_id, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("list_id", list_id.unwrap_or(""))]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, "project")
        },
    )?;
    // A step is filed where its task is: moving the task moves its open steps
    // in the same write, so a project filter and an export never disagree.
    let step_ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at IS NULL
                 UNION ALL
                 SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
                 WHERE t.deleted_at IS NULL
             )
             SELECT sub.id FROM sub JOIN tasks t ON t.id = sub.id
             WHERE t.list_id IS NOT ?2
             ORDER BY t.created_at, t.rowid",
        )?;
        let rows = stmt.query_map(params![id, list_id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for step_id in &step_ids {
        tx.execute(
            "UPDATE tasks SET list_id = ?2, updated_at = ?3, rev = rev + 1
             WHERE id = ?1 AND deleted_at IS NULL",
            params![step_id, list_id, now],
        )?;
        oplog::append(
            &tx,
            oplog::Entry {
                payload: &json(&[("list_id", list_id.unwrap_or(""))]),
                at: now,
                device_id,
                ..oplog::Entry::new("task", step_id, "project")
            },
        )?;
    }
    let task = task(&tx, id)?;
    tx.commit()?;
    Ok(task)
}

/// Set or clear an all-day due date. `due_at` stays NULL: a day is not an
/// instant, and storing one loses the date when the timezone moves.
pub fn set_due(
    conn: &mut Connection,
    id: &str,
    due_day: Option<&str>,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE tasks SET due_day = ?2, due_at = NULL, due_all_day = ?3,
                          updated_at = ?4, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, due_day, i32::from(due_day.is_some()), now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("due_day", due_day.unwrap_or(""))]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, "due")
        },
    )?;
    let task = task(&tx, id)?;
    tx.commit()?;
    Ok(task)
}

/// Complete or reopen. Completing stamps `completed_at`; reopening clears it,
/// so "how long did this take" is never mixed with "when was it last touched".
/// Completing a task closes every open step below it in the same transaction,
/// one oplog row each; a step that was already done is left alone.
pub fn set_done(
    conn: &mut Connection,
    id: &str,
    done: bool,
    now: i64,
    device_id: &str,
) -> Result<Option<Completion>, StoreError> {
    let tx = conn.transaction()?;
    if tx
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Ok(None);
    }
    // The ids are read before the update, so only steps that were open at this
    // moment are closed by this completion.
    let step_ids: Vec<String> = if done {
        let mut stmt = tx.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at IS NULL
                 UNION ALL
                 SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
                 WHERE t.deleted_at IS NULL
             )
             SELECT sub.id FROM sub JOIN tasks t ON t.id = sub.id
             WHERE t.status = 'open'
             ORDER BY t.created_at, t.rowid",
        )?;
        let rows = stmt.query_map([id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    } else {
        Vec::new()
    };

    tx.execute(
        "UPDATE tasks SET status = ?2, completed_at = ?3,
                          updated_at = ?4, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        params![
            id,
            if done { "done" } else { "open" },
            done.then_some(now),
            now
        ],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("done", if done { "true" } else { "false" })]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, if done { "complete" } else { "reopen" })
        },
    )?;
    let mut closed_steps = Vec::with_capacity(step_ids.len());
    for step_id in &step_ids {
        tx.execute(
            "UPDATE tasks SET status = 'done', completed_at = ?2,
                              updated_at = ?2, rev = rev + 1
             WHERE id = ?1 AND deleted_at IS NULL",
            params![step_id, now],
        )?;
        oplog::append(
            &tx,
            oplog::Entry {
                payload: &json(&[("done", "true")]),
                at: now,
                device_id,
                ..oplog::Entry::new("task", step_id, "complete")
            },
        )?;
        closed_steps.push(task(&tx, step_id)?.expect("the step just completed"));
    }
    let task = task(&tx, id)?.expect("the task just updated");
    // A no-op write still answers with the row: the caller may have been out of
    // date, and the row it gets back is the truth.
    tx.commit()?;
    Ok(Some(Completion { task, closed_steps }))
}

/// Soft delete. The row stays, with its oplog trail, so undo is one update and
/// an export still knows the task existed. Deleting a task deletes its steps in
/// the same transaction; the ids come back so the checklist can drop each row.
pub fn delete(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<Vec<String>>, StoreError> {
    let tx = conn.transaction()?;
    if tx
        .query_row(
            "SELECT 1 FROM tasks WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Ok(None);
    }
    let mut ids = vec![id.to_owned()];
    {
        let mut stmt = tx.prepare(
            "WITH RECURSIVE sub(id) AS (
                 SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at IS NULL
                 UNION ALL
                 SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
                 WHERE t.deleted_at IS NULL
             )
             SELECT sub.id FROM sub JOIN tasks t ON t.id = sub.id
             ORDER BY t.created_at, t.rowid",
        )?;
        let rows = stmt.query_map([id], |row| row.get(0))?;
        ids.extend(rows.collect::<Result<Vec<String>, _>>()?);
    }
    for gone in &ids {
        tx.execute(
            "UPDATE tasks SET deleted_at = ?2, updated_at = ?2, rev = rev + 1
             WHERE id = ?1 AND deleted_at IS NULL",
            params![gone, now],
        )?;
        oplog::append(
            &tx,
            oplog::Entry {
                at: now,
                device_id,
                ..oplog::Entry::new("task", gone, "delete")
            },
        )?;
    }
    tx.commit()?;
    Ok(Some(ids))
}

/// Undo a delete: the same row, no copy, so its history stays in one place.
/// The steps deleted by that same delete come back too: a step deleted on its
/// own earlier keeps its own `deleted_at` and stays gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub task: Task,
    pub steps: Vec<Task>,
}

pub fn restore(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<Restored>, StoreError> {
    let tx = conn.transaction()?;
    let Some(deleted_at) = tx
        .query_row("SELECT deleted_at FROM tasks WHERE id = ?1", [id], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()?
    else {
        return Ok(None);
    };
    tx.execute(
        "UPDATE tasks SET deleted_at = NULL, updated_at = ?2, rev = rev + 1
         WHERE id = ?1",
        params![id, now],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, "restore")
        },
    )?;
    let step_ids: Vec<String> = match deleted_at {
        Some(deleted_at) => {
            let mut stmt = tx.prepare(
                "WITH RECURSIVE sub(id) AS (
                     SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at = ?2
                     UNION ALL
                     SELECT t.id FROM tasks t JOIN sub ON t.parent_id = sub.id
                     WHERE t.deleted_at = ?2
                 )
                 SELECT sub.id FROM sub JOIN tasks t ON t.id = sub.id
                 ORDER BY t.created_at, t.rowid",
            )?;
            let rows = stmt.query_map(params![id, deleted_at], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        }
        None => Vec::new(),
    };
    let mut steps = Vec::with_capacity(step_ids.len());
    for step_id in step_ids {
        tx.execute(
            "UPDATE tasks SET deleted_at = NULL, updated_at = ?2, rev = rev + 1
             WHERE id = ?1",
            params![step_id, now],
        )?;
        oplog::append(
            &tx,
            oplog::Entry {
                at: now,
                device_id,
                ..oplog::Entry::new("task", &step_id, "restore")
            },
        )?;
        if let Some(step) = task(&tx, &step_id)? {
            steps.push(step);
        }
    }
    let task = task(&tx, id)?.expect("the task just restored");
    tx.commit()?;
    Ok(Some(Restored { task, steps }))
}

/// Create a project, in the same transaction as its oplog row.
pub fn create_list(
    conn: &mut Connection,
    name: &str,
    now: i64,
    device_id: &str,
) -> Result<List, StoreError> {
    let tx = conn.transaction()?;
    let (list, _) = get_or_create_list(&tx, name, now, device_id)?;
    tx.commit()?;
    Ok(list)
}

/// Rename a project. The name is trimmed here; an empty one and a name another
/// project already holds are refused, so the chips can never show two
/// `Launch`s. `None` means the row is gone (deleted elsewhere).
pub fn set_list_name(
    conn: &mut Connection,
    id: &str,
    name: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<List>, StoreError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StoreError::EmptyListName);
    }
    let tx = conn.transaction()?;
    if tx
        .query_row(
            "SELECT 1 FROM lists
             WHERE deleted_at IS NULL AND name = ?1 COLLATE NOCASE AND id <> ?2",
            params![name, id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(StoreError::DuplicateList(name.to_owned()));
    }
    let changed = tx.execute(
        "UPDATE lists SET name = ?2, updated_at = ?3, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, name, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("name", name)]),
            at: now,
            device_id,
            ..oplog::Entry::new("list", id, "rename")
        },
    )?;
    let list = tx.query_row(
        "SELECT id, name, color, position FROM lists WHERE id = ?1",
        [id],
        row_to_list,
    )?;
    tx.commit()?;
    Ok(Some(list))
}

/// Delete a project and release its tasks to **No project** in the same
/// transaction. Every released task gets its own oplog row, so an export can
/// rebuild the unfiling without guessing. The tasks themselves are untouched:
/// a project is a grouping, never the owner of a task.
pub fn delete_list(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<ListGone>, StoreError> {
    let tx = conn.transaction()?;
    let Some(list) = tx
        .query_row(
            "SELECT id, name, color, position FROM lists
             WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            row_to_list,
        )
        .optional()?
    else {
        return Ok(None);
    };

    let task_ids: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT id FROM tasks WHERE list_id = ?1 AND deleted_at IS NULL ORDER BY created_at",
        )?;
        let rows = stmt.query_map([id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for task_id in &task_ids {
        tx.execute(
            "UPDATE tasks SET list_id = NULL, updated_at = ?2, rev = rev + 1
             WHERE id = ?1 AND list_id = ?3 AND deleted_at IS NULL",
            params![task_id, now, id],
        )?;
        oplog::append(
            &tx,
            oplog::Entry {
                payload: &json(&[("list_id", "")]),
                at: now,
                device_id,
                ..oplog::Entry::new("task", task_id, "project")
            },
        )?;
    }
    tx.execute(
        "UPDATE lists SET deleted_at = ?2, updated_at = ?2, rev = rev + 1 WHERE id = ?1",
        params![id, now],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            at: now,
            device_id,
            ..oplog::Entry::new("list", id, "delete")
        },
    )?;
    tx.commit()?;
    Ok(Some(ListGone { list, task_ids }))
}

/// Undo a project delete. `task_ids` is the list the delete released, so the
/// tasks come back filed exactly as they were; a task moved somewhere else in
/// the meantime is left alone. A name taken since the delete is refused rather
/// than making two projects with one name.
pub fn restore_list(
    conn: &mut Connection,
    id: &str,
    task_ids: &[String],
    now: i64,
    device_id: &str,
) -> Result<Option<List>, StoreError> {
    let tx = conn.transaction()?;
    let Some(list) = tx
        .query_row(
            "SELECT id, name, color, position FROM lists
             WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
            row_to_list,
        )
        .optional()?
    else {
        return Ok(None);
    };
    if tx
        .query_row(
            "SELECT 1 FROM lists
             WHERE deleted_at IS NULL AND name = ?1 COLLATE NOCASE AND id <> ?2",
            params![list.name, id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(StoreError::DuplicateList(list.name));
    }
    tx.execute(
        "UPDATE lists SET deleted_at = NULL, updated_at = ?2, rev = rev + 1 WHERE id = ?1",
        params![id, now],
    )?;
    oplog::append(
        &tx,
        oplog::Entry {
            at: now,
            device_id,
            ..oplog::Entry::new("list", id, "restore")
        },
    )?;
    for task_id in task_ids {
        let changed = tx.execute(
            "UPDATE tasks SET list_id = ?2, updated_at = ?3, rev = rev + 1
             WHERE id = ?1 AND list_id IS NULL AND deleted_at IS NULL",
            params![task_id, id, now],
        )?;
        if changed > 0 {
            oplog::append(
                &tx,
                oplog::Entry {
                    payload: &json(&[("list_id", id)]),
                    at: now,
                    device_id,
                    ..oplog::Entry::new("task", task_id, "project")
                },
            )?;
        }
    }
    tx.commit()?;
    Ok(Some(list))
}

/// Find a project by name, or make one at the end of the chips. Names are
/// matched case-insensitively so `#Launch` and `#launch` are one project.
fn get_or_create_list(
    conn: &Connection,
    name: &str,
    now: i64,
    device_id: &str,
) -> Result<(List, bool), StoreError> {
    if let Some(list) = conn
        .query_row(
            "SELECT id, name, color, position FROM lists
             WHERE deleted_at IS NULL AND name = ?1 COLLATE NOCASE",
            [name],
            row_to_list,
        )
        .optional()?
    {
        return Ok((list, false));
    }

    let id = ItemId::new().to_string();
    let position: i64 = conn.query_row(
        "SELECT coalesce(max(position) + 1, 0) FROM lists WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO lists(id, name, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, name, position, now],
    )?;
    oplog::append(
        conn,
        oplog::Entry {
            payload: &json(&[("name", name)]),
            at: now,
            device_id,
            ..oplog::Entry::new("list", &id, "create")
        },
    )?;
    conn.query_row(
        "SELECT id, name, color, position FROM lists WHERE id = ?1",
        [&id],
        row_to_list,
    )
    .map(|list| (list, true))
    .map_err(StoreError::from)
}

/// A one-column update with its oplog row, for the fields that are just text.
fn update(
    conn: &mut Connection,
    id: &str,
    op: &'static str,
    fields: &[(&str, &str)],
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    let tx = conn.transaction()?;
    // The column names are compile-time literals from this module, never input.
    let assignments: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(index, (column, _))| format!("{column} = ?{}", index + 2))
        .collect();
    let sql = format!(
        "UPDATE tasks SET {}, updated_at = ?{}, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        assignments.join(", "),
        fields.len() + 2
    );
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(fields.len() + 2);
    let now_box = now;
    values.push(&id);
    for (_, value) in fields {
        values.push(value);
    }
    values.push(&now_box);
    let changed = tx.execute(&sql, values.as_slice())?;
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(fields),
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, op)
        },
    )?;
    let task = task(&tx, id)?;
    tx.commit()?;
    Ok(task)
}

/// How many rows deep a task sits: a task is 1, its steps 2, their steps 3.
/// The walk stops at [`notex_core::MAX_TASK_DEPTH`] + 1 so a corrupted cycle
/// cannot hang the worker.
fn depth_of(conn: &Connection, id: &str) -> Result<usize, StoreError> {
    let mut depth = 0;
    let mut cursor = Some(id.to_owned());
    while let Some(current) = cursor {
        depth += 1;
        if depth > notex_core::MAX_TASK_DEPTH {
            break;
        }
        cursor = conn
            .query_row(
                "SELECT parent_id FROM tasks WHERE id = ?1",
                [&current],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
    }
    Ok(depth)
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        done: row.get::<_, String>(3)? == "done",
        list_id: row.get(4)?,
        list_name: row.get(5)?,
        list_color: row.get(6)?,
        due_day: row.get(7)?,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
        rev: row.get(10)?,
        parent_id: row.get(11)?,
    })
}

fn row_to_list(row: &rusqlite::Row<'_>) -> rusqlite::Result<List> {
    Ok(List {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        position: row.get(3)?,
    })
}

fn json(fields: &[(&str, &str)]) -> String {
    let map: serde_json::Map<String, serde_json::Value> = fields
        .iter()
        .map(|(key, value)| ((*key).to_owned(), serde_json::Value::from(*value)))
        .collect();
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::oplog;

    fn now() -> i64 {
        1_763_500_800_000
    }

    #[test]
    fn a_task_is_created_with_its_project_and_a_log_row() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Send invites",
            Some("Launch"),
            Some("2026-11-21"),
            now(),
            "dev",
        )
        .unwrap();

        assert_eq!(created.task.title, "Send invites");
        assert_eq!(created.task.list_name.as_deref(), Some("Launch"));
        assert_eq!(created.task.due_day.as_deref(), Some("2026-11-21"));
        assert!(!created.task.done);
        assert_eq!(created.list.as_ref().unwrap().name, "Launch");
        assert_eq!(oplog::len(&conn).unwrap(), 2, "one task and one project");
    }

    #[test]
    fn projects_are_reused_case_insensitively() {
        let mut conn = db::open_in_memory().unwrap();
        let first = create(&mut conn, "A", Some("Launch"), None, now(), "dev").unwrap();
        let second = create(&mut conn, "B", Some("launch"), None, now(), "dev").unwrap();

        assert_eq!(first.task.list_id, second.task.list_id);
        assert!(second.list.is_none(), "the second task created no project");
        assert_eq!(lists(&conn).unwrap().len(), 1);
    }

    #[test]
    fn completing_stamps_the_time_and_reopening_clears_it() {
        let mut conn = db::open_in_memory().unwrap();
        let task = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;

        let done = set_done(&mut conn, &task.id, true, now() + 5, "dev")
            .unwrap()
            .unwrap()
            .task;
        assert!(done.done);
        assert_eq!(done.completed_at, Some(now() + 5));
        assert_eq!(done.rev, task.rev + 1);

        let open = set_done(&mut conn, &task.id, false, now() + 9, "dev")
            .unwrap()
            .unwrap()
            .task;
        assert!(!open.done);
        assert_eq!(open.completed_at, None);
    }

    #[test]
    fn deleting_hides_the_task_and_restoring_brings_the_same_row_back() {
        let mut conn = db::open_in_memory().unwrap();
        let row = create(&mut conn, "Later", None, None, now(), "dev")
            .unwrap()
            .task;

        let gone = delete(&mut conn, &row.id, now() + 1, "dev")
            .unwrap()
            .expect("the row was there");
        assert_eq!(gone, [row.id.as_str()]);
        assert!(tasks(&conn).unwrap().is_empty(), "a deleted task is hidden");
        assert_eq!(task(&conn, &row.id).unwrap(), None);

        let restored = restore(&mut conn, &row.id, now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(restored.task.id, row.id);
        assert_eq!(restored.task.title, "Later");
        assert!(
            restored.steps.is_empty(),
            "a task with no steps restores none"
        );
        assert_eq!(tasks(&conn).unwrap().len(), 1);
    }

    #[test]
    fn due_day_and_project_are_editable_and_logged() {
        let mut conn = db::open_in_memory().unwrap();
        let task = create(&mut conn, "Plan", None, None, now(), "dev")
            .unwrap()
            .task;
        let items = create_list(&mut conn, "Studio", now(), "dev").unwrap();

        let due = set_due(&mut conn, &task.id, Some("2026-11-20"), now() + 1, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(due.due_day.as_deref(), Some("2026-11-20"));

        let filed = set_list(&mut conn, &task.id, Some(&items.id), now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(filed.list_name.as_deref(), Some("Studio"));

        let cleared = set_due(&mut conn, &task.id, None, now() + 3, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(cleared.due_day, None);
        // create task, create project, due, project, due
        assert_eq!(oplog::len(&conn).unwrap(), 5);
    }

    #[test]
    fn a_project_is_renamed_in_place_and_the_tasks_follow_the_join() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Send invites",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap();
        let list = created.list.unwrap();

        let renamed = set_list_name(&mut conn, &list.id, "  Launch party  ", now() + 1, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(renamed.name, "Launch party");
        assert_eq!(renamed.id, list.id);
        assert_eq!(
            task(&conn, &created.task.id)
                .unwrap()
                .unwrap()
                .list_name
                .as_deref(),
            Some("Launch party"),
            "the task reads its project through the join"
        );
        assert_eq!(oplog::len(&conn).unwrap(), 3, "create, task, rename");
    }

    #[test]
    fn a_project_cannot_take_a_name_another_is_using() {
        let mut conn = db::open_in_memory().unwrap();
        let launch = create_list(&mut conn, "Launch", now(), "dev").unwrap();
        let studio = create_list(&mut conn, "Studio", now(), "dev").unwrap();

        let refused = set_list_name(&mut conn, &studio.id, "launch", now() + 1, "dev");
        assert!(matches!(
            refused,
            Err(StoreError::DuplicateList(name)) if name == "launch"
        ));
        assert_eq!(
            lists(&conn)
                .unwrap()
                .into_iter()
                .map(|l| l.name)
                .collect::<Vec<_>>(),
            ["Launch", "Studio"]
        );
        // Renaming a project to its own name, whatever the case, is fine.
        let same = set_list_name(&mut conn, &launch.id, "LAUNCH", now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(same.name, "LAUNCH");
    }

    #[test]
    fn a_project_needs_a_name() {
        let mut conn = db::open_in_memory().unwrap();
        let list = create_list(&mut conn, "Launch", now(), "dev").unwrap();
        assert!(matches!(
            set_list_name(&mut conn, &list.id, "   ", now(), "dev"),
            Err(StoreError::EmptyListName)
        ));
        assert_eq!(lists(&conn).unwrap()[0].name, "Launch");
    }

    #[test]
    fn deleting_a_project_releases_its_tasks_and_undo_refiles_them() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Send invites",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap();
        let list = created.list.unwrap();
        let unfiled = create(&mut conn, "Read the notes", None, None, now(), "dev")
            .unwrap()
            .task;

        let gone = delete_list(&mut conn, &list.id, now() + 1, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(gone.list.id, list.id);
        assert_eq!(gone.task_ids, std::slice::from_ref(&created.task.id));
        assert!(lists(&conn).unwrap().is_empty(), "the chip is gone");
        assert_eq!(
            task(&conn, &created.task.id).unwrap().unwrap().list_id,
            None,
            "the task is unfiled, not deleted"
        );
        assert_eq!(
            task(&conn, &unfiled.id).unwrap().unwrap().list_id,
            None,
            "a task that was never filed is untouched"
        );
        // create + task, the unfiled create, the released task, the list delete
        assert_eq!(oplog::len(&conn).unwrap(), 5);

        let restored = restore_list(&mut conn, &list.id, &gone.task_ids, now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(restored.name, "Launch");
        assert_eq!(
            task(&conn, &created.task.id)
                .unwrap()
                .unwrap()
                .list_name
                .as_deref(),
            Some("Launch"),
            "undo puts the task back where it was"
        );
        assert_eq!(lists(&conn).unwrap().len(), 1);
        // list restore and the task's project row
        assert_eq!(oplog::len(&conn).unwrap(), 7);
    }

    #[test]
    fn undo_leaves_a_task_that_moved_elsewhere_alone() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Send invites",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap();
        let launch = created.list.unwrap();
        let studio = create_list(&mut conn, "Studio", now(), "dev").unwrap();

        let gone = delete_list(&mut conn, &launch.id, now() + 1, "dev")
            .unwrap()
            .unwrap();
        set_list(
            &mut conn,
            &created.task.id,
            Some(&studio.id),
            now() + 2,
            "dev",
        )
        .unwrap();
        restore_list(&mut conn, &launch.id, &gone.task_ids, now() + 3, "dev").unwrap();

        assert_eq!(
            task(&conn, &created.task.id)
                .unwrap()
                .unwrap()
                .list_name
                .as_deref(),
            Some("Studio"),
            "a refiled task stays where the person put it"
        );
    }

    #[test]
    fn a_restore_does_not_make_two_projects_with_one_name() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Send invites",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap();
        let launch = created.list.unwrap();
        let gone = delete_list(&mut conn, &launch.id, now() + 1, "dev")
            .unwrap()
            .unwrap();
        let again = create_list(&mut conn, "Launch", now() + 2, "dev").unwrap();

        let refused = restore_list(&mut conn, &launch.id, &gone.task_ids, now() + 3, "dev");
        assert!(matches!(
            refused,
            Err(StoreError::DuplicateList(name)) if name == "Launch"
        ));
        assert_eq!(lists(&conn).unwrap(), [again]);
    }

    #[test]
    fn a_missing_project_is_none_not_an_error() {
        let mut conn = db::open_in_memory().unwrap();
        assert_eq!(
            set_list_name(&mut conn, "nope", "x", now(), "dev").unwrap(),
            None
        );
        assert_eq!(delete_list(&mut conn, "nope", now(), "dev").unwrap(), None);
        assert_eq!(
            restore_list(&mut conn, "nope", &[], now(), "dev").unwrap(),
            None
        );
    }

    #[test]
    fn a_missing_task_is_none_not_an_error() {
        let mut conn = db::open_in_memory().unwrap();
        assert_eq!(
            set_title(&mut conn, "nope", "x", now(), "dev").unwrap(),
            None
        );
        assert_eq!(
            set_done(&mut conn, "nope", true, now(), "dev").unwrap(),
            None
        );
        assert_eq!(delete(&mut conn, "nope", now(), "dev").unwrap(), None);
        assert_eq!(
            set_due(&mut conn, "nope", None, now(), "dev").unwrap(),
            None
        );
    }

    #[test]
    fn titles_are_editable_and_bump_the_revision() {
        let mut conn = db::open_in_memory().unwrap();
        let task = create(&mut conn, "Old", None, None, now(), "dev")
            .unwrap()
            .task;
        let renamed = set_title(&mut conn, &task.id, "New", now() + 1, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(renamed.title, "New");
        assert_eq!(renamed.rev, task.rev + 1);
    }

    #[test]
    fn tasks_are_listed_open_first_then_by_day() {
        let mut conn = db::open_in_memory().unwrap();
        create(&mut conn, "Later", None, Some("2026-12-01"), now(), "dev").unwrap();
        create(&mut conn, "Soon", None, Some("2026-11-20"), now(), "dev").unwrap();
        create(&mut conn, "Someday", None, None, now(), "dev").unwrap();
        let done = create(&mut conn, "Done", None, None, now(), "dev")
            .unwrap()
            .task;
        set_done(&mut conn, &done.id, true, now(), "dev").unwrap();

        let titles: Vec<_> = tasks(&conn)
            .unwrap()
            .into_iter()
            .map(|task| task.title)
            .collect();
        assert_eq!(titles, ["Soon", "Later", "Someday", "Done"]);
    }

    #[test]
    fn a_step_is_a_task_that_keeps_its_parents_project() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(
            &mut conn,
            "Ship the release",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap()
        .task;

        let step = create_subtask(
            &mut conn,
            &parent.id,
            "  Write the notes  ",
            now() + 1,
            "dev",
        )
        .unwrap();

        assert_eq!(step.title, "Write the notes", "the title is trimmed");
        assert_eq!(step.parent_id.as_deref(), Some(parent.id.as_str()));
        assert_eq!(
            step.list_id, parent.list_id,
            "a step is filed with its parent"
        );
        assert_eq!(step.due_day, None, "a step has no day of its own");
        assert!(!step.done);
        assert_eq!(oplog::len(&conn).unwrap(), 3, "create task, project, step");
        assert_eq!(tasks(&conn).unwrap().len(), 2, "both rows are in the list");
    }

    #[test]
    fn a_task_tree_stops_at_three_levels_and_a_done_task_takes_no_step() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;
        let step = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();
        let leaf = create_subtask(&mut conn, &step.id, "Open the draft", now(), "dev").unwrap();

        assert!(matches!(
            create_subtask(&mut conn, &leaf.id, "Too deep", now(), "dev"),
            Err(StoreError::NestedStep)
        ));
        assert!(matches!(
            create_subtask(&mut conn, "nope", "Orphan", now(), "dev"),
            Err(StoreError::MissingTask)
        ));
        assert!(matches!(
            create_subtask(&mut conn, &parent.id, "   ", now(), "dev"),
            Err(StoreError::EmptyStepTitle)
        ));

        set_done(&mut conn, &parent.id, true, now(), "dev").unwrap();
        assert!(matches!(
            create_subtask(&mut conn, &parent.id, "Late", now(), "dev"),
            Err(StoreError::TaskIsDone)
        ));
    }

    #[test]
    fn completing_a_task_closes_its_open_steps_in_one_transaction() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;
        let first = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();
        let second = create_subtask(&mut conn, &parent.id, "Test", now() + 1, "dev").unwrap();
        let already = create_subtask(&mut conn, &parent.id, "Plan", now() + 2, "dev").unwrap();
        set_done(&mut conn, &already.id, true, now() + 3, "dev").unwrap();
        // A step of a step is closed by the same check.
        let leaf =
            create_subtask(&mut conn, &first.id, "Open the draft", now() + 4, "dev").unwrap();
        let before = oplog::len(&conn).unwrap();

        let done = set_done(&mut conn, &parent.id, true, now() + 10, "dev")
            .unwrap()
            .unwrap();

        assert!(done.task.done);
        let closed: Vec<_> = done.closed_steps.iter().map(|s| s.id.clone()).collect();
        assert_eq!(
            closed,
            [first.id.clone(), second.id.clone(), leaf.id.clone()]
        );
        for step in &done.closed_steps {
            assert!(step.done);
            assert_eq!(step.completed_at, Some(now() + 10));
        }
        // The parent and its three open steps: four rows, four oplog rows.
        assert_eq!(oplog::len(&conn).unwrap(), before + 4);
        assert!(task(&conn, &already.id).unwrap().unwrap().done);
    }

    #[test]
    fn reopening_a_task_leaves_its_steps_closed() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;
        let step = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();
        set_done(&mut conn, &parent.id, true, now(), "dev").unwrap();

        let open = set_done(&mut conn, &parent.id, false, now() + 1, "dev")
            .unwrap()
            .unwrap();

        assert!(!open.task.done);
        assert!(open.closed_steps.is_empty(), "reopening closes nothing");
        assert!(
            task(&conn, &step.id).unwrap().unwrap().done,
            "a step that was finished stays finished"
        );
    }

    #[test]
    fn deleting_a_task_takes_its_steps_and_restore_brings_them_back() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;
        let step = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();
        let leaf = create_subtask(&mut conn, &step.id, "Open the draft", now(), "dev").unwrap();

        let gone = delete(&mut conn, &parent.id, now() + 1, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(gone, [parent.id.clone(), step.id.clone(), leaf.id.clone()]);
        assert!(tasks(&conn).unwrap().is_empty());

        let restored = restore(&mut conn, &parent.id, now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(
            restored
                .steps
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            [step.id.as_str(), leaf.id.as_str()],
            "undo brings the whole branch back with the task"
        );
        assert_eq!(
            task(&conn, &step.id).unwrap().map(|s| s.title),
            Some("Write".to_owned())
        );
    }

    #[test]
    fn moving_a_task_moves_its_steps_in_the_same_write() {
        let mut conn = db::open_in_memory().unwrap();
        let created = create(
            &mut conn,
            "Ship the release",
            Some("Launch"),
            None,
            now(),
            "dev",
        )
        .unwrap();
        let parent = created.task;
        let step = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();
        let leaf = create_subtask(&mut conn, &step.id, "Open the draft", now(), "dev").unwrap();
        let studio = create_list(&mut conn, "Studio", now(), "dev").unwrap();

        set_list(&mut conn, &parent.id, Some(&studio.id), now() + 1, "dev")
            .unwrap()
            .unwrap();

        assert_eq!(
            task(&conn, &step.id).unwrap().unwrap().list_name.as_deref(),
            Some("Studio"),
            "a step is filed where its task is"
        );
        assert_eq!(
            task(&conn, &leaf.id).unwrap().unwrap().list_name.as_deref(),
            Some("Studio"),
            "so is a step of a step"
        );
        // create task, its project, two steps, the Studio project, and the three
        // project moves (task and both steps)
        assert_eq!(oplog::len(&conn).unwrap(), 8);

        // Moving to no project takes the steps with it.
        set_list(&mut conn, &parent.id, None, now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(task(&conn, &step.id).unwrap().unwrap().list_id, None);
    }

    #[test]
    fn a_step_is_deleted_on_its_own_without_touching_its_parent() {
        let mut conn = db::open_in_memory().unwrap();
        let parent = create(&mut conn, "Ship", None, None, now(), "dev")
            .unwrap()
            .task;
        let step = create_subtask(&mut conn, &parent.id, "Write", now(), "dev").unwrap();

        let gone = delete(&mut conn, &step.id, now() + 1, "dev")
            .unwrap()
            .unwrap();

        assert_eq!(gone, [step.id.as_str()]);
        assert!(task(&conn, &parent.id).unwrap().is_some());
        assert_eq!(tasks(&conn).unwrap().len(), 1);
    }
}
