<img src="src/resources/logo.png" align="right" width="120">

# oronzo

A toolkit for Claude Code sessions. Search, resume, move, and switch accounts — all from one CLI.

## Requirements

- Claude Code CLI installed
- **macOS**: account commands use Keychain via `security` CLI
- **Linux**: account commands use libsecret via `secret-tool` (`sudo apt install libsecret-tools` / `sudo dnf install libsecret`)
- **Windows**: account commands use Credential Manager via PowerShell

## Installation

### Homebrew (macOS / Linux)

```bash
brew install aovestdipaperino/tap/oronzo
```

### Scoop (Windows)

```powershell
scoop bucket add aovestdipaperino https://github.com/aovestdipaperino/scoop-bucket
scoop install oronzo
```

### From source

```bash
git clone https://github.com/aovestdipaperino/oronzo.git
cd oronzo
cargo install --path .
```

### From GitHub

```bash
cargo install --git https://github.com/aovestdipaperino/oronzo
```

The binary lands at `~/.cargo/bin/oronzo`.

## Commands

### `oronzo search <query>`

Search across all locally saved Claude Code sessions using BM25 ranking. Select a result to resume it with `claude --resume`.

```bash
oronzo search "fix auth bug"
oronzo search "location history cluster"
```

Results show the first user message, working directory, and relevance score. Enter a number to resume that session in its original directory.

### `oronzo mv <from> <to>`

Move a project folder while preserving its Claude Code sessions. Updates the project directory under `~/.claude/projects/`, rewrites `cwd` references in all session files, and updates prompt history so arrow-key recall works in the new location.

```bash
oronzo mv ~/Code/old-name ~/Code/new-name
```

Without this, moving a folder would orphan all Claude Code sessions associated with it.

### `oronzo account-switch`

Interactive account switcher. Shows the active Claude Code account and lets you pick another saved account to switch to.

```bash
oronzo account-switch
```

### `oronzo account-save [alias]`

Save the currently logged-in Claude Code account as a named profile. Stores OAuth credentials in the OS credential store and account metadata in `~/.claude-switcher/accounts/`.

An optional alias lets you refer to the account by a short name (e.g. `work`, `personal`) instead of the full email.

```bash
oronzo account-save
oronzo account-save work
```

### `oronzo account-list`

List all saved account profiles, marking the currently active one.

```bash
oronzo account-list
```

### `oronzo account-use <email|alias>`

Switch to a saved account non-interactively. Accepts an email or alias. Useful in scripts.

```bash
oronzo account-use user@example.com
oronzo account-use work
```

### First-time account setup

1. You're logged in as account A — run `oronzo account-save work`
2. In Claude Code, run `/logout` then `/login` with account B
3. Run `oronzo account-save personal`
4. From now on, use `oronzo account-switch` or `oronzo account-use work` to swap between them

After switching, restart Claude Code if it's already running.

## How it works

### Search

Sessions are stored as `.jsonl` files in `~/.claude/projects/`. The tool extracts user messages from each session, builds a BM25 index, and ranks results against your query. The index is cached at `~/.cache/claude-search/index.json` and auto-refreshed when sessions change.

Based on [claude-search](https://github.com/sangelastro/claude-search).

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

Based on [claude-code-switcher](https://github.com/eddya92/claude-code-switcher).

## Cache

The session search index is saved to `~/.cache/claude-search/index.json`. Only new or modified sessions are re-parsed.

To force a full re-index:

```bash
rm ~/.cache/claude-search/index.json
```

## License

[MIT](LICENSE)
