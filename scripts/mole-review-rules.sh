#!/bin/bash
# Read-only Mole path guards plus explicitly bounded module observations.
# This never calls a cleanup lifecycle or deletion sink; no execution authority.
set -euo pipefail

moomole_probe_rules() {
    local path="$1" state=0
    MOOMOLE_RULE_CODE=passed
    should_protect_path "$path" >&2 || state=$?
    case "$state" in 0) MOOMOLE_RULE_CODE=coreProtection; return ;; 1) ;; *) MOOMOLE_RULE_CODE=checkFailed; return ;; esac
    state=0
    is_path_whitelisted "$path" >&2 || state=$?
    case "$state" in 0) MOOMOLE_RULE_CODE=whitelist; return ;; 1) ;; *) MOOMOLE_RULE_CODE=checkFailed; return ;; esac
    state=0
    holds_compiled_model_cache "$path" >&2 || state=$?
    case "$state" in 0) MOOMOLE_RULE_CODE=compiledModel; return ;; 1) ;; *) MOOMOLE_RULE_CODE=checkFailed; return ;; esac
    state=0
    _mole_should_refuse_live_user_cache_path "$path" >&2 || state=$?
    # Upstream combines active and uncertain owners in return code 0.
    case "$state" in 0|2) MOOMOLE_RULE_CODE=activeOrUncertain; return ;; 1) ;; *) MOOMOLE_RULE_CODE=checkFailed; return ;; esac
    state=0
    _mole_is_sqlite_database_path "$path" >&2 || state=$?
    case "$state" in
        0)
            state=0
            _mole_sqlite_database_in_use "$path" >&2 || state=$?
            case "$state" in 0) MOOMOLE_RULE_CODE=sqliteInUse ;; 1) ;; 2) MOOMOLE_RULE_CODE=sqliteUnknown ;; *) MOOMOLE_RULE_CODE=checkFailed ;; esac
            ;;
        1) ;;
        *) MOOMOLE_RULE_CODE=checkFailed ;;
    esac
}

# Scope comes from path boundaries, never terminal section labels. This only
# observes these named module predicates; other modules may impose extra rules.
moomole_module_scope() {
    local path="$1" parent="${1%/*}"
    MOOMOLE_MODULE_SCOPE=unsupported
    [[ "${path##*/}" != .* ]] || return 0
    case "$parent" in
        "$HOME/Library/Caches") MOOMOLE_MODULE_SCOPE=userCache ;;
        "$HOME/Library/Logs"|"$HOME/Library/Logs/DiagnosticReports") MOOMOLE_MODULE_SCOPE=userLogs ;;
    esac
}

moomole_probe_log_module() {
    local candidate="$1" ancestor="$HOME/Library/Logs"
    local _MOLE_USER_LOG_ROOT="$ancestor"
    [[ -d "$ancestor" ]] || return 3
    # Mirror _clean_user_log_directory's physical-root prerequisites, without
    # enumerating targets or calling safe_clean_guarded.
    while [[ "$ancestor" != "$HOME" ]]; do
        [[ "$ancestor" == "$HOME/"* && ! -L "$ancestor" ]] || return 1
        ancestor="${ancestor%/*}"
    done
    _mole_snapshot_path_identity "$_MOLE_USER_LOG_ROOT" || return 3
    local _MOLE_USER_LOG_PARENT="$_MOLE_PATH_SNAPSHOT_PARENT"
    local _MOLE_USER_LOG_PARENT_ID="$_MOLE_PATH_SNAPSHOT_PARENT_ID"
    local _MOLE_USER_LOG_ROOT_ID="$_MOLE_PATH_SNAPSHOT_TARGET_ID"
    local _MOLE_USER_REPORTS_ID="" _MOLE_USER_REPORTS_PARENT=""
    if [[ "${candidate%/*}" == "$_MOLE_USER_LOG_ROOT/DiagnosticReports" ]]; then
        [[ -d "$_MOLE_USER_LOG_ROOT/DiagnosticReports" && ! -L "$_MOLE_USER_LOG_ROOT/DiagnosticReports" ]] || return 1
        _mole_snapshot_path_identity "$_MOLE_USER_LOG_ROOT/DiagnosticReports" || return 3
        _MOLE_USER_REPORTS_ID="$_MOLE_PATH_SNAPSHOT_TARGET_ID"
        _MOLE_USER_REPORTS_PARENT="$_MOLE_PATH_SNAPSHOT_PARENT"
    fi
    _user_log_delete_guard "$candidate"
}

