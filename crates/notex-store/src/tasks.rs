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

const SELECT: &str = "\
SELECT t.id, t.title, t.notes, t.status, t.list_id, l.name, l.color,
       t.due_day, t.completed_at, t.created_at, t.rev
FROM tasks t
LEFT JOIN lists l ON l.id = t.list_id AND l.deleted_at IS NULL";

/// Every task that is not deleted, open first, then by day and age.
pub fn tasks(conn: &Connection) -> Result<Vec<Task>, StoreError> {
    let sql = format!(
        "{SELECT} WHERE t.deleted_at IS NULL ORDER BY (t.status = 'done'), t.due_day IS NULL, t.due_day, t.created_at"
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
pub fn set_done(
    conn: &mut Connection,
    id: &str,
    done: bool,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
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
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            payload: &json(&[("done", if done { "true" } else { "false" })]),
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, if done { "complete" } else { "reopen" })
        },
    )?;
    let task = task(&tx, id)?;
    tx.commit()?;
    Ok(task)
}

/// Soft delete. The row stays, with its oplog trail, so undo is one update and
/// an export still knows the task existed.
pub fn delete(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<bool, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE tasks SET deleted_at = ?2, updated_at = ?2, rev = rev + 1
         WHERE id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    if changed == 0 {
        return Ok(false);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, "delete")
        },
    )?;
    tx.commit()?;
    Ok(true)
}

/// Undo a delete: the same row, no copy, so its history stays in one place.
pub fn restore(
    conn: &mut Connection,
    id: &str,
    now: i64,
    device_id: &str,
) -> Result<Option<Task>, StoreError> {
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE tasks SET deleted_at = NULL, updated_at = ?2, rev = rev + 1
         WHERE id = ?1",
        params![id, now],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    oplog::append(
        &tx,
        oplog::Entry {
            at: now,
            device_id,
            ..oplog::Entry::new("task", id, "restore")
        },
    )?;
    let task = task(&tx, id)?;
    tx.commit()?;
    Ok(task)
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
            .unwrap();
        assert!(done.done);
        assert_eq!(done.completed_at, Some(now() + 5));
        assert_eq!(done.rev, task.rev + 1);

        let open = set_done(&mut conn, &task.id, false, now() + 9, "dev")
            .unwrap()
            .unwrap();
        assert!(!open.done);
        assert_eq!(open.completed_at, None);
    }

    #[test]
    fn deleting_hides_the_task_and_restoring_brings_the_same_row_back() {
        let mut conn = db::open_in_memory().unwrap();
        let row = create(&mut conn, "Later", None, None, now(), "dev")
            .unwrap()
            .task;

        assert!(delete(&mut conn, &row.id, now() + 1, "dev").unwrap());
        assert!(tasks(&conn).unwrap().is_empty(), "a deleted task is hidden");
        assert_eq!(task(&conn, &row.id).unwrap(), None);

        let restored = restore(&mut conn, &row.id, now() + 2, "dev")
            .unwrap()
            .unwrap();
        assert_eq!(restored.id, row.id);
        assert_eq!(restored.title, "Later");
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
        assert!(!delete(&mut conn, "nope", now(), "dev").unwrap());
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
}
