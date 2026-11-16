# Rules for agents

Read `docs/ARCHITECTURE.md` before touching a view. These are not style
preferences; each one protects a property of the app.

1. **Never do I/O inside `view()`.** No SQLite, no file reads, no clock reads in
   loops. `view()` is a pure function of state. It runs on every event, so I/O
   there is a dropped frame.
2. **One database connection, on one thread.** All DB and vault I/O goes through
   `StoreHandle` to the DB worker. Never open a second `rusqlite::Connection`.
   The UI state must stay `Send + Sync`, and SQLite allows one writer.
3. **Markdown files are the source of truth for note bodies.** SQLite is a
   rebuildable index. Never store a note body only in SQLite: `--reindex` must
   be able to restore everything from the vault.
4. **Write order is file first, index second.** The index may be stale; it is
   never authoritative. A crash between the two costs a reindex, not a note.
5. **Every mutation writes its oplog row in the same transaction.** No
   exceptions. Undo, export and any future sync depend on it.
6. **No new dependency without an entry in `docs/DECISIONS.md`.** No tokio, no
   async runtime, no HTTP server, no WebView, no JavaScript, no Electron.
7. **Rune is pinned by git rev in the root `Cargo.toml`.** Upgrade it in its own
   commit, with snapshots regenerated in that same commit.
8. **Never put the database inside the vault directory.** Assert it at startup.
9. **Performance budgets in `docs/PERFORMANCE.md` are merge gates.** If a change
   exceeds one, fix it or change the budget in an explicit commit.
10. **A user-visible feature lands with** a headless tree test, a snapshot scene,
    and a `CHANGELOG.md` entry.

## Commands

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask seed --count 500 --dir /tmp/vault
```

## Conventions

- Timestamps are UTC epoch milliseconds (`i64`). Local days are `YYYY-MM-DD`
  strings derived in `notex-time`, never stored as timestamps.
- Ids are ULIDs. Every mutable row has `created_at`, `updated_at`, `deleted_at`
  (soft delete) and `rev`.
- Errors are typed (`thiserror`); handlers never panic. A worker panic becomes an
  `Evt::Failed`.
- Pure logic goes in `notex-time` or `notex-core` so it can be tested without a
  clock, a database, or a window.