moomole_probe_module() {
    local path="$1" binding="${2:-missing}" state=0 fn
    moomole_module_scope "$path"
    [[ "$binding" == "$MOOMOLE_MODULE_SCOPE" && "$binding" != unsupported ]] || MOOMOLE_MODULE_SCOPE=unsupported
    MOOMOLE_MODULE_CODE=notChecked
    [[ "$MOOMOLE_RULE_CODE" == passed ]] || return 0
    case "$MOOMOLE_MODULE_SCOPE" in
        userCache)
            declare -F _user_cache_deno_delete_guard > /dev/null || { MOOMOLE_MODULE_CODE=checkFailed; return 0; }
            _user_cache_deno_delete_guard "$path" >&2 || state=$?
            ;;
        userLogs)
            if [[ "$path" == "$HOME/Library/Logs/DiagnosticReports" ]]; then
                MOOMOLE_MODULE_CODE=preserveRoot
                return 0
            fi
            for fn in _user_log_delete_guard _mole_snapshot_path_identity _mole_path_matches_identity; do
                declare -F "$fn" > /dev/null || { MOOMOLE_MODULE_CODE=checkFailed; return 0; }
            done
            moomole_probe_log_module "$path" >&2 || state=$?
            ;;
        *) MOOMOLE_MODULE_CODE=unsupported; return 0 ;;
    esac
    case "$state" in
        0) MOOMOLE_MODULE_CODE=checked ;;
        1) MOOMOLE_MODULE_CODE=retainOrUnknown ;;
        *) MOOMOLE_MODULE_CODE=checkFailed ;;
    esac
}

moomole_review_rules() {
    [[ $# -eq 2 && "$MOLE_USER_HOME" == "$HOME" ]] || return 1
    local request="$1" report="$2" fn header id path binding count=0
    [[ "$request" == /* && -f "$request" && ! -L "$request" && -O "$request" ]] || return 1
    [[ "$report" == /* && -f "$report" && ! -L "$report" && -O "$report" ]] || return 1
    for fn in should_protect_path is_path_whitelisted holds_compiled_model_cache \
        _mole_should_refuse_live_user_cache_path _mole_is_sqlite_database_path _mole_sqlite_database_in_use; do
        declare -F "$fn" > /dev/null || return 1
    done
    exec 3< "$request"
    IFS= read -r -d '' header <&3 || return 1
    [[ "$header" == MooMole.rules.request.v3 ]] || return 1
    printf '%s\0%s\0%s\0%s\0' 'MooMole.rules.v3' 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f' '2' "$HOME" > "$report"
    while IFS= read -r -d '' id <&3; do
        if [[ "$id" == complete ]]; then
            # The Rust producer owns framing; reject unexpected trailing bytes.
            if IFS= read -r -n 1 header <&3; then return 1; fi
            printf '%s\0' complete >> "$report"
            exec 3<&-
            return 0
        fi
        IFS= read -r -d '' path <&3 || return 1
        IFS= read -r -d '' binding <&3 || return 1
        case "$binding" in userCache|userLogs|missing|mixed|unsupported) ;; *) return 1 ;; esac
        [[ -n "$id" && "$path" == /* ]] || return 1
        count=$((count + 1))
        [[ "$count" -le 500 ]] || return 1
        moomole_probe_rules "$path"
        moomole_probe_module "$path" "$binding"
        printf '%s\0%s\0%s\0%s\0%s\0%s\0' "$id" "$path" "$binding" "$MOOMOLE_RULE_CODE" "$MOOMOLE_MODULE_SCOPE" "$MOOMOLE_MODULE_CODE" >> "$report"
    done
    return 1
}

moomole_validate_rule_policy() {
    if [[ ${#WHITELIST_WARNINGS[@]} -ne 0 ]]; then
        printf '%s\n' 'Mole whitelist contains ignored or invalid rules; review the whitelist before continuing.' >&2
        return 1
    fi
    local whitelist="$MOLE_USER_HOME/.config/mole/whitelist"
    if [[ (-e "$whitelist" || -L "$whitelist") && ! -f "$whitelist" ]]; then
        printf '%s\n' 'Mole whitelist is not a readable regular file.' >&2
        return 1
    fi
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    export MOLE_DRY_RUN=1 MOLE_TEST_NO_AUTH=1 MO_NO_OPLOG=1 NO_COLOR=1 TERM=dumb
    export MOLE_UNINSTALL_MODE=0 MOLE_TEST_MODE=0 _MOLE_DEFER_CONTAINER_HANDLE_PROBE=false
    engine_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    source "$engine_root/bin/clean.sh"
    # Refuse policy configurations that Mole had to ignore or could not read.
    moomole_validate_rule_policy
    moomole_review_rules "$@"
fi
