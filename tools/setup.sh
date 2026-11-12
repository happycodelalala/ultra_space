#!/usr/bin/env bash
# One-time repo setup: enables the versioned git hooks in tools/git-hooks.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
git config core.hooksPath tools/git-hooks
echo "Git hooks enabled (tools/git-hooks)."
