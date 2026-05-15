#!/usr/bin/env bash
# Refresh the bundled pricing snapshot from LiteLLM.
# Run this manually when Anthropic publishes new pricing.
set -euo pipefail

URL="https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"
DEST="$(git rev-parse --show-toplevel)/src/usage/pricing.json"

echo "Fetching $URL"
curl -fsSL "$URL" | jq 'with_entries(select(.key | test("^claude-")))' > "$DEST"
echo "Updated $DEST ($(wc -c < "$DEST") bytes)"
