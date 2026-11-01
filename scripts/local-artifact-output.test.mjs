import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { publishLocalArtifacts } from './local-artifact-output.mjs'

function fixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'moomole-output-test-')))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const stage = join(root, 'stage'),
    output = join(root, 'local-artifacts')
  mkdirSync(stage)
  writeFileSync(join(stage, 'validation.json'), '{}\n')
  writeFileSync(join(stage, 'MooMole-0.2.0-mac-arm64.dmg'), 'first validated image')
  return { root, stage, output, names: ['validation.json', 'MooMole-0.2.0-mac-arm64.dmg'] }
}
test('successful local deliveries replace the preceding set without accumulating directories', (t) => {
  const f = fixture(t)
  publishLocalArtifacts(f.stage, f.output, f.names)
  writeFileSync(join(f.stage, 'MooMole-0.2.1-mac-arm64.dmg'), 'second validated image')
  publishLocalArtifacts(f.stage, f.output, ['validation.json', 'MooMole-0.2.1-mac-arm64.dmg'])
  assert.deepEqual(readdirSync(f.output).sort(), [
    '.moomole-artifacts.json',
    'MooMole-0.2.1-mac-arm64.dmg',
    'validation.json',
  ])
  assert.equal(
    readFileSync(join(f.output, 'MooMole-0.2.1-mac-arm64.dmg'), 'utf8'),
    'second validated image',
  )
  assert.deepEqual(readdirSync(f.root).sort(), ['local-artifacts', 'stage'])
})
test('failed builds, changed deliveries and unknown user files preserve the current package', (t) => {
  const f = fixture(t)
  publishLocalArtifacts(f.stage, f.output, f.names)
  assert.throws(() =>
    publishLocalArtifacts(f.stage, f.output, ['validation.json', 'MooMole-9-mac-arm64.dmg']),
  )
  assert.equal(readFileSync(join(f.output, f.names[1]), 'utf8'), 'first validated image')
  writeFileSync(join(f.output, 'notes.txt'), 'keep user note')
  assert.throws(() => publishLocalArtifacts(f.stage, f.output, f.names), /ownership changed/)
  assert.equal(readFileSync(join(f.output, 'notes.txt'), 'utf8'), 'keep user note')
  rmSync(join(f.output, 'notes.txt'))
  writeFileSync(join(f.output, f.names[1]), 'local edits')
  assert.throws(() => publishLocalArtifacts(f.stage, f.output, f.names), /delivery changed/)
  assert.equal(readFileSync(join(f.output, f.names[1]), 'utf8'), 'local edits')
})
test('symlink outputs and stage links cannot redirect replacement or cleanup', (t) => {
  const f = fixture(t),
    outside = join(f.root, 'outside')
  mkdirSync(outside)
  writeFileSync(join(outside, 'keep.txt'), 'keep')
  symlinkSync(outside, f.output)
  assert.throws(() => publishLocalArtifacts(f.stage, f.output, f.names), /physical/)
  assert.equal(readFileSync(join(outside, 'keep.txt'), 'utf8'), 'keep')
  rmSync(f.output)
  symlinkSync(join(outside, 'keep.txt'), join(f.stage, 'MooMole-9-mac-arm64.dmg'))
  assert.throws(
    () => publishLocalArtifacts(f.stage, f.output, ['validation.json', 'MooMole-9-mac-arm64.dmg']),
    /Unsupported/,
  )
  assert.equal(existsSync(f.output), false)
  assert.equal(
    readdirSync(f.root).some((n) => n.startsWith('.MooMole-publish-')),
    false,
  )
})
