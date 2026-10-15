#!/bin/bash
# MooMole adapter for the pinned Mole revision. No upstream source is patched.
# Only the NUL ledger supplies candidates; terminal output remains diagnostic text.
set -euo pipefail

moomole_write_clean_report() {
    local report="$1"
    printf '%s\0%s\0%s\0' 'MooMole.clean.v1' \
        'ba3a0f57f78e4c22818fda701219cc3dd4d6953f' "$SYSTEM_CLEAN" > "$report"
    emit_deduplicated_dry_run_ledger >> "$report"
    printf '%s\0' 'complete' >> "$report"
}

moomole_clean_preview() {
    [[ $# -eq 1 ]] || return 1
    local report="$1"
    # Rust creates this file inside an owned, private temporary directory.
    [[ "$report" == /* && -f "$report" && ! -L "$report" && -O "$report" ]] || return 1
    [[ "$DRY_RUN" == true ]] || return 1
    # Keep the human preview out of the user's ~/.config/mole/clean-list.txt.
    CLEAN_PREVIEW_FINAL_FILE="${report}.txt"
    start_cleanup
    local cleanup_rc=0
    perform_cleanup || cleanup_rc=$?
    [[ "$cleanup_rc" -eq 0 ]] || return "$cleanup_rc"
    moomole_write_clean_report "$report"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    export MOLE_DRY_RUN=1 MOLE_TEST_NO_AUTH=1 MO_NO_OPLOG=1 NO_COLOR=1 TERM=dumb
    engine_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    # Source at file scope: Mole declares arrays consumed by its EXIT trap.
    source "$engine_root/bin/clean.sh"
    moomole_clean_preview "$@"
fi
