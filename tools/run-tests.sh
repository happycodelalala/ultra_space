#!/usr/bin/env bash
# Runs the test list (tools/test-scripts/tests.txt) on the native Windows build: builds and
# exports once, runs the visual tests several at a time (each in its own window, folder and
# log), then the performance tests one at a time, and writes a summary.
#
#   tools/run-tests.sh [--no-export] [--parallel N] [name ...]
#
# --no-export   use the build already in the export folder
# --parallel N  visual tests at once (default 3)
# name ...      only these tests (default: all)
#
# Respects ULTRA_SPACE_EXPORT_DIR and CARGO_TARGET_DIR like tools/export-windows.sh.
# Results: <export>/runs/<name>/ (screenshots, game.log, output.txt) and <export>/runs/summary.txt.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
DEST="${ULTRA_SPACE_EXPORT_DIR:-/mnt/c/Users/yangc/Apps/ultra_space}"
LIST="$ROOT/tools/test-scripts/tests.txt"
PARALLEL=3
EXPORT=1
ONLY=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-export) EXPORT=0 ;;
        --parallel) PARALLEL="$2"; shift ;;
        *) ONLY+=("$1") ;;
    esac
    shift
done

if [[ $EXPORT == 1 ]]; then
    "$ROOT/tools/export-windows.sh" || { echo "export failed"; exit 1; }
fi

trim() { local s="$1"; s="${s#"${s%%[![:space:]]*}"}"; echo "${s%"${s##*[![:space:]]}"}"; }

VISUAL=()
PERF=()
while IFS='|' read -r name script settings kind _checks; do
    name="$(trim "${name:-}")"
    [[ -z "$name" || "$name" == \#* ]] && continue
    if [[ ${#ONLY[@]} -gt 0 && ! " ${ONLY[*]} " == *" $name "* ]]; then continue; fi
    entry="$name|$(trim "$script")|$(trim "$settings")"
    if [[ "$(trim "$kind")" == perf ]]; then PERF+=("$entry"); else VISUAL+=("$entry"); fi
done < "$LIST"

run_one() {
    local name script settings
    IFS='|' read -r name script settings <<< "$1"
    mkdir -p "$DEST/runs/$name"
    # shellcheck disable=SC2086
    RUN_NAME="$name" "$ROOT/tools/run-windows.sh" "$ROOT/tools/test-scripts/$script" $settings \
        > "$DEST/runs/$name/output.txt" 2>&1
    echo "finished $name"
}

echo "Visual tests (${#VISUAL[@]}, $PARALLEL at a time)..."
running=0
for entry in "${VISUAL[@]}"; do
    run_one "$entry" &
    running=$((running + 1))
    if [[ $running -ge $PARALLEL ]]; then wait -n; running=$((running - 1)); fi
    sleep 2   # stagger window start-up
done
wait

if [[ ${#PERF[@]} -gt 0 ]]; then
    echo "Performance tests (${#PERF[@]}, one at a time)..."
    for entry in "${PERF[@]}"; do run_one "$entry"; done
fi

SUMMARY="$DEST/runs/summary.txt"
{
    echo "Test run $(date -Is), commit $(git -C "$ROOT" rev-parse --short HEAD)"
    for entry in "${VISUAL[@]}" "${PERF[@]}"; do
        name="${entry%%|*}"
        dir="$DEST/runs/$name"
        echo
        echo "== $name"
        grep -hE "script: done|panicked|ERROR" "$dir/game.log" 2>/dev/null | cut -c 1-220 || true
        echo "hitches: $(grep -c "hitch" "$dir/game.log" 2>/dev/null || echo 0)"
        find "$dir" -maxdepth 1 -name '*.png' -printf '%f ' 2>/dev/null; echo
    done
} > "$SUMMARY"
cat "$SUMMARY"
