#!/usr/bin/env bash
# A first-in, first-out queue of Windows test rounds. Each request names the exact commit it
# tests, so a round can never test other code than asked, and each gets its own id and results
# folder, so results can't be mixed up.
#
#   tools/test-queue.sh add [run-tests.sh options] [test names...]   queue the current commit
#   tools/test-queue.sh run                                          work through the queue
#   tools/test-queue.sh status                                       pending, running, done
#
# add: records the commit at HEAD (full id; uncommitted changes are not tested, so it warns about
#   them), the options and tests, and prints the request id (e.g. 0007).
# run: takes requests oldest first; for each it checks out the commit in the test worktree
#   (.claude/worktrees/round), refuses to run if the checkout doesn't match, then runs
#   tools/run-tests.sh with results in <export>/runs/q<id>/ plus a manifest.txt (id, commit,
#   tests, start and end, exit status). Requests added while it runs are picked up; it exits when
#   the queue is empty. Only one runner works at a time (a lock).
#
# Uses CARGO_TARGET_DIR (default ~/.cache/ultra_space/test-target) and ULTRA_SPACE_EXPORT_DIR
# (default /mnt/c/Users/yangc/Apps/ultra_space-test), never the developer's build.
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
QUEUE="$ROOT/.test-queue"
WORKTREE="$ROOT/.claude/worktrees/round"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/ultra_space/test-target}"
export ULTRA_SPACE_EXPORT_DIR="${ULTRA_SPACE_EXPORT_DIR:-/mnt/c/Users/yangc/Apps/ultra_space-test}"
mkdir -p "$QUEUE"/{pending,running,done}

next_id() {
    local last
    last=$(find "$QUEUE" -name '*.req' -printf '%f\n' | sed 's/\.req$//' | sort -n | tail -1)
    printf '%04d' $((10#${last:-0} + 1))
}

field() { grep -m1 "^$1=" "$2" | cut -d= -f2-; }

case "${1:-}" in
add)
    shift
    sha=$(git -C "$ROOT" rev-parse HEAD)
    if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ]]; then
        echo "warning: uncommitted changes are not tested (request tests commit ${sha:0:7})" >&2
    fi
    id=$(next_id)
    {
        echo "id=$id"
        echo "commit=$sha"
        echo "subject=$(git -C "$ROOT" log -1 --format=%s "$sha")"
        echo "args=$*"
        echo "queued=$(date -Is)"
    } > "$QUEUE/pending/$id.req"
    echo "$id"
    ;;
run)
    exec 9> "$QUEUE/runner.lock"
    if ! flock -n 9; then
        echo "a runner is already working through the queue"
        exit 0
    fi
    [[ -d "$WORKTREE" ]] || git -C "$ROOT" worktree add -f --detach "$WORKTREE" HEAD >/dev/null
    while true; do
        req=$(find "$QUEUE/pending" -name '*.req' | sort | head -1)
        [[ -z "$req" ]] && break
        id=$(field id "$req")
        sha=$(field commit "$req")
        args=$(field args "$req")
        started=""
        mv "$req" "$QUEUE/running/"
        req="$QUEUE/running/$id.req"
        runs="runs/q$id"
        out="$ULTRA_SPACE_EXPORT_DIR/$runs"
        mkdir -p "$out"
        echo "== request $id: commit ${sha:0:7} ($(field subject "$req")), tests: ${args:-all}"
        git -C "$WORKTREE" checkout -q --detach "$sha"
        actual=$(git -C "$WORKTREE" rev-parse HEAD)
        if [[ "$actual" != "$sha" ]]; then
            status="refused: worktree is at $actual, not $sha"
        else
            started=$(date -Is)
            # shellcheck disable=SC2086
            # The current runner tools, on the tested commit's code, test scripts and test list.
            (cd "$WORKTREE" && RUNS="$runs" "$ROOT/tools/run-tests.sh" $args) > "$out/round.log" 2>&1
            code=$?
            status="exit $code"
        fi
        {
            cat "$req"
            echo "started=${started:-}"
            echo "finished=$(date -Is)"
            echo "status=$status"
            echo "results=$out"
        } > "$out/manifest.txt"
        cp "$out/manifest.txt" "$QUEUE/done/$id.req"
        rm -f "$req"
        echo "   $status; results in $runs/ (summary.txt, manifest.txt)"
        grep -hE "^== |script: done|panicked|ERROR" "$out/summary.txt" 2>/dev/null | cut -c1-200
    done
    ;;
status)
    for state in running pending; do
        for req in $(find "$QUEUE/$state" -name '*.req' | sort); do
            echo "$state  $(field id "$req")  $(field commit "$req" | cut -c1-7)  $(field args "$req")"
        done
    done
    for req in $(find "$QUEUE/done" -name '*.req' | sort | tail -5); do
        echo "done     $(field id "$req")  $(field commit "$req" | cut -c1-7)  $(field status "$req")  $(field args "$req")"
    done
    ;;
*)
    sed -n '2,20p' "$0"
    exit 2
    ;;
esac
