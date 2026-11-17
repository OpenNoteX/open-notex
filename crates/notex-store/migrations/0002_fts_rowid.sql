-- 0002_fts_rowid: remember each note's FTS rowid.
--
-- `notes_fts.item_id` is UNINDEXED, so `DELETE ... WHERE item_id = ?` scans the
-- whole FTS table; a 10k-note reindex was quadratic because of it. FTS5's rowid
-- is its real key, so store it beside the note and delete by that.
--
-- A database from v1 has FTS rows but no mapping. Fill it once here, through a
-- temp table with an index, rather than scanning the FTS table per note.

ALTER TABLE note_index ADD COLUMN fts_rowid INTEGER;

CREATE TEMP TABLE notes_fts_map AS
    SELECT rowid AS fts_rowid, item_id FROM notes_fts;
CREATE INDEX notes_fts_map_item ON notes_fts_map(item_id);

UPDATE note_index
SET fts_rowid = (SELECT fts_rowid FROM notes_fts_map WHERE item_id = note_index.id)
WHERE id IN (SELECT item_id FROM notes_fts_map);

DROP TABLE notes_fts_map;
