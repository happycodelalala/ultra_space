#!/usr/bin/env bash
# Runs every check the same way each time. Use before merging, and in CI later.
#
# Usage: tools/ci.sh [--windows]
#   --windows   also type-check the whole workspace for the Windows target
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
WS="$ROOT/impl/rust"
WINDOWS=0
for arg in "$@"; do
    case "$arg" in
        --windows) WINDOWS=1 ;;
        *) echo "usage: tools/ci.sh [--windows]" >&2; exit 2 ;;
    esac
done

# Make cargo available when run from a fresh shell.
[[ -f "$HOME/.cargo/env" ]] && source "$HOME/.cargo/env"

names=(); results=(); times=()
step() {
    local name="$1"; shift
    echo "==> $name"
    local start=$SECONDS
    if "$@"; then results+=("ok"); else results+=("FAIL"); fi
    names+=("$name"); times+=("$((SECONDS - start))s")
}

step "secrets"       "$ROOT/tools/check-secrets.sh" --all
step "boundaries"    "$ROOT/tools/check-boundaries.py"
step "content"       "$ROOT/tools/check-content.py"
step "models"        "$ROOT/tools/gen-models.py" --check
if command -v node >/dev/null; then
    step "viewer"        bash -c "node --test '$ROOT'/impl/viewer/*.test.mjs"
fi
step "format"        bash -c "cd '$WS' && cargo fmt --all --check"
step "clippy"        bash -c "cd '$WS' && cargo clippy --workspace --all-targets --locked -- -D warnings"
step "tests"         bash -c "cd '$WS' && cargo test --workspace --locked"
if [[ $WINDOWS -eq 1 ]]; then
    step "windows check" bash -c "cd '$WS' && cargo check --workspace --locked --target x86_64-pc-windows-gnu"
fi

echo
echo "CI summary"
failed=0
for i in "${!names[@]}"; do
    printf '  %-14s %-5s %s\n' "${names[$i]}" "${results[$i]}" "${times[$i]}"
    [[ "${results[$i]}" == "FAIL" ]] && failed=1
done
if [[ $failed -eq 1 ]]; then echo "CI: FAILED"; exit 1; fi
echo "CI: passed"
