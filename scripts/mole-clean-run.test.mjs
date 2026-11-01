import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

const root = resolve(import.meta.dirname, '..')
const engine = join(root, 'src-tauri/resources/mole')
const adapter = join(root, 'scripts/mole-clean-run.sh')
function owned(run) {
  const work = mkdtempSync(join(tmpdir(), 'mole-clean-owned-'))
  try {
    mkdirSync(join(work, 'cache'))
    return run(work, (script) => {
      const result = spawnSync(
        '/bin/bash',
        [
          '-c',
          `source "$MOOMOLE_TEST_ENGINE/bin/clean.sh"\nsource "$MOOMOLE_TEST_ADAPTER"\n${script}\nprintf '\\nMooMole-owned-complete\\n'`,
        ],
        {
          env: {
            ...process.env,
            MOOMOLE_TEST_ENGINE: engine,
            MOOMOLE_TEST_ADAPTER: adapter,
            MOOMOLE_TEST_ROOT: work,
            MOLE_TEST_MODE: '0',
            MOLE_DRY_RUN: '0',
            MOLE_TEST_NO_AUTH: '1',
            MO_NO_OPLOG: '1',
            NO_COLOR: '1',
            TERM: 'dumb',
          },
          encoding: 'utf8',
          timeout: 30_000,
          maxBuffer: 4 * 1024 ** 2,
        },
      )
      assert.equal(result.status, 0, result.stderr + result.stdout)
      assert.ok(result.stdout.endsWith('MooMole-owned-complete\n'), result.stderr + result.stdout)
      return result
    })
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}
const recorder = `
MOOMOLE_PROTECTED_PATHS=()
moomole_emit() {
  printf '%s|%s|%s|%s\\n' "$1" "\${2:-}" "\${3:-}" "\${4:-}" >> "$MOOMOLE_TEST_ROOT/events"
  if [[ "$1" == intent ]]; then
    [[ "\${MOOMOLE_REFUSE_INTENT:-false}" != true ]] || return 1
    printf '31d8c490-1245-414b-8d62-0c6aab303555'
  fi
}
moomole_install_hooks
`
test('all pinned mutation boundaries are hooked without replacing upstream safety implementations', () =>
  owned((_work, run) => {
    run(`
  names=(safe_remove safe_remove_symlink safe_sudo_remove safe_sudo_find_delete _remove_verified_container_stub clean_tool_cache _run_go_cache_clean_bound _run_github_cli_clear_cache_bound restore_homebrew_link run_with_timeout sweep_legacy_analyzer_cache_now _run_simctl)
  for name in "\${names[@]}"; do declare -f "$name" > "$MOOMOLE_TEST_ROOT/$name.before"; done
  ${recorder}
  for name in "\${names[@]}"; do
    expected=$(cat "$MOOMOLE_TEST_ROOT/$name.before")
    actual=$(declare -f "moomole_original_$name")
    [[ "$actual" == "\${expected/$name ()/moomole_original_$name ()}" ]] || exit 21
  done
  `)
  }))
test('real Mole deletion occurs only after acknowledged intent; refused intent never removes its owned sample', () =>
  owned((work, run) => {
    writeFileSync(join(work, 'cache/removable.txt'), 'owned removable')
    writeFileSync(join(work, 'cache/kept.txt'), 'owned kept')
    const result = run(`${recorder}
    safe_remove "$MOOMOLE_TEST_ROOT/cache/removable.txt" true
    MOOMOLE_REFUSE_INTENT=true
    rc=0
    safe_remove "$MOOMOLE_TEST_ROOT/cache/kept.txt" true || rc=$?
    [[ $rc -eq 130 && -f "$MOOMOLE_TEST_ROOT/cache/kept.txt" ]]
  `)
    assert.equal(
      existsSync(join(work, 'cache/removable.txt')),
      false,
      readFileSync(join(work, 'events'), 'utf8') + result.stderr + result.stdout,
    )
    assert.equal(readFileSync(join(work, 'cache/kept.txt'), 'utf8'), 'owned kept')
    const events = readFileSync(join(work, 'events'), 'utf8').split('\n').filter(Boolean)
    const intent = events.findIndex((e) => e.startsWith('intent|') && e.includes('removable.txt'))
    const removed = events.findIndex((e) => e.startsWith('removed|') && e.includes('removable.txt'))
    assert.ok(intent >= 0 && removed > intent)
    assert.equal(
      events.some((e) => e.startsWith('removed|') && e.includes('kept.txt')),
      false,
    )
  }))
test('Mole identity recheck, final guards and client protection preserve owned files', () =>
  owned((work, run) => {
    for (const name of ['changed', 'guarded', 'protected'])
      writeFileSync(join(work, `cache/${name}.txt`), 'owned kept')
    run(`${recorder}
    _mole_snapshot_path_identity "$MOOMOLE_TEST_ROOT/cache/changed.txt"
    old_parent="$_MOLE_PATH_SNAPSHOT_PARENT"; old_parent_id="$_MOLE_PATH_SNAPSHOT_PARENT_ID"; old_target_id="$_MOLE_PATH_SNAPSHOT_TARGET_ID"
    mv "$MOOMOLE_TEST_ROOT/cache/changed.txt" "$MOOMOLE_TEST_ROOT/cache/old.txt"
    printf replacement > "$MOOMOLE_TEST_ROOT/cache/changed.txt"
    safe_remove "$MOOMOLE_TEST_ROOT/cache/changed.txt" true '' '' "$old_parent" "$old_parent_id" "$old_target_id" && exit 23
    owned_deny() { return 1; }
    _MOLE_SAFE_REMOVE_FINAL_GUARD=owned_deny
    safe_remove "$MOOMOLE_TEST_ROOT/cache/guarded.txt" true && exit 24
    unset _MOLE_SAFE_REMOVE_FINAL_GUARD
    MOOMOLE_PROTECTED_PATHS=("$MOOMOLE_TEST_ROOT/cache/protected.txt")
    safe_remove "$MOOMOLE_TEST_ROOT/cache/protected.txt" true && exit 25
    [[ -f "$MOOMOLE_TEST_ROOT/cache/changed.txt" && -f "$MOOMOLE_TEST_ROOT/cache/guarded.txt" && -f "$MOOMOLE_TEST_ROOT/cache/protected.txt" ]]
  `)
    assert.equal(readFileSync(join(work, 'cache/changed.txt'), 'utf8'), 'replacement')
    assert.equal(readFileSync(join(work, 'cache/guarded.txt'), 'utf8'), 'owned kept')
  }))
test('owner-command mutations are recorded; package-manager dry runs remain observations', () =>
  owned((work, run) => {
    run(`${recorder}
    moomole_original_run_with_timeout() { printf 'called\\n' >> "$MOOMOLE_TEST_ROOT/owner-calls"; }
    moomole_original__run_simctl() { printf 'called\\n' >> "$MOOMOLE_TEST_ROOT/owner-calls"; }
    _run_simctl 5 list devices unavailable
    _run_simctl 5 delete unavailable
    BREW_ACTIVE_PREFIX="$MOOMOLE_TEST_ROOT/cache"
    bun_cache_path="$MOOMOLE_TEST_ROOT/cache"
    cache_root="$MOOMOLE_TEST_ROOT/cache"
    run_with_timeout 5 brew cleanup --prune=30 --dry-run
    run_with_timeout 5 brew cleanup --prune=30
    run_with_timeout 5 bun pm cache rm
    run_with_timeout 5 tart prune --entries caches --older-than 30
    run_with_timeout 5 tmutil delete "$MOOMOLE_TEST_ROOT/cache/backup.inProgress"
  `)
    const events = readFileSync(join(work, 'events'), 'utf8').split('\n')
    assert.equal(events.filter((e) => e.startsWith('intent|')).length, 5)
    assert.equal(events.filter((e) => e.startsWith('commandCompleted|')).length, 5)
    assert.equal(readFileSync(join(work, 'owner-calls'), 'utf8').trim().split('\n').length, 7)
  }))
test('the complete upstream section traversal is retained, including review-only hints', () =>
  owned((work, run) => {
    const source = readFileSync(join(engine, 'bin/clean.sh'), 'utf8')
    const body = source.slice(
      source.indexOf('    run_clean_sections() {'),
      source.indexOf('    run_clean_sections ||'),
    )
    const functions = [...body.matchAll(/_run_cleanup_step ([a-zA-Z_][a-zA-Z0-9_]*)/g)].map(
      (m) => m[1],
    )
    assert.ok(
      functions.includes('clean_deep_system') &&
        functions.includes('clean_browsers') &&
        functions.includes('clean_developer_tools') &&
        functions.includes('clean_orphaned_app_data') &&
        functions.includes('clean_time_machine_failed_backups'),
    )
    assert.ok(
      functions.includes('check_large_file_candidates') &&
        functions.includes('show_project_artifact_hint_notice'),
    )
    run(`${recorder}
    check_tcc_permissions() { :; }
    start_legacy_analyzer_cache_sweep() { :; }
    adopt_sudo_session() { return 1; }
    ${functions.map((name) => `${name}() { printf '%s\\n' '${name}' >> "$MOOMOLE_TEST_ROOT/sections"; }`).join('\n')}
    start_cleanup
    SYSTEM_CLEAN=true
    perform_cleanup
  `)
    assert.deepEqual(readFileSync(join(work, 'sections'), 'utf8').trim().split('\n'), functions)
  }))

test('user-only choice blocks explicit sudo even when a caller has cached credentials', () =>
  owned((work, run) => {
    run(`${recorder}
    MOOMOLE_SYSTEM_ALLOWED=false
    moomole_original_run_with_timeout(){ printf bad > "$MOOMOLE_TEST_ROOT/privileged-started"; }
    run_with_timeout 2 /usr/bin/sudo -n true && exit 29
    run_with_timeout 2 sudo -n true && exit 30
    printf checked
  `)
    assert.equal(existsSync(join(work, 'privileged-started')), false)
  }))

test('legacy analyzer sweep records its exact root and keeps non-cache state files', () =>
  owned((work, run) => {
    writeFileSync(join(work, 'cache/legacy.cache'), 'regenerable')
    writeFileSync(join(work, 'cache/keep.txt'), 'owned state')
    run(`${recorder}
      sweep_legacy_analyzer_cache_now "$MOOMOLE_TEST_ROOT/cache"
    `)
    assert.equal(existsSync(join(work, 'cache/legacy.cache')), false)
    assert.equal(readFileSync(join(work, 'cache/keep.txt'), 'utf8'), 'owned state')
    const events = readFileSync(join(work, 'events'), 'utf8')
    assert.ok(events.includes(`intent|${work}/cache|sweep_legacy_analyzer_cache_now`))
    assert.ok(events.includes(`commandCompleted|${work}/cache|`))
  }))
