import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createHash } from 'node:crypto'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { collectNotices, publishNotices, verifyNotices } from './dependency-notices.mjs'

const sha = (b) => createHash('sha256').update(b).digest('hex')
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'moomole-notices-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const put = (path, text) => {
    mkdirSync(dirname(join(root, path)), { recursive: true })
    writeFileSync(join(root, path), text)
  }
  for (const [p, text] of Object.entries({
    LICENSE: 'MooMole fixture\n',
    'third-party/NOTICE.md': 'Unmodified upstream fixture\n',
    'src-tauri/resources/mole/LICENSE': 'Mole fixture\n',
    'src-tauri/resources/mole/TRADEMARK.md': 'Upstream policy fixture\n',
    'node_modules/example/LICENSE': 'npm original attribution\n',
    'cargo/library/LICENSE': 'cargo original attribution\n',
    'go-module/LICENSE': 'Go module original attribution\n',
    'go/VERSION': 'go1.26.0\n',
    'go/LICENSE': 'Go runtime attribution\n',
    'rust/COPYRIGHT-library.html': '<p>Rust library original attribution</p>\n',
    'rust/licenses/MIT.txt': 'Canonical fixture\n',
  }))
    put(p, text)
  put(
    'node_modules/example/package.json',
    JSON.stringify({ name: 'example', version: '1.0.0', license: 'MIT' }),
  )
  put(
    'package-lock.json',
    JSON.stringify({
      packages: {
        '': {},
        'node_modules/example': { version: '1.0.0', license: 'MIT', integrity: 'sha512-fixture' },
        'node_modules/dev': { dev: true },
      },
    }),
  )
  const sum = 'h1:' + createHash('sha256').update('fixture').digest('base64')
  const input = {
    root,
    engine: { appVersion: '0.1.0', version: '1.59.1', revision: 'a'.repeat(40), arch: 'arm64' },
    cargo: {
      resolve: {
        root: 'app',
        nodes: [
          {
            id: 'app',
            deps: [
              { pkg: 'library', dep_kinds: [{ kind: null }] },
              { pkg: 'dev', dep_kinds: [{ kind: 'dev' }] },
            ],
          },
          { id: 'library', deps: [] },
          { id: 'dev', deps: [] },
        ],
      },
      packages: [
        { id: 'app' },
        {
          id: 'library',
          name: 'library',
          version: '1.0.0',
          source: 'registry+https://github.com/rust-lang/crates.io-index',
          manifest_path: join(root, 'cargo/library/Cargo.toml'),
          license: 'MIT',
          authors: ['Declared fixture author'],
        },
        { id: 'dev', name: 'dev' },
      ],
    },
    goBuilds: [
      {
        Main: { Path: 'github.com/tw93/mole' },
        GoVersion: 'go1.26.0',
        Deps: [{ Path: 'example.invalid/module', Version: 'v1.0.0', Sum: sum }],
      },
    ],
    goModules: new Map([
      ['example.invalid/module@v1.0.0', { Dir: join(root, 'go-module'), Sum: sum }],
    ]),
    rustRoot: join(root, 'rust'),
    rustVersion: '1.99.0',
    goRoot: join(root, 'go'),
    supplements: { version: 1, entries: [] },
  }
  const folder = join(root, 'generated'),
    expected = { version: '0.1.0', revision: 'a'.repeat(40), arch: 'arm64' }
  return { root, put, input, folder, expected, collect: () => collectNotices(input) }
}
test('selected dependency scopes retain original text, deduplicate materials and publish a self-contained book without host paths', (t) => {
  const f = fixture(t),
    r = f.collect()
  assert.equal(r.manifest.packages.length, 7)
  assert.ok(!r.manifest.packages.some((p) => p.id.includes('dev')))
  assert.ok(!JSON.stringify(r.manifest).includes(f.root))
  assert.ok(r.bytes.includes(Buffer.from('cargo original attribution\n')))
  assert.equal(r.manifest.reviewStatus, 'pending')
  publishNotices(f.folder, r)
  assert.deepEqual(verifyNotices(f.folder, f.expected), r.manifest)
  publishNotices(f.folder, f.collect())
  assert.equal(
    sha(readFileSync(join(f.folder, 'THIRD_PARTY_NOTICES.txt'))),
    r.manifest.notices.sha256,
  )
})
test('missing material or a linked license is refused without reading unrelated private data', (t) => {
  const f = fixture(t)
  rmSync(join(f.root, 'cargo/library/LICENSE'))
  assert.throws(f.collect, /Missing license material/)
  f.put('private', 'private sentinel')
  symlinkSync(join(f.root, 'private'), join(f.root, 'cargo/library/LICENSE'))
  assert.throws(f.collect, /links require/)
  assert.equal(existsSync(f.folder), false)
  const g = fixture(t)
  rmSync(join(g.root, 'node_modules/example/LICENSE'))
  g.put('node_modules/example/LICENSE.spdx', 'PackageLicenseDeclared: MIT\n')
  assert.throws(g.collect, /Missing license text/)
})
test('supplements bind exact source and hashes; policy/reference material remains explicitly pending review', (t) => {
  const f = fixture(t),
    text = Buffer.from('Original policy, unchanged\n'),
    h = sha(text)
  rmSync(join(f.root, 'cargo/library/LICENSE'))
  f.put('cargo/library/.cargo_vcs_info.json', JSON.stringify({ git: { sha1: 'b'.repeat(40) } }))
  f.put('third-party/license-supplements/' + h + '.txt', text)
  const e = {
    id: 'cargo:library@1.0.0',
    declaredLicense: 'MIT',
    vcs: 'b'.repeat(40),
    reviewRequired: true,
    files: [
      {
        file: h + '.txt',
        sha256: h,
        size: text.length,
        source: 'https://example.invalid/pinned/LICENSE',
        role: 'policy',
      },
    ],
  }
  f.input.supplements.entries.push(e)
  const r = f.collect()
  assert.deepEqual(r.manifest.reviewRequired, ['cargo:library@1.0.0'])
  assert.ok(r.bytes.includes(text))
  e.vcs = 'c'.repeat(40)
  assert.throws(f.collect, /no longer matches/)
  e.vcs = 'b'.repeat(40)
  f.put('third-party/license-supplements/' + h + '.txt', 'changed')
  assert.throws(f.collect, /contents changed/)
})
test('npm version/license drift, Go replacements/checksum conflicts and the wrong runtime are refused', (t) => {
  for (const kind of ['npm', 'replacement', 'checksum', 'runtime']) {
    const f = fixture(t)
    if (kind === 'npm')
      f.put(
        'node_modules/example/package.json',
        JSON.stringify({ name: 'example', version: '2.0.0', license: 'MIT' }),
      )
    if (kind === 'replacement') f.input.goBuilds[0].Deps[0].Replace = { Path: 'local' }
    if (kind === 'checksum')
      f.input.goModules.get('example.invalid/module@v1.0.0').Sum = 'different'
    if (kind === 'runtime') f.put('go/VERSION', 'go1.27.0\n')
    assert.throws(f.collect)
  }
})
test('individual material hashes and review coverage cannot be hidden by renewing only the whole-file digest', (t) => {
  for (const kind of ['text', 'coverage', 'review', 'identity']) {
    const f = fixture(t)
    publishNotices(f.folder, f.collect())
    const p = join(f.folder, 'THIRD_PARTY_NOTICES.json'),
      m = JSON.parse(readFileSync(p))
    if (kind === 'text') {
      const b = readFileSync(join(f.folder, 'THIRD_PARTY_NOTICES.txt'))
      b[m.materials[0].offset] ^= 1
      writeFileSync(join(f.folder, 'THIRD_PARTY_NOTICES.txt'), b)
      m.notices.sha256 = sha(b)
    }
    if (kind === 'coverage') m.packages[0].materials[0].sha256 = '0'.repeat(64)
    if (kind === 'review') m.reviewStatus = 'complete'
    if (kind === 'identity') m.appVersion = '0.2.0'
    writeFileSync(p, JSON.stringify(m))
    assert.throws(() => verifyNotices(f.folder, f.expected))
  }
})
test('publishing refuses unknown existing files and preserves previous generated evidence on refusal', (t) => {
  const f = fixture(t),
    r = f.collect()
  publishNotices(f.folder, r)
  const before = readFileSync(join(f.folder, 'THIRD_PARTY_NOTICES.json'))
  writeFileSync(join(f.folder, 'do-not-remove'), 'keep')
  assert.throws(() => publishNotices(f.folder, r), /Unexpected notices files/)
  assert.deepEqual(readFileSync(join(f.folder, 'THIRD_PARTY_NOTICES.json')), before)
  assert.equal(readFileSync(join(f.folder, 'do-not-remove'), 'utf8'), 'keep')
})
