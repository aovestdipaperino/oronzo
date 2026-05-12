# Changelog

## [0.3.0] - 2026-05-12

### Added
- Multi-command CLI structure with subcommand dispatch.
- `claudio search <query>` — search and resume sessions (previously the default behavior).
- `claudio mv <from> <to>` — move a folder while preserving Claude Code sessions.
- `claudio account-switch` — interactive account switcher (macOS only).
- `claudio account-save` — save current account credentials to a profile.
- `claudio account-list` — list saved account profiles.
- `claudio account-use <email>` — switch to a saved account non-interactively.

### Changed
- `claudio <query>` no longer works — use `claudio search <query>` instead.

## [0.2.0] - 2026-05-12

### Added
- Renamed project from `claude-search` to `claudio`.

## [0.1.0] - 2026-05-12

### Added
- Initial release as `claude-search`.
- BM25-based session search with interactive selection and resume.
- Session index caching for fast subsequent searches.
