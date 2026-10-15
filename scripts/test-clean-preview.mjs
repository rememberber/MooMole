import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
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
function run(script, args, env = {}) {
  const result = spawnSync('/bin/bash', ['-c', script, 'moomole-test', ...args], {
    env: { ...process.env, MOLE_TEST_NO_AUTH: '1', MO_NO_OPLOG: '1', ...env },
    encoding: 'utf8',
    timeout: 15_000,
  })
  assert.equal(result.status, 0, result.stderr || result.stdout || String(result.error))
}
try {
  writeFileSync(ledger, `${rows.flat().join('\0')}\0`)
  writeFileSync(report, '')
  run(
    'source "$1/bin/clean.sh"; source "$1/moomole-clean-preview.sh"; CLEAN_PREVIEW_LEDGER_FILE="$2"; SYSTEM_CLEAN=false; moomole_write_clean_report "$3"',
    [engine, ledger, report],
  )
  const expected = [
    'MooMole.clean.v1',
    revision,
    'false',
    ...rows[0],
    '',
    ...rows[1],
    parent,
    ...rows[2],
    '',
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
    'MooMole.clean.v1',
    revision,
    'false',
    'complete',
    '',
  ])
  console.log(
    'Mole adapter fixtures passed: NUL paths, deduplication, ancestor coverage, empty preview lifecycle.',
  )
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
