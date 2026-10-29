import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { checkVersion, syncVersion, validVersion, versionPlan } from './version.mjs'

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'moomole-version-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  mkdirSync(join(root, 'src-tauri'))
  mkdirSync(join(root, 'third-party'))
  const json = (value) => JSON.stringify(value, null, 2) + '\n'
  const initial = {
    'package.json': json({ name: 'moomole', version: '0.1.0' }),
    'package-lock.json': json({
      name: 'moomole',
      version: '0.1.0',
      lockfileVersion: 3,
      packages: {
        '': { name: 'moomole', version: '0.1.0', dependencies: { fixture: '0.1.0' } },
        'node_modules/fixture': { version: '0.1.0' },
      },
    }),
    'src-tauri/tauri.conf.json':
      '{\n  "version": "0.1.0",\n  "bundle": { "icon": ["a", "b"] }\n}\n',
    'src-tauri/Cargo.toml':
      '# fixture\n[package]\nname = "moomole"\nversion = "0.1.0" # keep comment\n\n[dependencies]\nversion = "0.1.0"\n',
    'src-tauri/Cargo.lock':
      '# lock\nversion = 4\n\n[[package]]\nname = "fixture"\nversion = "0.1.0"\n\n[[package]]\nname = "moomole"\nversion = "0.1.0"\ndependencies = ["fixture"]\n',
  }
  initial['third-party/npm-source-origins.json'] = json({
    schemaVersion: 1,
    purpose: 'npm-source-input-origins',
    packageLockSha256: createHash('sha256').update(initial['package-lock.json']).digest('hex'),
    packages: [{ name: 'fixture', version: '0.1.0' }],
  })
  const write = (name, text) => writeFileSync(join(root, name), text)
  const read = (name) => readFileSync(join(root, name), 'utf8')
  const snapshot = () => Object.fromEntries(Object.keys(initial).map((name) => [name, read(name)]))
  Object.entries(initial).forEach(([name, text]) => write(name, text))
  return { root, initial, read, write, snapshot }
}

test('version grammar accepts SemVer and rejects ambiguous, unsafe or oversized source versions', () => {
  for (const v of [
    '0.1.0',
    '1.2.3-beta.1',
    '1.2.3-0alpha',
    '1.2.3-0',
    '1.2.3+build.001',
    '1.2.3-rc.1+build.7',
  ])
    assert.ok(validVersion(v), v)
  for (const v of [
    null,
    123,
    '',
    'v1.2.3',
    '1.2',
    '01.2.3',
    '1.02.3',
    '1.2.03',
    '1.2.3-01',
    '1.2.3-a..b',
    '1.2.3+',
    '1.2.3\n',
    ' 1.2.3',
    '1.2.3+$(literal)',
    '1.2.3+' + 'x'.repeat(130),
  ])
    assert.equal(validVersion(v), false, String(v))
})

test('version checking is read-only and rejects drift in every mirror and mismatched release tags', (t) => {
  const f = fixture(t)
  assert.equal(checkVersion(f.root, 'v0.1.0'), '0.1.0')
  assert.deepEqual(f.snapshot(), f.initial)
  for (const file of Object.keys(f.initial).filter(
    (name) => !['package.json', 'third-party/npm-source-origins.json'].includes(name),
  )) {
    f.write(file, f.initial[file].replaceAll('0.1.0', '0.2.0'))
    const before = f.snapshot()
    assert.throws(() => checkVersion(f.root), /Version mismatch/)
    assert.deepEqual(f.snapshot(), before)
    f.write(file, f.initial[file])
  }
  const lock = JSON.parse(f.read('package-lock.json'))
  lock.packages[''].version = '0.2.0'
  f.write('package-lock.json', JSON.stringify(lock, null, 2) + '\n')
  assert.throws(() => checkVersion(f.root), /Version mismatch/)
  f.write('package-lock.json', f.initial['package-lock.json'])
  for (const tag of ['0.1.0', 'v0.2.0', 'v0.1.0-beta.1', 'v0.1.0\n', '--help'])
    assert.throws(() => checkVersion(f.root, tag), /Release tag/)
  assert.deepEqual(f.snapshot(), f.initial)
})

