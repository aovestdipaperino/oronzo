# Changelog

## [0.3.0] - 2026-05-12

### Added
- Multi-command CLI structure with subcommand dispatch.
- `oronzo search <query>` — search and resume sessions (previously the default behavior).
- `oronzo mv <from> <to>` — move a folder while preserving Claude Code sessions.
- `oronzo account-switch` — interactive account switcher (macOS only).
- `oronzo account-save` — save current account credentials to a profile.
- `oronzo account-list` — list saved account profiles.
- `oronzo account-use <email>` — switch to a saved account non-interactively.

### Changed
- `oronzo <query>` no longer works — use `oronzo search <query>` instead.

## [0.2.0] - 2026-05-12

### Added
- Renamed project from `claude-search` to `oronzo`.

## [0.1.0] - 2026-05-12

### Added
- Initial release as `claude-search`.
- BM25-based session search with interactive selection and resume.
- Session index caching for fast subsequent searches.
