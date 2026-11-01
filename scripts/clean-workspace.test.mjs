import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { cleanWorkspace } from './clean-workspace.mjs'
function fixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'moomole-clean-test-')))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  writeFileSync(join(root, 'package.json'), '{"name":"moomole"}')
  function file(name, data = 'keep') {
    const path = join(root, name)
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, data)
  }
  return { root, file }
}
test('cleanup previews, then removes only generated cache while retaining sources, deliveries, tools and compact evidence', (t) => {
  const f = fixture(t)
  const keep = [
    'src/unfinished.rs',
    'local-artifacts/latest.dmg',
    '.git/config',
    'node_modules/pkg/a',
    '.cache/cargo/bin/rustc',
    '.cache/mole-source/mole',
  ]
  keep.forEach((name) => f.file(name))
  f.file('src-tauri/target/debug/old-build', 'discard')
  f.file('.cache/probe-old/sample', 'discard')
  f.file('.cache/example-native.json', '{"actualOperation":true}')
  f.file('.cache/old.log', 'old log')
  const preview = cleanWorkspace(f.root)
  assert.equal(preview.applied, false)
  assert.ok(existsSync(join(f.root, 'src-tauri/target')))
  const result = cleanWorkspace(f.root, { apply: true })
  assert.equal(result.applied, true)
  for (const name of keep) assert.equal(readFileSync(join(f.root, name), 'utf8'), 'keep')
  assert.equal(existsSync(join(f.root, 'src-tauri/target')), false)
  assert.equal(existsSync(join(f.root, '.cache/probe-old')), false)
  assert.equal(
    JSON.parse(readFileSync(join(f.root, '.cache/verification/native-evidence.json'), 'utf8'))
      .entries['example-native.json'].data.actualOperation,
    true,
  )
  assert.equal(cleanWorkspace(f.root, { apply: true }).paths, 0)
})
test('symlinked temporary entries are unlinked without following their targets; redirected cache roots are refused', (t) => {
  const f = fixture(t)
  f.file('outside/keep.txt')
  mkdirSync(join(f.root, '.cache'))
  symlinkSync(join(f.root, 'outside'), join(f.root, '.cache/old-fixture'))
  cleanWorkspace(f.root, { apply: true })
  assert.equal(readFileSync(join(f.root, 'outside/keep.txt'), 'utf8'), 'keep')
  rmSync(join(f.root, '.cache'), { recursive: true })
  symlinkSync(join(f.root, 'outside'), join(f.root, '.cache'))
  assert.throws(() => cleanWorkspace(f.root, { apply: true }), /redirected/)
  assert.equal(readFileSync(join(f.root, 'outside/keep.txt'), 'utf8'), 'keep')
})
test('a redirected build parent or verification directory cannot remove or overwrite outside files', (t) => {
  const f = fixture(t)
  f.file('outside/target/keep.txt')
  symlinkSync(join(f.root, 'outside'), join(f.root, 'src-tauri'))
  assert.throws(() => cleanWorkspace(f.root, { apply: true }), /Redirected parent/)
  assert.equal(readFileSync(join(f.root, 'outside/target/keep.txt'), 'utf8'), 'keep')
  rmSync(join(f.root, 'src-tauri'))
  mkdirSync(join(f.root, '.cache'))
  symlinkSync(join(f.root, 'outside'), join(f.root, '.cache/verification'))
  f.file('.cache/remove.log', 'keep until validated')
  assert.throws(() => cleanWorkspace(f.root, { apply: true }), /Verification directory/)
  assert.equal(existsSync(join(f.root, '.cache/remove.log')), true)
})
