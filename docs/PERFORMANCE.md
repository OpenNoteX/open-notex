# Performance and footprint

The app should feel instant and cost almost nothing when idle. These are merge
gates from M0, not post-launch cleanup.

## Budgets

| Metric | Target | Gate (fail) |
| --- | --- | --- |
| Cold start → first frame, 10k notes | ≤ 400 ms | 600 ms |
| Commit (event → draw) p95 | ≤ 8 ms | 16 ms |
| Idle CPU | 0 frames, no render wakeups | any drawn frame while nothing changed |
| Settled footprint, idle | ≤ 100 MB | 150 MB |
| Footprint, 50k notes, Today open | ≤ 200 MB | 300 MB |
| `SaveNote` p95 | ≤ 5 ms | 20 ms |
| `--reindex`, 10k notes | ≤ 3 s | 10 s |
| Watcher pass, 10k notes (no note read) | ≤ 100 ms | 250 ms |
| DB size, 10k notes | ≤ 25 MB | warn only |

Reference point: Rune's own settings window settles at ~70 MB footprint and
~0.85% CPU idle, drawing nothing while still. We aim for the same class.

"0 frames" is about the render loop: `poll` returns `false`, so Rune draws
nothing. The store's worker does wake every two seconds while a vault is open, to
look for edits made outside the app; that is a metadata walk on a background
thread, it publishes nothing when the vault has not moved, and it never asks for a
frame. Its cost is the "watcher pass" row.

## Measured

M0.5 shell, release build, macOS 27.0.1 on an Apple M5, warm file cache.

