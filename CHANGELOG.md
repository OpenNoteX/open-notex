# Changelog

All notable changes to this project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
