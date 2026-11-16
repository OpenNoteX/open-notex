# Changelog

All notable changes to this project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The shell: a window with a sidebar, a list and a detail pane, ten routes
  (Inbox, Today, Upcoming, Projects, Notes, Tags, Habits, Reminders, Search,
  Settings), `⌘K` for search, `⌘,` for settings and the sidebar toggle.
- `crates/notex-ui`: `Route` as the single source of labels, icons and empty
  states; a three-seed palette (warm paper, near-black, teal) for light and
  dark; loading, failed and empty states; `SCENES` for snapshots.
- `apps/open-notex`: the `AppSpec`, traffic-light and drag-region chrome, and
  `--vault` / `--route`.
- `cargo xtask snap` renders every scene in both themes through the real binary,
  writes a PNG and an element tree, and asserts the tree is not blank.
- Rune and rune-kit pinned by revision (`631e8a6`), with the three dependencies
  Rune patches copied into `vendor/`.
- A `.cargo/config.toml` alias so `cargo xtask` works as documented.
- CI renders the snapshots on macOS and uploads them with the other artifacts.
- Project context: `AGENTS.md`, `README.md`, and `docs/` covering plan,
  architecture, data model, scheduling, decisions, testing and budgets.
- Cargo workspace on Rust 1.99.1 with `notex-core`, `notex-time`, `notex-store`
  and `xtask`.
- `notex-time`: day keys with a configurable day start, recurrence expansion
  (daily/weekly/monthly), streak calculation, and the scheduler's `next_action`.
- `notex-store`: WAL pragmas, forward-only migrations, note index with FTS5,
  oplog writer, and a Markdown vault writer with frontmatter handling and atomic
  replacement.
- `xtask seed` to generate a fixture vault and database.
- CI: formatting, clippy with `-D warnings`, and the workspace test suite.
