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
  ['a', clear],
  ['b', protectedPath],
  ['c', model],
  ['d', database],
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
  writeFileSync(request, ['MooMole.rules.request.v1', ...rows.flat(), 'complete', ''].join('\0'))
  writeFileSync(report, '')
  run(prefix + 'WHITELIST_PATTERNS=("$4"); moomole_review_rules "$2" "$3"', [
    engine,
    request,
    report,
    `${scratch}/keep`,
  ])
  const fields = readFileSync(report, 'utf8').split('\0')
  assert.deepEqual(fields.slice(0, 11), [
    'MooMole.rules.v1',
    revision,
    'a',
    clear,
    'passed',
    'b',
    protectedPath,
    'whitelist',
    'c',
    model,
    'coreProtection',
  ])
  assert.ok(
    ['sqliteInUse', 'sqliteUnknown'].includes(fields[13]),
    'An open database must never be reported as passed',
  )
  assert.deepEqual(fields.slice(14), ['complete', ''])
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
  writeFileSync(request, ['MooMole.rules.request.v1', 'a', clear, ''].join('\0'))
  run('exec /bin/bash "$1/moomole-review-rules.sh" "$2" "$3"', [engine, request, report], 1)
  const config = resolve(scratch, '.config/mole')
  mkdirSync(config, { recursive: true })
  writeFileSync(resolve(config, 'whitelist'), '../invalid-policy\n')
  run(prefix + 'load_mole_whitelist "$2"; moomole_validate_rule_policy', [engine, scratch], 1)
  console.log(
    'Mole rule fixtures passed: exact NUL paths, whitelist, model guard, open database, unknown state and invalid policy.',
  )
} finally {
  if (fd != null) closeSync(fd)
  rmSync(scratch, { recursive: true, force: true })
}
