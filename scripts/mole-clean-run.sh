#!/bin/bash
# Whole-command operation, not an executor for the item-selection protocol.
# The pinned upstream implementation alone selects and validates its targets.
# Synchronous native acknowledgements commit intent BEFORE each mutation sink.
set -euo pipefail

moomole_emit() {
    printf '%s\0%s\0%s\0%s\0' "$1" "${2:-}" "${3:-}" "${4:-}" |
        "$MOOMOLE_EVENT_WORKER" --moomole-clean-event "$MOOMOLE_EVENT_SOCKET"
}
moomole_rename() {
    local moomole_name="$1" moomole_definition
    [[ "$moomole_name" =~ ^[a-zA-Z_][a-zA-Z0-9_]*$ ]] || return 1
    moomole_definition=$(declare -f "$moomole_name") || return 1
    [[ "$moomole_definition" == "$moomole_name ()"* ]] || return 1
    eval "${moomole_definition/$moomole_name ()/moomole_original_$moomole_name ()}"
}
moomole_sink() {
    local moomole_function="$1" moomole_mode="$2" moomole_path="$3"
    shift 3
    local moomole_token moomole_rc=0 moomole_present=false moomole_result=commandCompleted
    local moomole_skip_confirmed=false
    [[ ! -e "$moomole_path" && ! -L "$moomole_path" ]] || moomole_present=true
    moomole_token=$(moomole_emit intent "$moomole_path" "$moomole_function") || return 130
    [[ "$moomole_token" != retained ]] || return 1
    "moomole_original_$moomole_function" "$@" || moomole_rc=$?
    if [[ $moomole_rc -ne 0 ]]; then
        moomole_result=uncertain
        # A fixed upstream SKIPPED callback for this exact target is a known
        # pre-mutation refusal, not an ambiguous failed/partial deletion.
        [[ "$moomole_mode" != remove || "$moomole_skip_confirmed" != true ]] || moomole_result=retained
    elif [[ "$moomole_mode" == remove ]]; then
        moomole_result=retained
        if [[ "$moomole_present" == true && ! -e "$moomole_path" && ! -L "$moomole_path" ]]; then moomole_result=removed; fi
    fi
    moomole_emit "$moomole_result" "$moomole_path" "$moomole_function: $moomole_rc" "$moomole_token" > /dev/null || return 130
    return "$moomole_rc"
}
moomole_install_hooks() {
    local moomole_function
    for moomole_function in safe_remove safe_remove_symlink safe_sudo_remove _remove_verified_container_stub; do
        moomole_rename "$moomole_function" || return 1
        eval "$moomole_function() { moomole_sink '$moomole_function' remove \"\$1\" \"\$@\"; }"
    done
    moomole_rename safe_sudo_find_delete
    safe_sudo_find_delete() { moomole_sink safe_sudo_find_delete command "$1" "$@"; }
    moomole_rename sweep_legacy_analyzer_cache_now
    sweep_legacy_analyzer_cache_now() { moomole_sink sweep_legacy_analyzer_cache_now command "$1" "$@"; }
    moomole_rename _run_simctl
    _run_simctl() {
        if [[ "${2:-}" == delete ]]; then
            moomole_sink _run_simctl command "$HOME/Library/Developer/CoreSimulator/Devices" "$@"
        else
            moomole_original__run_simctl "$@"
        fi
    }
    moomole_rename clean_tool_cache
    clean_tool_cache() {
        # ':' is upstream's explicitly non-mutating whitelist placeholder.
        [[ "${3:-}" != : ]] || { moomole_original_clean_tool_cache "$@"; return $?; }
        moomole_sink clean_tool_cache command "${2:-}" "$@"
    }
    for moomole_function in _run_go_cache_clean_bound _run_github_cli_clear_cache_bound; do
        moomole_rename "$moomole_function" || return 1
        eval "$moomole_function() { moomole_sink '$moomole_function' command \"\$1\" \"\$@\"; }"
    done
    moomole_rename run_with_timeout
    run_with_timeout() {
        case "${2:-}" in sudo|*/sudo) [[ "${MOOMOLE_SYSTEM_ALLOWED:-false}" == true ]] || return 1 ;; esac
        case "${2:-} ${3:-} ${4:-} ${5:-}" in
            'brew cleanup '*|'bun pm cache rm'|'tart prune '*|'tmutil delete '*)
                # --dry-run is never a mutation (Homebrew preflight).
                local moomole_argument moomole_preview=false
                for moomole_argument in "$@"; do [[ "$moomole_argument" != --dry-run ]] || moomole_preview=true; done
                if [[ "$moomole_preview" != true ]]; then
                    local moomole_owner_path=""
                    case "${2:-}" in
                        brew) moomole_owner_path="${BREW_ACTIVE_PREFIX:-}" ;;
                        bun) moomole_owner_path="${bun_cache_path:-}" ;;
                        tart) moomole_owner_path="${cache_root:-}" ;;
                        tmutil) moomole_owner_path="${4:-}" ;;
                    esac
                    moomole_sink run_with_timeout command "$moomole_owner_path" "$@"; return $?
                fi
                ;;
        esac
        moomole_original_run_with_timeout "$@"
    }
    moomole_rename restore_homebrew_link
    restore_homebrew_link() { moomole_sink restore_homebrew_link command "$2" "$@"; }
    moomole_rename log_operation
    log_operation() {
        local moomole_log_path="${3:-}" moomole_log_detail="${2:-}: ${4:-}"
        if [[ "${2:-}" == SKIPPED && -n "${moomole_path:-}" && "${3:-}" == "$moomole_path" ]]; then
            moomole_skip_confirmed=true
        fi
        if [[ -n "$moomole_log_path" && "$moomole_log_path" != /* ]]; then moomole_log_detail+=" ($moomole_log_path)"; moomole_log_path=""; fi
        moomole_emit upstream "$moomole_log_path" "$moomole_log_detail" > /dev/null || return 130
        moomole_original_log_operation "$@"
    }
    moomole_rename start_section
    start_section() {
        moomole_emit section "" "$1" > /dev/null || return 130
        moomole_original_start_section "$@"
    }
    moomole_rename is_path_whitelisted
    is_path_whitelisted() {
        local moomole_target="${1%/}" moomole_protected
        [[ -n "$moomole_target" ]] || moomole_target=/
        if [[ ${#MOOMOLE_PROTECTED_PATHS[@]} -gt 0 ]]; then
            for moomole_protected in "${MOOMOLE_PROTECTED_PATHS[@]}"; do
                if [[ "$moomole_protected" == / || "$moomole_target" == / || "$moomole_target" == "$moomole_protected" || "$moomole_target" == "$moomole_protected/"* || "$moomole_protected" == "$moomole_target/"* ]]; then return 0; fi
            done
        fi
        moomole_original_is_path_whitelisted "$@"
    }
}
moomole_run() {
    [[ $# -eq 4 && "$1" == /* && "$2" == /* && "$3" == /* && ("$4" == user || "$4" == system) ]] || return 1
    MOOMOLE_EVENT_WORKER="$1"; MOOMOLE_EVENT_SOCKET="$2"
    export -n MOOMOLE_EVENT_WORKER MOOMOLE_EVENT_SOCKET
    local moomole_protection_file="$3" moomole_system="$4"
    [[ -f "$moomole_protection_file" && ! -L "$moomole_protection_file" && -O "$moomole_protection_file" ]] || return 1
    MOOMOLE_PROTECTED_PATHS=()
    local moomole_protected
    while IFS= read -r -d '' moomole_protected; do
        [[ "$moomole_protected" == /* ]] || return 1
        [[ "$moomole_protected" == / ]] || moomole_protected="${moomole_protected%/}"
        MOOMOLE_PROTECTED_PATHS+=("$moomole_protected")
    done < "$moomole_protection_file"
    # Read state/owners as the real account. Never launch all of Mole as root.
    export MOLE_TEST_MODE=0 MOLE_DRY_RUN=0 MO_NO_OPLOG=0 NO_COLOR=1 TERM=dumb
    unset MOLE_TEST_NO_AUTH MOLE_UNINSTALL_MODE
    [[ "$MOLE_USER_HOME" == "$HOME" && ${#WHITELIST_WARNINGS[@]} -eq 0 ]] || return 1
    moomole_install_hooks
    MOOMOLE_SYSTEM_ALLOWED=false
    if [[ "$moomole_system" == system ]]; then
        MOOMOLE_SYSTEM_ALLOWED=true
        moomole_emit authorization "" begin > /dev/null || return 1
        # A private native askpass worker reads the non-echoing controlling tty.
        # Credentials go only to sudo's askpass pipe, never to application logs.
        local moomole_auth_rc=0
        SUDO_ASKPASS="${MOOMOLE_EVENT_WORKER%/*}/moomole-clean-askpass" /usr/bin/sudo -A -p '' -v || moomole_auth_rc=$?
        moomole_emit authorization "" finished > /dev/null || return 1
        [[ $moomole_auth_rc -eq 0 ]] || return "$moomole_auth_rc"
    else
        # Explicit user-only choice must ignore any cached system credentials.
        has_sudo_session() { return 1; }
        adopt_sudo_session() { return 1; }
        request_sudo_access() { return 1; }
        sudo() { return 1; }
    fi
    exec 0< /dev/null
    start_cleanup
    if [[ "$moomole_system" == system && "$SYSTEM_CLEAN" != true ]]; then
        printf '%s\n' 'Administrator access could not be adopted; no cleanup started.' >&2
        return 1
    fi
    local moomole_rc=0
    perform_cleanup || moomole_rc=$?
    moomole_emit complete "" "$moomole_rc" > /dev/null || return 1
    return "$moomole_rc"
}
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    export MOLE_TEST_MODE=0 MOLE_DRY_RUN=0 MO_NO_OPLOG=0 NO_COLOR=1 TERM=dumb
    unset MOLE_TEST_NO_AUTH MOLE_UNINSTALL_MODE
    moomole_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    # Upstream declares arrays consumed by its EXIT trap: source at file scope.
    source "$moomole_root/bin/clean.sh"
    moomole_run "$@"
fi
