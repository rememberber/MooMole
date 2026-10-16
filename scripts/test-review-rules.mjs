import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'

const engine = resolve(import.meta.dirname, '../src-tauri/resources/mole')
const scratch = mkdtempSync(resolve(tmpdir(), 'moomole-rule-test-'))
const request = resolve(scratch, 'request.nul')
const report = resolve(scratch, 'report.nul')
const revision = 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f'
const clear = `${scratch}/缓存\n\t[*] $(literal)`
const protectedPath = `${scratch}/keep/child`
const model = `${scratch}/com.apple.e5rt.e5bundlecache`
const database = `${scratch}/live.sqlite`
const rows = [
  ['a', clear, 'unsupported'],
  ['b', protectedPath, 'unsupported'],
  ['c', model, 'unsupported'],
  ['d', database, 'unsupported'],
]
const prefix = 'source "$1/bin/clean.sh"; source "$1/moomole-review-rules.sh"; '
function run(script, args, expected = 0) {
  const result = spawnSync('/bin/bash', ['-c', script, 'moomole-test', ...args], {
    env: {
      ...process.env,
      MOLE_DRY_RUN: '1',
      MOLE_TEST_NO_AUTH: '1',
      MO_NO_OPLOG: '1',
      TMPDIR: scratch,
      HOME: scratch,
      SUDO_USER: '',
      DENO_DIR: '',
    },
    encoding: 'utf8',
    timeout: 20_000,
  })
  assert.equal(result.status, expected, result.stderr || result.stdout || String(result.error))
}
let fd
try {
  writeFileSync(database, 'isolated fixture')
  fd = openSync(database, 'r')
  writeFileSync(request, ['MooMole.rules.request.v4', ...rows.flat(), 'complete', ''].join('\0'))
  writeFileSync(report, '')
  run(prefix + 'WHITELIST_PATTERNS=("$4"); moomole_review_rules "$2" "$3"', [
    engine,
    request,
    report,
    `${scratch}/keep`,
  ])
  const fields = readFileSync(report, 'utf8').split('\0')
  assert.deepEqual(fields.slice(0, 5), ['MooMole.rules.v4', revision, '2', '1', scratch])
  const decoded = new Map()
  for (let i = 5; i < fields.length - 2; i += 7) decoded.set(fields[i], fields.slice(i + 1, i + 7))
  assert.deepEqual(decoded.get('a'), [
    clear,
    'unsupported',
    'passed',
    'unsupported',
    'unsupported',
    'notChecked',
  ])
  assert.deepEqual(decoded.get('b'), [
    protectedPath,
    'unsupported',
    'whitelist',
    'unsupported',
    'notChecked',
    'notChecked',
  ])
  assert.deepEqual(decoded.get('c'), [
    model,
    'unsupported',
    'coreProtection',
    'unsupported',
    'notChecked',
    'notChecked',
  ])
  assert.ok(
    ['sqliteInUse', 'sqliteUnknown'].includes(decoded.get('d')[2]),
    'An open database must never be reported as passed',
  )
  assert.equal(decoded.get('d')[4], 'notChecked')
  assert.deepEqual(fields.slice(-2), ['complete', ''])
  run(
    prefix +
      '_mole_should_refuse_live_user_cache_path() { return 2; }; moomole_probe_rules "$2"; [[ "$MOOMOLE_RULE_CODE" == activeOrUncertain ]]',
    [engine, clear],
  )
  run(
    prefix +
      'should_protect_path() { return 99; }; moomole_probe_rules "$2"; [[ "$MOOMOLE_RULE_CODE" == checkFailed ]]',
    [engine, clear],
  )
  writeFileSync(request, ['MooMole.rules.request.v4', 'a', clear, ''].join('\0'))
  run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report], 1)
  // The request format is versioned; old requests must never run silently.
  for (const version of [1, 2, 3]) {
    writeFileSync(
      request,
      [`MooMole.rules.request.v${version}`, 'a', clear, 'complete', ''].join('\0'),
    )
    run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report], 1)
  }
  const cache = resolve(scratch, 'Library/Caches')
  const logs = resolve(scratch, 'Library/Logs')
  const reports = resolve(logs, 'DiagnosticReports')
  mkdirSync(resolve(cache, 'app'), { recursive: true })
  mkdirSync(resolve(cache, 'deno'), { recursive: true })
  mkdirSync(resolve(cache, 'owner/deno-root'), { recursive: true })
  mkdirSync(reports, { recursive: true })
  const log = resolve(logs, 'fixture.log')
  const crash = resolve(reports, '中文 report\n[*].ips')
  writeFileSync(log, 'keep log content')
  writeFileSync(crash, 'keep diagnostic content')
  const moduleProbe =
    prefix +
    'MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" "$3"; [[ "$MOOMOLE_MODULE_SCOPE" == "$3" && "$MOOMOLE_MODULE_CODE" == "$4" ]]'
  for (const [path, scope, code] of [
    [resolve(cache, 'app'), 'userCache', 'checked'],
    [resolve(cache, 'deno'), 'userCache', 'retainOrUnknown'],
    [log, 'userLogs', 'checked'],
    [reports, 'userLogs', 'preserveRoot'],
    [crash, 'userLogs', 'checked'],
    [resolve(cache, 'app/deep'), 'unsupported', 'unsupported'],
    [resolve(cache, '.hidden'), 'unsupported', 'unsupported'],
    [resolve(reports, '.hidden'), 'unsupported', 'unsupported'],
    [resolve(scratch, 'Library/LogsOther/sibling'), 'unsupported', 'unsupported'],
  ])
    run(moduleProbe, [engine, path, scope, code])
  run(
    prefix +
      'DENO_DIR="$2"; MOOMOLE_RULE_CODE=passed; moomole_probe_module "$3" userCache; [[ "$MOOMOLE_MODULE_CODE" == retainOrUnknown ]]',
    [engine, resolve(cache, 'owner/deno-root'), resolve(cache, 'owner')],
  )
  run(
    prefix +
      'DENO_DIR=relative; MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" userCache; [[ "$MOOMOLE_MODULE_CODE" == retainOrUnknown ]]',
    [engine, resolve(cache, 'app')],
  )
  run(
    prefix +
      '_user_cache_deno_delete_guard() { return 99; }; MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" userCache; [[ "$MOOMOLE_MODULE_CODE" == checkFailed ]]',
    [engine, resolve(cache, 'app')],
  )
  run(
    prefix +
      'unset -f _user_log_delete_guard; MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" userLogs; [[ "$MOOMOLE_MODULE_CODE" == checkFailed ]]',
    [engine, log],
  )
  // Change the root exactly between snapshot and the real upstream final guard.
  run(
    prefix +
      `definition=$(declare -f _user_log_delete_guard); eval "${'${definition/_user_log_delete_guard/_fixture_original_log_guard}'}";
    _user_log_delete_guard() { mv "$HOME/Library/Logs" "$HOME/Library/Logs.previous"; mkdir "$HOME/Library/Logs"; _fixture_original_log_guard "$@"; };
    MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" userLogs; [[ "$MOOMOLE_MODULE_CODE" == retainOrUnknown ]];
    rmdir "$HOME/Library/Logs"; mv "$HOME/Library/Logs.previous" "$HOME/Library/Logs"`,
    [engine, log],
  )
  run(
    prefix +
      'mv "$HOME/Library/Logs/DiagnosticReports" "$HOME/reports-original"; ln -s "$HOME/reports-original" "$HOME/Library/Logs/DiagnosticReports"; MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" userLogs; [[ "$MOOMOLE_MODULE_CODE" == retainOrUnknown ]]; unlink "$HOME/Library/Logs/DiagnosticReports"; mv "$HOME/reports-original" "$HOME/Library/Logs/DiagnosticReports"',
    [engine, crash],
  )
  for (const binding of ['missing', 'mixed', 'unsupported', 'userLogs'])
    run(
      prefix +
        'MOOMOLE_RULE_CODE=passed; moomole_probe_module "$2" "$3"; [[ "$MOOMOLE_MODULE_SCOPE" == unsupported && "$MOOMOLE_MODULE_CODE" == unsupported ]]',
      [engine, resolve(cache, 'app'), binding],
    )
  // Exercise the complete v4 wrapper and real guards with isolated HOME only.
  writeFileSync(
    request,
    [
      'MooMole.rules.request.v4',
      'log',
      log,
      'userLogs',
      'cache',
      resolve(cache, 'app'),
      'userCache',
      'outside',
      clear,
      'missing',
      'complete',
      '',
    ].join('\0'),
  )
  run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report])
  const full = readFileSync(report, 'utf8').split('\0')
  assert.deepEqual(full.slice(0, 5), ['MooMole.rules.v4', revision, '2', '1', scratch])
  assert.deepEqual(full.slice(5, 11), ['log', log, 'userLogs', 'passed', 'userLogs', 'checked'])
  assert.deepEqual(full.slice(12, 18), [
    'cache',
    resolve(cache, 'app'),
    'userCache',
    'passed',
    'userCache',
    'checked',
  ])
  for (const index of [11, 18])
    assert.ok(
      ['busy', 'noOpenHandles', 'unknown', 'budgetExhausted', 'checkFailed'].includes(full[index]),
      full[index],
    )
  assert.equal(readFileSync(log, 'utf8'), 'keep log content')
  assert.equal(readFileSync(crash, 'utf8'), 'keep diagnostic content')
  // Real lsof against only our fixtures. The visibility override here is test
  // instrumentation, NOT evidence that the ordinary account sees root handles.
  const direct = '_mole_complete_lsof_mode() { _MOLE_COMPLETE_LSOF_MODE=direct; return 0; }; '
  const activity = prefix + 'MOOMOLE_RULE_CODE=passed; MOOMOLE_MODULE_CODE=checked; '
  const observe = 'moomole_probe_activity "$2"; [[ "$MOOMOLE_ACTIVITY_CODE" == "$3" ]]'
  const nested = resolve(cache, 'app/deep/open.log')
  mkdirSync(resolve(cache, 'app/deep'), { recursive: true })
  writeFileSync(nested, 'held fixture')
  const openLog = openSync(log, 'r')
  const openNested = openSync(nested, 'r')
  try {
    run(activity + direct + observe, [engine, log, 'busy'])
    run(activity + direct + observe, [engine, resolve(cache, 'app'), 'busy'])
  } finally {
    closeSync(openLog)
    closeSync(openNested)
  }
  // Exact child paths are already propagated to parents by upstream. A glob
  // above the candidate may instead match only a deep descendant. Probe both
  // through the SAME unchanged v4 adapter; do not reimplement glob matching.
  const originalRequest = readFileSync(request)
  writeFileSync(
    request,
    [
      'MooMole.rules.request.v4',
      'parent',
      resolve(cache, 'app'),
      'userCache',
      'descendant:parent:0',
      nested,
      'unsupported',
      'complete',
      '',
    ].join('\0'),
  )
  run(
    prefix +
      'WHITELIST_PATTERNS=("$HOME/Library/Caches/*/deep/open.*"); moomole_review_rules "$2" "$3"',
    [engine, request, report],
  )
  const guarded = readFileSync(report, 'utf8').split('\0')
  assert.equal(guarded[8], 'passed')
  assert.equal(guarded[15], 'whitelist')
  assert.equal(guarded[17], 'notChecked')
  assert.equal(guarded[18], 'notChecked')
  writeFileSync(request, originalRequest)
  // Deterministic empty results still run the original identity and lsof-status
  // handling; warning prose, partial field records and timeouts stay distinct.
  for (const [probe, code] of [
    ['return 1', 'noOpenHandles'],
    ['printf "warning: incomplete view\\n"; return 1', 'unknown'],
    ['printf "p42\\nf3\\nnfixture\\n"; return 1', 'busy'],
    ['return 124', 'unknown'],
    ['return 99', 'unknown'],
    [
      'mv "$HOME/Library/Logs/fixture.log" "$HOME/original.log"; printf replacement > "$HOME/Library/Logs/fixture.log"; return 1',
      'unknown',
    ],
  ]) {
    run(activity + direct + `_mole_run_complete_lsof() { ${probe}; }; ` + observe, [
      engine,
      log,
      code,
    ])
    if (code === 'unknown' && probe.startsWith('mv ')) {
      rmSync(log)
      writeFileSync(log, readFileSync(resolve(scratch, 'original.log')))
      rmSync(resolve(scratch, 'original.log'))
    }
  }
  run(activity + '_mole_complete_lsof_mode() { return 2; }; ' + observe, [engine, log, 'unknown'])
  run(
    activity +
      direct +
      '_mole_container_cache_has_open_handle() { rm "$1"; return 1; }; ' +
      observe,
    [engine, log, 'unknown'],
  )
  writeFileSync(log, 'keep log content')
  run(activity + observe, [engine, resolve(logs, 'missing.log'), 'unknown'])
  run(
    activity +
      'MOOMOLE_ACTIVITY_DEADLINE=$SECONDS; _mole_container_cache_has_open_handle() { return 99; }; ' +
      observe,
    [engine, log, 'budgetExhausted'],
  )
  run(
    activity +
      '_mole_container_cache_has_open_handle() { [[ "$(( _MOLE_CONTAINER_CACHE_PROBE_DEADLINE - SECONDS ))" -le 5 ]] || exit 99; return 2; }; ' +
      observe,
    [engine, log, 'unknown'],
  )
  for (const [pathCode, moduleCode] of [
    ['whitelist', 'notChecked'],
    ['passed', 'unsupported'],
    ['passed', 'retainOrUnknown'],
  ])
    run(
      activity +
        `MOOMOLE_RULE_CODE=${pathCode}; MOOMOLE_MODULE_CODE=${moduleCode}; _mole_container_cache_has_open_handle() { exit 99; }; ` +
        observe,
      [engine, log, 'notChecked'],
    )
  run(activity + 'unset -f _mole_container_cache_has_open_handle; ' + observe, [
    engine,
    log,
    'checkFailed',
  ])
  run(activity + '_mole_container_cache_has_open_handle() { return 99; }; ' + observe, [
    engine,
    log,
    'checkFailed',
  ])
  run(
    activity +
      '_mole_container_cache_has_open_handle() { return 130; }; moomole_probe_activity "$2"; exit $?',
    [engine, log],
    130,
  )
  // A failed root-view probe must not attempt even noninteractive sudo.
  run(
    activity +
      '_MOLE_COMPLETE_LSOF_MODE=""; run_with_timeout() { [[ "$2" != sudo ]] || touch "$HOME/unauthorized"; return 1; }; sudo() { touch "$HOME/unauthorized"; return 99; }; ' +
      observe,
    [engine, log, 'unknown'],
  )
  assert.throws(() => readFileSync(resolve(scratch, 'unauthorized')))
  // Exhausting the first item's dynamic deadline affects the next item in
  // the SAME wrapper batch; a per-item reset would incorrectly probe it again.
  run(
    prefix +
      '_mole_container_cache_has_open_handle() { _MOLE_CONTAINER_CACHE_PROBE_DEADLINE=$SECONDS; return 2; }; moomole_review_rules "$2" "$3"',
    [engine, request, report],
  )
  const budget = readFileSync(report, 'utf8').split('\0')
  assert.equal(budget[11], 'unknown')
  assert.equal(budget[18], 'budgetExhausted')
  // A signalled observation produces no complete report and is never accepted.
  run(
    prefix +
      '_mole_container_cache_has_open_handle() { return 130; }; moomole_review_rules "$2" "$3"; exit $?',
    [engine, request, report],
    130,
  )
  assert.ok(!readFileSync(report, 'utf8').endsWith('complete\0'))
  const config = resolve(scratch, '.config/mole')
  mkdirSync(config, { recursive: true })
  writeFileSync(resolve(config, 'whitelist'), '../invalid-policy\n')
  run(prefix + 'load_mole_whitelist "$2"; moomole_validate_rule_policy', [engine, scratch], 1)
  console.log(
    'Mole rule fixtures passed: v4 framing and source binding, isolated HOME, path guards, Deno roots, log identities, root substitution, symlink refusal, exact/recursive live handles, idle identity binding, incomplete visibility, warning/partial/timeout/signal results, shared budget, no authorization, unknown/missing guards and invalid policy.',
  )
} finally {
  if (fd != null) closeSync(fd)
  rmSync(scratch, { recursive: true, force: true })
}
