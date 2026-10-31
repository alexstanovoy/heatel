#!/usr/bin/env bash
# The merge gate of this repository: every check a change must pass, in order, stopping at the first failure.
#
#   tools/gate.sh
#
# Steps: ignored-audit (every #[ignore]d test has a step that runs it), fuzz-check (the fuzz targets compile), clippy and test
# (all features, profile test-release), install-tests (the ignored tests that read a Dota 2 install; no install fails the gate),
# deny, then fmt (last: it changes no code).
# A step that does not run fails the gate; nothing is skipped quietly.
# Environment:
#   GATE_STEP_LOG=FILE   append the seconds each step took to FILE (tab separated)
#   GATE_LOGS=DIR        where each step's output goes (default: temp/gate under the repository)
# Prints GATE-OK when every step ran and passed, GATE-FAIL <step> (with the tail of its output) otherwise.
set -uo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
cd "$here" || exit 2
logs=${GATE_LOGS:-$here/temp/gate}
mkdir -p "$logs"

step() { # name command...
    local name=$1
    shift
    echo "== $name"
    local began=$SECONDS
    "$@" > "$logs/$name.log" 2>&1
    local rc=$?
    [ -z "${GATE_STEP_LOG:-}" ] || printf '%s\t%s\t%s\t%s\t%s\n' "$(date +%FT%T)" "$(basename -- "$here")" "$name" "$((SECONDS - began))" "$rc" >> "$GATE_STEP_LOG"
    if [ $rc -ne 0 ]; then
        tail -n 40 "$logs/$name.log"
        echo "GATE-FAIL $name (full output: $logs/$name.log)"
        exit 1
    fi
    grep -E '^test result' "$logs/$name.log" | awk '{p += $4; f += $6; i += $8} END {if (NR) print "   " p " passed, " f " failed, " i " ignored"}'
}

has_install() {
    local library
    for library in "${DBOTA_DOTA_PATH:+$DBOTA_DOTA_PATH/..}" "$HOME/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common" \
        "$HOME/.steam/steam/steamapps/common" "$HOME/.local/share/Steam/steamapps/common"; do
        [ -n "$library" ] && [ -f "$library/dota 2 beta/game/dota/pak01_dir.vpk" ] && return 0
    done
    [ -n "${DBOTA_DOTA_PATH:-}" ] && [ -f "$DBOTA_DOTA_PATH/game/dota/pak01_dir.vpk" ]
}

step ignored-audit tools/ignored_audit.sh
step fuzz-check fuzz/fuzz.sh check
step clippy cargo clippy --all-targets --all-features -- -D warnings
step test cargo test --profile test-release --all-features
if ! has_install; then
    echo "GATE-FAIL install-tests (no Dota 2 install: the ignored tests that read it cannot run)"
    exit 1
fi
step install-tests cargo test --release --all-features -- --ignored
step deny cargo deny check
step fmt cargo fmt --check
echo "GATE-OK"
