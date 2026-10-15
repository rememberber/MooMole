#!/bin/bash
# Read-only, path-level Mole guards. This is not cleanup authorization.
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

moomole_review_rules() {
    [[ $# -eq 2 ]] || return 1
    local request="$1" report="$2" fn header id path count=0
    [[ "$request" == /* && -f "$request" && ! -L "$request" && -O "$request" ]] || return 1
    [[ "$report" == /* && -f "$report" && ! -L "$report" && -O "$report" ]] || return 1
    for fn in should_protect_path is_path_whitelisted holds_compiled_model_cache \
        _mole_should_refuse_live_user_cache_path _mole_is_sqlite_database_path _mole_sqlite_database_in_use; do
        declare -F "$fn" > /dev/null || return 1
    done
    exec 3< "$request"
    IFS= read -r -d '' header <&3 || return 1
    [[ "$header" == MooMole.rules.request.v1 ]] || return 1
    printf '%s\0%s\0' 'MooMole.rules.v1' 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f' > "$report"
    while IFS= read -r -d '' id <&3; do
        if [[ "$id" == complete ]]; then
            # The Rust producer owns framing; reject unexpected trailing bytes.
            if IFS= read -r -n 1 header <&3; then return 1; fi
            printf '%s\0' complete >> "$report"
            exec 3<&-
            return 0
        fi
        IFS= read -r -d '' path <&3 || return 1
        [[ -n "$id" && "$path" == /* ]] || return 1
        count=$((count + 1))
        [[ "$count" -le 500 ]] || return 1
        moomole_probe_rules "$path"
        printf '%s\0%s\0%s\0' "$id" "$path" "$MOOMOLE_RULE_CODE" >> "$report"
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