test('sync updates only application versions and preserves dependencies, comments, formatting and lockfile content', (t) => {
  const f = fixture(t)
  const next = '2.4.0-rc.1+build.7'
  f.write('package.json', f.initial['package.json'].replace('0.1.0', next))
  assert.equal(syncVersion(f.root), next)
  assert.equal(checkVersion(f.root, `v${next}`), next)
  assert.equal(
    JSON.parse(f.read('package-lock.json')).packages['node_modules/fixture'].version,
    '0.1.0',
  )
  assert.ok(f.read('package-lock.json').includes('"fixture": "0.1.0"'))
  const origins = JSON.parse(f.read('third-party/npm-source-origins.json'))
  assert.equal(
    origins.packageLockSha256,
    createHash('sha256').update(f.read('package-lock.json')).digest('hex'),
  )
  assert.deepEqual(origins.packages, [{ name: 'fixture', version: '0.1.0' }])
  assert.equal(
    f.read('src-tauri/tauri.conf.json'),
    f.initial['src-tauri/tauri.conf.json'].replace('0.1.0', next),
  )
  assert.equal(
    f.read('src-tauri/Cargo.toml'),
    f.initial['src-tauri/Cargo.toml'].replace('version = "0.1.0" #', `version = "${next}" #`),
  )
  assert.equal(
    f.read('src-tauri/Cargo.lock'),
    f.initial['src-tauri/Cargo.lock'].replace(
      'name = "moomole"\nversion = "0.1.0"',
      `name = "moomole"\nversion = "${next}"`,
    ),
  )
  const before = f.snapshot()
  syncVersion(f.root)
  assert.deepEqual(f.snapshot(), before)
})

test('all sync targets are validated before writes; missing or ambiguous package sections are refused', (t) => {
  const f = fixture(t)
  f.write('package.json', f.initial['package.json'].replace('0.1.0', '0.2.0'))
  for (const invalid of [
    [
      'src-tauri/Cargo.lock',
      f.initial['src-tauri/Cargo.lock'] + '\n[[package]]\nname = "moomole"\nversion = "0.1.0"\n',
    ],
    [
      'src-tauri/Cargo.lock',
      f.initial['src-tauri/Cargo.lock'].replace('name = "moomole"', 'name = "other"'),
    ],
    ['src-tauri/Cargo.lock', f.initial['src-tauri/Cargo.lock'] + 'source = "registry"\n'],
    [
      'src-tauri/Cargo.toml',
      f.initial['src-tauri/Cargo.toml'] + '\n[package]\nversion = "0.1.0"\n',
    ],
    [
      'src-tauri/Cargo.toml',
      f.initial['src-tauri/Cargo.toml'].replace(
        'version = "0.1.0" # keep comment',
        'version.workspace = true',
      ),
    ],
    ['src-tauri/tauri.conf.json', '{"version": false}'],
    [
      'package-lock.json',
      f.initial['package-lock.json'].replace('"lockfileVersion": 3', '"lockfileVersion": 9'),
    ],
  ]) {
    f.write(invalid[0], invalid[1])
    const before = f.snapshot()
    assert.throws(() => syncVersion(f.root))
    assert.deepEqual(f.snapshot(), before)
    f.write(invalid[0], f.initial[invalid[0]])
  }
})

test('invalid canonical source is rejected even when mirrors would otherwise be writable', (t) => {
  const f = fixture(t)
  for (const source of [
    { name: 'moomole', version: '01.0.0' },
    { name: 'other', version: '0.1.0' },
    { name: 'moomole' },
  ]) {
    f.write('package.json', JSON.stringify(source))
    const before = f.snapshot()
    assert.throws(() => versionPlan(f.root), /invalid MooMole/)
    assert.deepEqual(f.snapshot(), before)
  }
})

test('stale source bindings reject checks and sync without blessing dependency or lockfile edits', (t) => {
  const f = fixture(t)
  f.write('package.json', f.initial['package.json'].replace('0.1.0', '0.2.0'))
  for (const text of [
    f.initial['package-lock.json'].replace('"fixture": "0.1.0"', '"fixture": "0.2.0"'),
    f.initial['package-lock.json'] + '\n',
  ]) {
    f.write('package-lock.json', text)
    const before = f.snapshot()
    assert.throws(() => checkVersion(f.root), /SHA-256 mismatch/)
    assert.throws(() => syncVersion(f.root), /binding is stale/)
    assert.deepEqual(f.snapshot(), before)
  }
  f.write('package-lock.json', f.initial['package-lock.json'])
  f.write(
    'third-party/npm-source-origins.json',
    f.initial['third-party/npm-source-origins.json'].replace(
      /"[a-f0-9]{64}"/,
      `"${'0'.repeat(64)}"`,
    ),
  )
  const before = f.snapshot()
  assert.throws(() => syncVersion(f.root), /binding is stale/)
  assert.deepEqual(f.snapshot(), before)
})
