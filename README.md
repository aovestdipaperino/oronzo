# claudio

A toolkit for Claude Code sessions. Search, resume, move, and switch accounts — all from one CLI.

## Requirements

- Claude Code CLI installed
- **macOS**: account commands use Keychain via `security` CLI
- **Linux**: account commands use libsecret via `secret-tool` (`sudo apt install libsecret-tools` / `sudo dnf install libsecret`)
- **Windows**: account commands use Credential Manager via PowerShell

## Installation

### From source

```bash
git clone https://github.com/aovestdipaperino/claudio.git
cd claudio
cargo install --path .
```

### From GitHub

```bash
cargo install --git https://github.com/aovestdipaperino/claudio
```

The binary lands at `~/.cargo/bin/claudio`.

## Commands

### `claudio search <query>`

Search across all locally saved Claude Code sessions using BM25 ranking. Select a result to resume it with `claude --resume`.

```bash
claudio search "fix auth bug"
claudio search "location history cluster"
```

Results show the first user message, working directory, and relevance score. Enter a number to resume that session in its original directory.

### `claudio mv <from> <to>`

Move a project folder while preserving its Claude Code sessions. Updates the project directory under `~/.claude/projects/`, rewrites `cwd` references in all session files, and updates prompt history so arrow-key recall works in the new location.

```bash
claudio mv ~/Code/old-name ~/Code/new-name
```

Without this, moving a folder would orphan all Claude Code sessions associated with it.

### `claudio account-switch`

Interactive account switcher. Shows the active Claude Code account and lets you pick another saved account to switch to.

```bash
claudio account-switch
```

### `claudio account-save`

Save the currently logged-in Claude Code account as a named profile. Stores OAuth credentials in the macOS Keychain and account metadata in `~/.claude-switcher/accounts/`.

```bash
claudio account-save
```

### `claudio account-list`

List all saved account profiles, marking the currently active one.

```bash
claudio account-list
```

### `claudio account-use <email>`

Switch to a saved account non-interactively. Useful in scripts.

```bash
claudio account-use user@example.com
```

### First-time account setup

1. You're logged in as account A — run `claudio account-save`
2. In Claude Code, run `/logout` then `/login` with account B
3. Run `claudio account-save` again
4. From now on, use `claudio account-switch` to swap between them

After switching, restart Claude Code if it's already running.

## How it works

### Search

Sessions are stored as `.jsonl` files in `~/.claude/projects/`. The tool extracts user messages from each session, builds a BM25 index, and ranks results against your query. The index is cached at `~/.cache/claude-search/index.json` and auto-refreshed when sessions change.

See [ALGORITHM.md](ALGORITHM.md) for details on the scoring algorithm.

### Move

The `mv` command does four things:
1. Moves the folder on disk
2. Renames the `~/.claude/projects/<encoded-path>` directory (merging into an existing one if present)
3. Rewrites `cwd` fields in all JSONL session files (including subagent files)
4. Rewrites `cwd` in `~/.claude/history.jsonl` so prompt history reflects the new path

### Account switching

Account metadata (email, display name, OAuth account info) is saved as JSON files in `~/.claude-switcher/accounts/`. OAuth credentials are stored in the OS credential store:

| Platform | Credential backend |
|---|---|
| macOS | Keychain (`security` CLI) |
| Linux | libsecret / GNOME Keyring (`secret-tool` CLI) |
| Windows | Credential Manager (PowerShell P/Invoke) |

Switching swaps both the stored credential and `~/.claude.json` fields.

Based on [claude-code-switch](https://github.com/aovestdipaperino/claude-code-switch).

## Cache

The session search index is saved to `~/.cache/claude-search/index.json`. Only new or modified sessions are re-parsed.

To force a full re-index:

```bash
rm ~/.cache/claude-search/index.json
```

## License

[MIT](LICENSE)
