#!/bin/bash
# MooMole adapter for the pinned Mole revision. No upstream source is patched.
# Only the NUL ledger supplies candidates; terminal output remains diagnostic text.
set -euo pipefail

# Observe the fixed engine's append boundary without changing upstream files.
# eval only renames a verified, bundled function definition; no paths, reports
# or user content enter executable text. Subshells inherit the wrapper and its
# file-backed output, so swallowed subprocess failures still stop publication.
moomole_install_origin_hook() {
    if declare -F moomole_upstream_append_target > /dev/null; then return 1; fi
    local definition
    definition=$(declare -f append_dry_run_cleanup_target) || return 1
    [[ "$definition" == 'append_dry_run_cleanup_target ()'* ]] || return 1
    eval "${definition/append_dry_run_cleanup_target/moomole_upstream_append_target}"
    append_dry_run_cleanup_target() {
        local moomole_identity moomole_after moomole_fn moomole_stack="" moomole_depth=0
        [[ "$DRY_RUN" == true && "${MOLE_DRY_RUN:-0}" == 1 ]] || return 1
        [[ -f "$MOOMOLE_ORIGINS" && ! -L "$MOOMOLE_ORIGINS" && -O "$MOOMOLE_ORIGINS" ]] || return 1
        moomole_identity=$(mole_path_identity "$1") || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
        moomole_upstream_append_target "$@" || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
        moomole_after=$(mole_path_identity "$1") || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
        [[ "$moomole_identity" == "$moomole_after" ]] || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
        for moomole_fn in "${FUNCNAME[@]:1}"; do
            [[ "$moomole_fn" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ && ${#moomole_fn} -le 96 ]] || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
            moomole_depth=$((moomole_depth + 1))
            [[ "$moomole_depth" -le 32 ]] || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
            [[ -z "$moomole_stack" ]] || moomole_stack+=,
            moomole_stack+="$moomole_fn"
        done
        [[ $(/usr/bin/stat -f %z "$MOOMOLE_ORIGINS") -le 8388608 ]] || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
        printf '%s\0%s\0%s\0%s\0%s\0' "$moomole_identity" "$1" "$moomole_stack" \
            "${delete_guard:-}" "${_MOLE_SAFE_REMOVE_FINAL_GUARD:-}" >> "$MOOMOLE_ORIGINS" || { : > "$MOOMOLE_ORIGIN_FAILURE"; return 1; }
    }
}

moomole_write_clean_report() {
    local report="$1"
    [[ -f "$MOOMOLE_ORIGINS" && ! -L "$MOOMOLE_ORIGINS" && ! -e "$MOOMOLE_ORIGIN_FAILURE" ]] || return 1
    # Refuse oversized sidecars before concatenating; Rust checks framing,
    # candidates, exact identity associations and per-item evidence budgets.
    [[ $(/usr/bin/stat -f %z "$MOOMOLE_ORIGINS") -le 8388608 ]] || return 1
    printf '%s\0%s\0%s\0' 'MooMole.clean.v2' \
        'ba3a0f57f78e4c22818fda701219cc3dd4d6953f' "$SYSTEM_CLEAN" > "$report"
    emit_deduplicated_dry_run_ledger >> "$report" || return 1
    printf '%s\0' ledger >> "$report" || return 1
    if [[ -n "${CLEAN_PREVIEW_LEDGER_FILE:-}" ]]; then
        [[ -f "$CLEAN_PREVIEW_LEDGER_FILE" && ! -L "$CLEAN_PREVIEW_LEDGER_FILE" ]] || return 1
        [[ $(/usr/bin/stat -f %z "$CLEAN_PREVIEW_LEDGER_FILE") -le 8388608 ]] || return 1
        cat "$CLEAN_PREVIEW_LEDGER_FILE" >> "$report" || return 1
    fi
    printf '%s\0' origins >> "$report"
    cat "$MOOMOLE_ORIGINS" >> "$report" || return 1
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
    export MOOMOLE_ORIGINS="${report}.origins" MOOMOLE_ORIGIN_FAILURE="${report}.origin-failed"
    (umask 077; set -C; : > "$MOOMOLE_ORIGINS") || return 1
    [[ ! -e "$MOOMOLE_ORIGIN_FAILURE" ]] || return 1
    moomole_install_origin_hook || return 1
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
