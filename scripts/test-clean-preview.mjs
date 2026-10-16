import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { resolve } from 'node:path'

const engine = resolve(import.meta.dirname, '../src-tauri/resources/mole')
const scratch = mkdtempSync(resolve(tmpdir(), 'moomole-adapter-test-'))
const ledger = resolve(scratch, 'ledger.nul')
const report = resolve(scratch, 'report.nul')
const revision = 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f'
const parent = `${scratch}/缓存 ' [*]\n\t$(literal)`
const rows = [
  ['path:parent', '8', '3', 'true', 'Apps', parent],
  ['path:child', '2', '1', 'true', 'Apps', `${parent}/child`],
  ['path:unknown', '0', '1', 'false', 'Other', `${scratch}/unknown`],
  ['path:child', '2', '1', 'true', 'Apps', `${parent}/child`],
]
function run(script, args, env = {}, expectedStatus = 0) {
  const result = spawnSync('/bin/bash', ['-c', script, 'moomole-test', ...args], {
    env: {
      ...process.env,
      MOLE_DRY_RUN: '1',
      MOLE_TEST_NO_AUTH: '1',
      MO_NO_OPLOG: '1',
      HOME: scratch,
      SUDO_USER: '',
      DENO_DIR: '',
      TMPDIR: scratch,
      ...env,
    },
    encoding: 'utf8',
    timeout: 15_000,
  })
  assert.equal(
    result.status,
    expectedStatus,
    result.stderr || result.stdout || String(result.error),
  )
}
try {
  writeFileSync(ledger, `${rows.flat().join('\0')}\0`)
  writeFileSync(report, '')
  const origins = resolve(scratch, 'origins.nul')
  writeFileSync(
    origins,
    rows.flatMap((row) => [row[0], row[5], 'fixture_module', '', '']).join('\0') + '\0',
  )
  run(
    'source "$1/bin/clean.sh"; source "$1/moomole-clean-preview.sh"; CLEAN_PREVIEW_LEDGER_FILE="$2"; MOOMOLE_ORIGINS="$4"; MOOMOLE_ORIGIN_FAILURE="$4.failed"; SYSTEM_CLEAN=false; moomole_write_clean_report "$3"',
    [engine, ledger, report, origins],
  )
  const expected = [
    'MooMole.clean.v2',
    revision,
    'false',
    ...rows[0],
    '',
    ...rows[1],
    parent,
    ...rows[2],
    '',
    'ledger',
    ...rows.flat(),
    'origins',
    ...rows.flatMap((row) => [row[0], row[5], 'fixture_module', '', '']),
    'complete',
    '',
  ]
  assert.deepEqual(readFileSync(report, 'utf8').split('\0'), expected)
  // Exercise the actual wrapper lifecycle without scanning user files.
  writeFileSync(report, '')
  run('exec /bin/bash "$1/moomole-clean-preview.sh" "$2"', [engine, report], {
    MOLE_TEST_MODE: '1',
  })
  assert.deepEqual(readFileSync(report, 'utf8').split('\0'), [
    'MooMole.clean.v2',
    revision,
    'false',
    'ledger',
    'origins',
    'complete',
    '',
  ])
  const cache = resolve(scratch, 'Library/Caches/app')
  const logs = resolve(scratch, 'Library/Logs')
  mkdirSync(cache, { recursive: true })
  mkdirSync(logs, { recursive: true })
  writeFileSync(resolve(cache, 'fixture'), 'keep cache')
  writeFileSync(resolve(logs, 'fixture.log'), 'keep log')
  const prefix =
    'source "$1/bin/clean.sh"; source "$1/moomole-clean-preview.sh"; CLEAN_PREVIEW_FINAL_FILE="$2.txt"; prepare_clean_preview_file; export MOOMOLE_ORIGINS="$2.origins" MOOMOLE_ORIGIN_FAILURE="$2.failed"; : > "$MOOMOLE_ORIGINS"; moomole_install_origin_hook; '
  const hooked = resolve(scratch, 'hooked.nul')
  run(
    prefix +
      'clean_user_essentials > /dev/null; fixture_other_module() { safe_clean "$3" "isolated fixture"; }; ( fixture_other_module "$@" ) > /dev/null; SYSTEM_CLEAN=false; moomole_write_clean_report "$2"',
    [engine, hooked, cache],
  )
  const all = readFileSync(hooked, 'utf8').split('\0')
  const ledgerMarker = all.findIndex((v, i) => i >= 3 && (i - 3) % 7 === 0 && v === 'ledger')
  const marker = all.findIndex(
    (v, i) => i > ledgerMarker && (i - ledgerMarker - 1) % 6 === 0 && v === 'origins',
  )
  assert.ok(marker >= 3)
  const candidates = []
  for (let i = 3; i < ledgerMarker; i += 7) candidates.push(all.slice(i, i + 7))
  const observed = []
  for (let i = marker + 1; i < all.length - 2; i += 5) observed.push(all.slice(i, i + 5))
  const candidate = candidates.find((row) => row[5] === cache)
  assert.ok(candidate)
  const same = observed.filter((row) => row[0] === candidate[0] && row[1] === cache)
  assert.ok(
    same.some(
      (row) =>
        row[2].startsWith(
          'record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,clean_user_essentials',
        ) &&
        row[3] === '_user_cache_deno_delete_guard' &&
        row[4] === row[3],
    ),
  )
  assert.ok(
    same.some((row) => row[2].includes('fixture_other_module')),
    'Subshell origin must survive upstream deduplication',
  )
  assert.equal(candidates.filter((row) => row[0] === candidate[0]).length, 1)
  assert.ok(
    observed.some(
      (row) =>
        row[1] === resolve(logs, 'fixture.log') &&
        row[2].startsWith(
          'record_dry_run_cleanup_target,_safe_clean_impl,safe_clean_guarded,_clean_user_log_directory,clean_user_essentials',
        ) &&
        row[3] === '_user_log_delete_guard' &&
        row[4] === row[3],
    ),
  )
  assert.equal(readFileSync(resolve(cache, 'fixture'), 'utf8'), 'keep cache')
  assert.equal(readFileSync(resolve(logs, 'fixture.log'), 'utf8'), 'keep log')
  const failed = resolve(scratch, 'failed.nul')
  run(
    prefix +
      'moomole_upstream_append_target() { return 1; }; append_dry_run_cleanup_target "$3" 1 1 true || true; SYSTEM_CLEAN=false; moomole_write_clean_report "$2"',
    [engine, failed, cache],
    {},
    1,
  )
  console.log(
    'Mole adapter fixtures passed: NUL paths, deduplication, ancestor coverage, empty preview lifecycle, actual module/guard observations, duplicate/subshell origins and swallowed recorder failure.',
  )
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
