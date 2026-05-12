# claude-search

Search across all locally saved Claude Code sessions and resume them with `claude --resume`.

Uses **BM25** to rank sessions by relevance to your query.

---

## Requirements

- Claude Code CLI installed

---

## Installation

### From source

```bash
git clone <url-repo>
cd claude-search
cargo build --release
cp target/release/claude-search ~/.local/bin/
```

### Verify PATH

```bash
echo $PATH | grep -q "$HOME/.local/bin" && echo "OK" || echo 'Add to ~/.bashrc: export PATH="$HOME/.local/bin:$PATH"'
```

---

## Usage

```bash
claude-search "<query>"
```

### Examples

```bash
claude-search "activity report"
claude-search "location history cluster"
claude-search "fix auth bug"
```

### Selection and resume

Enter the number of the session you want and press `Enter`. The tool opens Claude Code in the original working directory of the session.

---

## How it works

See [ALGORITHM.md](ALGORITHM.md) for a detailed description of the algorithms used.

---

## Cache

The session index is saved to `~/.cache/claude-search/index.json` and automatically updated only for new or modified sessions.

To force a full re-index:
```bash
rm ~/.cache/claude-search/index.json
```
