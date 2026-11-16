# Performance and footprint

The app should feel instant and cost almost nothing when idle. These are merge
gates from M0, not post-launch cleanup.

## Budgets

| Metric | Target | Gate (fail) |
| --- | --- | --- |
| Cold start → first frame, 10k notes | ≤ 400 ms | 600 ms |
| Commit (event → draw) p95 | ≤ 8 ms | 16 ms |
| Idle CPU | 0 frames, 0 wakeups | any sustained wake |
| Settled footprint, idle | ≤ 100 MB | 150 MB |
| Footprint, 50k notes, Today open | ≤ 200 MB | 300 MB |
| `SaveNote` p95 | ≤ 5 ms | 20 ms |
| `--reindex`, 10k notes | ≤ 3 s | 10 s |
| DB size, 10k notes | ≤ 25 MB | warn only |

Reference point: Rune's own settings window settles at ~70 MB footprint and
~0.85% CPU idle, drawing nothing while still. We aim for the same class.

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
4. Read models are bounded per route; the UI never holds all notes.
5. Titles and hashes only in the index; FTS returns snippets, not documents.
6. Daily counts come from `habit_days` and small rollups, not `COUNT(*)` scans.
7. Prepared-statement cache and covering indexes for the four hot queries.
8. `PRAGMA optimize` on shutdown, `ANALYZE` after migrations.
9. `on_ready` defers indexing, backups and the watcher until after the first
   frame.
10. `.cache()` for static chrome; it bypasses the cache only while something
    inside actually animates.
11. Skeletons instead of infinite spinners: a loop is a permanent wakeup.

## Measuring

- Dev builds: `rune-perf` + `procwatch` report footprint, lifetime peak, fd
  count and frame percentiles, and flag `long-frames`, `memory-growth` and
  `fd-growth`.
- `cargo xtask bench --items 50000`: prints the table above.
- Manual: `footprint`, `vmmap`, Instruments (Allocations, Metal System Trace) at
  every milestone.
- Soak: 200 edits/hour for 24 h; footprint floor must not rise by 64 MiB.
