#!/usr/bin/env bash
# Blocks commits that contain anything that looks like an API key.
# Usage: tools/check-secrets.sh            (checks staged changes)
#        tools/check-secrets.sh --all      (checks all tracked files)
set -euo pipefail

PATTERN='(sk-ant-[A-Za-z0-9_-]{20,}|sk-proj-[A-Za-z0-9_-]{20,}|sk-[A-Za-z0-9]{32,}|(ANTHROPIC|OPENAI)_API_KEY[[:space:]]*[=:][[:space:]]*["'"'"']?[A-Za-z0-9_-]{16,})'

if [[ "${1:-}" == "--all" ]]; then
    matches=$(git grep -nIE "$PATTERN" -- . ':!tools/check-secrets.sh' || true)
else
    matches=$(git diff --cached -U0 --no-color -- . ':!tools/check-secrets.sh' | grep -E '^\+' | grep -E "$PATTERN" || true)
fi

if [[ -n "$matches" ]]; then
    echo "check-secrets: possible API key found, commit blocked:" >&2
    echo "$matches" >&2
    echo "Move keys to environment variables or a gitignored .env file." >&2
    exit 1
fi