| Metric | Measured | Budget |
| --- | --- | --- |
| Process start → first pixels (headless) | 31 ms | — |
| GPU init | 9.4 ms | — |
| First frame, CPU | 2.9 ms | ≤ 8 ms |
| State setup (`init`) | 0.05 ms | — |
| Layout nodes / glyphs / instances, one screen | 88 / 178 / 197 | — |
| First launch with a cold page cache | 0.6 s | ≤ 400 ms |
| Settled footprint, idle | 53.6 MB physical, 97 MB RSS | ≤ 100 MB |
| Peak footprint during startup | 134 MB (GPU upload) | — |
| Idle CPU, 29 s after the first frame | 0.0%, no wakeups | 0 frames |
| Threads at rest | 7 | — |
| Release binary | 28 MB (with Rune's `tooling` feature) | 20–30 MB |

"Process start → first pixels" is Rune's `--first-pixels`, which includes arg
parsing, state, GPU init and one laid-out, painted frame. The 0.6 s cold-cache
figure is the same run with the 28 MB binary read from disk for the first time.

M1a store, release build, same machine, 10k-note fixture from `xtask seed`:

| Metric | Measured | Budget |
| --- | --- | --- |
| `--reindex`, 10k notes, empty database | 3.3 s | ≤ 3 s target, 10 s gate |
| `--reindex`, 10k notes, unchanged vault | 0.19 s | — |
| `notes()` list for 10k notes | 4.9–5.2 ms | — |
| `COUNT(*)` for 10k notes | 0.3–0.5 ms | — |
| FTS query, 50 hits | 3.3–5.1 ms | — |
| DB size, 10k notes | 11.4 MiB | ≤ 25 MB |
| Vault scan (10k files, cache warm) | 0.24 s | — |
| Watcher quiet pass, 10k notes | 36 ms median, 36 ms p95 | ≤ 100 ms target, 250 ms gate |

The reindex figures include the prune added with delete: a scan of `note_index`
plus a set of the paths found (10k rows, ~5 ms), so a note deleted outside the
app leaves the list at the next launch. The 3 s target is still missed by the
vault scan and the per-note write; the 10 s gate holds.

The watcher pass is one `read_dir` walk plus a `stat` per file: it reads no
note, which is what keeps it at 36 ms for 10k notes. The worker runs one every
two seconds while a vault is open, so it costs ~2% of that thread and a save
waits at most one pass behind it — well inside the 700 ms autosave idle window.
A vault that has not moved is not opened at all.

`notes()` is the whole index, decoded into `NoteRow`s: ~5 ms and a few MB at
10k, inside the 8 MB read model budget but not bounded by it. It has to become a
windowed query before the 50k row is meaningful.

M1b editing, release build, same machine, same 10k fixture:

| Metric | Measured | Budget |
| --- | --- | --- |
| `SaveNote` median | 5.1 ms | ≤ 5 ms target |
| `SaveNote` p95 | 6.3 ms | ≤ 5 ms target, 20 ms gate |
| — of which file write (atomic + fsync) | 4.2 ms | — |
| — of which index upsert (one WAL commit) | 1.0 ms | — |

A save is one atomic file write — create, fsync, rename — plus one index
commit; reading the old body and hashing it costs ~50 µs. The fsync is the whole
budget: it dominates the 5.1 ms median and its variance sets the p95. Dropping
it would make a save fast and a power cut lossy, so the target is missed and the
20 ms gate holds with room. Typing never waits for it either way: the write
happens on the worker thread, after the frame that asked for it.

First frame does not wait for the index: a warm launch sends `ListNotes` and
draws the list frame while the worker answers; a fresh launch draws the same
frame with the list's "opening" state. The 10k cold-start row is therefore the
shell's first-frame number plus the list round trip, not the reindex.

The 3 s reindex target is still missed on a **cold full rebuild** of 10k notes;
the 10 s gate holds. The first measurement of the same work was 8.4 s, because
every note deleted its search row with `WHERE item_id = ?` and `item_id` is
`UNINDEXED`, so each upsert scanned the whole FTS table. Migration
`0002_fts_rowid` stores the FTS rowid beside the note and the reindex became
linear. Batching the vault into one transaction and skipping unchanged files
took the rest; a warm launch now spends ~0.2 s reading file metadata and writes
nothing to the database.

## Where memory goes

| Consumer | Budget | Bound by |
| --- | --- | --- |
| GPU shell, window, 2–3 drawables | 25–35 MB | `release_gpu_when_closed(true)` |
| Rune texture cache | 16 MB | `Renderer::set_cache_budget(16 << 20)` |
| Glyph atlas | ~4 MB | 2048² R8, two faces only |
| UI read model | ≤ 8 MB | per-route `LIMIT`, counts precomputed |
| SQLite page cache | 8 MB | `PRAGMA cache_size = -8000` |
| Note bodies in RAM | 1 open note | load on open, drop on navigate |
| Binary + deps | 20–30 MB | no tokio, no webview, no image decoders |

## Techniques

1. `poll()` returns `false` when nothing changed; Rune then skips the frame.
2. No per-second clocks. A visible countdown re-arms `wake_after` at its next
   change only while on screen.
3. Virtualize every long list (`.growth()` + `.virtualize(h)`), so a 10k-row list
   costs the same as 20 rows.
4. Read models are bounded per route. The notes list is the one unbounded read
   today (~5 ms and a few MB at 10k); it becomes a window before 50k.
5. Titles and hashes only in the index; FTS returns snippets, not documents.
6. Daily counts come from `habit_days` and small rollups, not `COUNT(*)` scans.
7. Prepared-statement cache and covering indexes for the four hot queries.
8. `PRAGMA optimize` on shutdown, `ANALYZE` after migrations.
9. `on_ready` defers indexing, backups and the watcher until after the first
   frame. The watcher is a metadata comparison, so a quiet vault costs a walk
   and no reads.
10. `.cache()` for static chrome; it bypasses the cache only while something
    inside actually animates.
11. Skeletons instead of infinite spinners: a loop is a permanent wakeup.

## Measuring

- Dev builds: `rune-perf` + `procwatch` report footprint, lifetime peak, fd
  count and frame percentiles, and flag `long-frames`, `memory-growth` and
  `fd-growth`.
- `cargo xtask seed --count 50000 --dir artifacts/vault --db artifacts/bench.db`,
  then `cargo xtask bench --db artifacts/bench.db`: prints the table above.
- Manual: `footprint`, `vmmap`, Instruments (Allocations, Metal System Trace) at
  every milestone.
- Soak: 200 edits/hour for 24 h; footprint floor must not rise by 64 MiB.
