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
  writeFileSync(request, ['MooMole.rules.request.v3', ...rows.flat(), 'complete', ''].join('\0'))
  writeFileSync(report, '')
  run(prefix + 'WHITELIST_PATTERNS=("$4"); moomole_review_rules "$2" "$3"', [
    engine,
    request,
    report,
    `${scratch}/keep`,
  ])
  const fields = readFileSync(report, 'utf8').split('\0')
  assert.deepEqual(fields.slice(0, 4), ['MooMole.rules.v3', revision, '2', scratch])
  const decoded = new Map()
  for (let i = 4; i < fields.length - 2; i += 6) decoded.set(fields[i], fields.slice(i + 1, i + 6))
  assert.deepEqual(decoded.get('a'), [clear, 'unsupported', 'passed', 'unsupported', 'unsupported'])
  assert.deepEqual(decoded.get('b'), [
    protectedPath,
    'unsupported',
    'whitelist',
    'unsupported',
    'notChecked',
  ])
  assert.deepEqual(decoded.get('c'), [
    model,
    'unsupported',
    'coreProtection',
    'unsupported',
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
  writeFileSync(request, ['MooMole.rules.request.v3', 'a', clear, ''].join('\0'))
  run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report], 1)
  // The request format is versioned; old requests must never run silently.
  writeFileSync(request, ['MooMole.rules.request.v1', 'a', clear, 'complete', ''].join('\0'))
  run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report], 1)
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
  // Exercise the complete v3 wrapper and real guards with isolated HOME only.
  writeFileSync(
    request,
    [
      'MooMole.rules.request.v3',
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
  assert.deepEqual(full.slice(0, 4), ['MooMole.rules.v3', revision, '2', scratch])
  assert.deepEqual(full.slice(4, 10), ['log', log, 'userLogs', 'passed', 'userLogs', 'checked'])
  assert.deepEqual(full.slice(10, 16), [
    'cache',
    resolve(cache, 'app'),
    'userCache',
    'passed',
    'userCache',
    'checked',
  ])
  assert.equal(readFileSync(log, 'utf8'), 'keep log content')
  assert.equal(readFileSync(crash, 'utf8'), 'keep diagnostic content')
  const config = resolve(scratch, '.config/mole')
  mkdirSync(config, { recursive: true })
  writeFileSync(resolve(config, 'whitelist'), '../invalid-policy\n')
  run(prefix + 'load_mole_whitelist "$2"; moomole_validate_rule_policy', [engine, scratch], 1)
  console.log(
    'Mole rule fixtures passed: v3 framing and source binding, isolated HOME, path guards, Deno roots, log identities, root substitution, symlink refusal, unknown/missing guards and invalid policy.',
  )
} finally {
  if (fd != null) closeSync(fd)
  rmSync(scratch, { recursive: true, force: true })
}
