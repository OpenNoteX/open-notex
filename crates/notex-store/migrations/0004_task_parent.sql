-- Steps are tasks with a parent; the checklist reads them per parent row.
CREATE INDEX tasks_parent ON tasks(parent_id) WHERE deleted_at IS NULL;
