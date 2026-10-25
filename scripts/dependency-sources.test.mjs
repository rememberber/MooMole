import assert from 'node:assert/strict'
import { test } from 'node:test'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  existsSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  cargoChecksums,
  dependencyAppBinding,
  writeDependencyPacket,
} from './dependency-sources.mjs'
import { toolchainEnv } from './env.mjs'

const digest = (bytes, algorithm = 'sha256', encoding = 'hex') =>
  createHash(algorithm).update(bytes).digest(encoding)
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'moomole-dep-sources-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const crate = Buffer.from('public source archive fixture'),
    npm = Buffer.from('public npm distribution fixture')
  writeFileSync(join(root, 'crate'), crate)
  writeFileSync(join(root, 'npm'), npm)
  return {
    root,
    output: join(root, 'output'),
    metadata: {
      version: '0.1.0',
      arch: 'arm64',
      engineRevision: 'a'.repeat(40),
      engineVersion: '1.59.1',
      noticesSha256: 'c'.repeat(64),
      expectedIds: ['cargo:example@1.0.0', 'npm:example@1.0.0'],
      source: { revision: 'b'.repeat(40), dirty: false },
    },
    inputs: [
      {
        id: 'cargo:example@1.0.0',
        kind: 'cargo-published-crate',
        file: 'example-1.0.0.crate',
        path: join(root, 'crate'),
        origin: 'https://crates.io/crates/example/1.0.0',
        verification: { algorithm: 'sha256', value: digest(crate) },
      },
      {
        id: 'npm:example@1.0.0',
        kind: 'npm-published-distribution',
        file: 'example-1.0.0.tgz',
        path: join(root, 'npm'),
        origin: 'https://registry.npmjs.org/example/-/example-1.0.0.tgz',
        verification: {
          algorithm: 'npm-sha512',
          value: 'sha512-' + digest(npm, 'sha512', 'base64'),
        },
      },
    ],
  }
}
function appFixture(f) {
  const file = `MooMole-${f.metadata.version}-mac-${f.metadata.arch}.app.zip`,
    bytes = Buffer.from('owned app archive fixture')
  const report = {
    schemaVersion: 1,
    profile: 'release',
    identifier: 'io.github.rememberber.moomole',
    source: { ...f.metadata.source },
    version: f.metadata.version,
    architecture: f.metadata.arch,
    engineRevision: f.metadata.engineRevision,
    engineVersion: f.metadata.engineVersion,
    dependencyMaterials: {
      noticesSha256: f.metadata.noticesSha256,
      packages: f.metadata.expectedIds.length + 2,
    },
    archive: { file, size: bytes.length, sha512: digest(bytes, 'sha512'), sha512Encoding: 'hex' },
  }
  f.appReport = join(f.root, 'app-validation.json')
  const archive = join(f.root, file)
  writeFileSync(archive, bytes)
  writeFileSync(f.appReport, JSON.stringify(report))
  return { report, archive }
}
test('registry checksums are unique and literal; ambiguous, private or unsupported lock sources are refused', () => {
  const source = 'registry+https://github.com/rust-lang/crates.io-index'
  const entry = `[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "${source}"\nchecksum = "${'a'.repeat(64)}"\n`
  assert.equal(
    cargoChecksums('version = 4\n' + entry).get('example@1.0.0|' + source),
    'a'.repeat(64),
  )
  for (const value of [
    'version = 9\n' + entry,
    'version = 4\n' + entry + entry,
    ('version = 4\n' + entry).replace(source, 'git+https://example.invalid/private'),
    ('version = 4\n' + entry).replace('a'.repeat(64), 'bad'),
  ])
    assert.throws(() => cargoChecksums(value))
})
test('published artifacts are copied byte-for-byte, mapped to the complete inventory and rechecked after outer extraction', (t) => {
  const f = fixture(t),
    result = writeDependencyPacket(f)
  assert.equal(result.report.records, 2)
  assert.equal(result.manifest.preferredSourceReview, 'pending')
  assert.equal(result.manifest.binaryBinding, null)
  assert.ok(!JSON.stringify(result.manifest).includes(f.root))
  const file = join(f.output, result.report.file),
    bytes = readFileSync(file)
  assert.equal(digest(bytes, 'sha512'), result.report.sha512)
  assert.equal(bytes.length, result.report.size)
})
test('optional binding records the matching clean App bytes and notices in both manifests without local paths', (t) => {
  const f = fixture(t),
    { archive } = appFixture(f),
    result = writeDependencyPacket(f)
  assert.equal(result.manifest.binaryBinding.sourceRevision, f.metadata.source.revision)
  assert.equal(
    result.manifest.binaryBinding.archive.sha512,
    digest(readFileSync(archive), 'sha512'),
  )
  assert.equal(result.manifest.binaryBinding.noticesSha256, f.metadata.noticesSha256)
  assert.deepEqual(result.report.binaryBinding, result.manifest.binaryBinding)
  assert.equal(result.report.reviewStatus, 'pending')
  assert.ok(!JSON.stringify(result).includes(f.root))
})
test('dirty, mismatched, unsafe, oversized or changed App inputs cannot produce a bound packet', (t) => {
  for (const kind of [
    'dirtySource',
    'dirtyApp',
    'revision',
    'version',
    'arch',
    'engine',
    'engineVersion',
    'notices',
    'packages',
    'product',
    'profile',
    'filename',
    'size',
    'digest',
    'missing',
    'archiveLink',
    'archiveHardlink',
    'reportLink',
    'reportHardlink',
    'oversized',
  ]) {
    const f = fixture(t),
      { report, archive } = appFixture(f)
    if (kind === 'dirtySource') f.metadata.source.dirty = true
    if (kind === 'dirtyApp') report.source.dirty = true
    if (kind === 'revision') report.source.revision = 'd'.repeat(40)
    if (kind === 'version') report.version = '9.0.0'
    if (kind === 'arch') report.architecture = 'x64'
    if (kind === 'engine') report.engineRevision = 'd'.repeat(40)
    if (kind === 'engineVersion') report.engineVersion = '9.0.0'
    if (kind === 'notices') report.dependencyMaterials.noticesSha256 = 'd'.repeat(64)
    if (kind === 'packages') report.dependencyMaterials.packages++
    if (kind === 'product') report.identifier = 'another.product'
    if (kind === 'profile') report.profile = 'debug'
    if (kind === 'filename') report.archive.file = '../escape.zip'
    if (kind === 'size') report.archive.size++
    if (kind === 'digest') writeFileSync(archive, Buffer.alloc(report.archive.size))
    if (kind === 'missing') rmSync(archive)
    if (kind === 'archiveLink') {
      const saved = join(f.root, 'saved')
      writeFileSync(saved, readFileSync(archive))
      rmSync(archive)
      symlinkSync(saved, archive)
    }
    if (kind === 'archiveHardlink') linkSync(archive, join(f.root, 'linked'))
    writeFileSync(f.appReport, JSON.stringify(report))
    if (kind === 'reportLink') {
      const saved = join(f.root, 'saved-report')
      writeFileSync(saved, readFileSync(f.appReport))
      rmSync(f.appReport)
      symlinkSync(saved, f.appReport)
    }
    if (kind === 'reportHardlink') linkSync(f.appReport, join(f.root, 'linked-report'))
    if (kind === 'oversized') writeFileSync(f.appReport, Buffer.alloc(1024 ** 2 + 1))
    assert.throws(() => writeDependencyPacket(f), undefined, kind)
    assert.equal(existsSync(f.output), false, kind)
  }
})
test('App bytes changed during dependency collection withdraw the complete output', (t) => {
  const f = fixture(t),
    { archive } = appFixture(f)
  f.inputs[0] = {
    ...f.inputs[0],
    id: 'go:example.invalid/module@v1.0.0',
    kind: 'go-published-module',
    module: 'example.invalid/module@v1.0.0',
    origin: 'https://proxy.golang.org/example.invalid/module/@v/v1.0.0.zip',
    verification: { algorithm: 'go-h1', value: 'h1:fixture' },
  }
  f.metadata.expectedIds[0] = f.inputs[0].id
  f.verifyGo = () => {
    writeFileSync(archive, Buffer.alloc(readFileSync(archive).length))
    return { version: 1, sum: 'h1:fixture', files: 1, bytes: 4 }
  }
  assert.throws(() => writeDependencyPacket(f), /App archive digest/)
  assert.equal(existsSync(f.output), false)
  assert.equal(dependencyAppBinding(null, f.metadata), null)
})
test('source tampering, incomplete inventory and unsafe output filenames never leave a successful packet', (t) => {
  for (const kind of ['cargo', 'npm', 'missing', 'duplicate', 'filename', 'kind', 'origin']) {
    const f = fixture(t)
    if (kind === 'cargo') writeFileSync(join(f.root, 'crate'), 'changed')
    if (kind === 'npm') f.inputs[1].verification.value = 'sha512-wrong'
    if (kind === 'missing') f.metadata.expectedIds.push('cargo:missing@1.0.0')
    if (kind === 'duplicate') f.inputs.push({ ...f.inputs[0] })
    if (kind === 'filename') f.inputs[0].file = '../escape'
    if (kind === 'kind') f.inputs[0].kind = 'npm-published-distribution'
    if (kind === 'origin') f.inputs[0].origin = 'https://example.invalid/private'
    assert.throws(() => writeDependencyPacket(f))
    assert.equal(existsSync(f.output), false)
  }
})
test('existing directories are preserved and a Go proof must match its exact expected content hash', (t) => {
  const f = fixture(t)
  mkdirSync(f.output)
  writeFileSync(join(f.output, 'keep'), 'original')
  assert.throws(() => writeDependencyPacket(f))
  assert.equal(readFileSync(join(f.output, 'keep'), 'utf8'), 'original')
  const g = fixture(t)
  g.inputs[0].id = 'go:example.invalid/module@v1.0.0'
  g.metadata.expectedIds[0] = g.inputs[0].id
  g.inputs[0].kind = 'go-published-module'
  g.inputs[0].origin = 'https://proxy.golang.org/example.invalid/module/@v/v1.0.0.zip'
  g.inputs[0].verification = { algorithm: 'go-h1', value: 'h1:fixture' }
  g.inputs[0].module = 'example.invalid/module@v1.0.0'
  assert.throws(
    () =>
      writeDependencyPacket({
        ...g,
        verifyGo: () => ({ version: 1, sum: 'h1:other', files: 1, bytes: 4 }),
      }),
    /proof/,
  )
  assert.equal(existsSync(g.output), false)
})
test(
  'native Go verifier checks sorted content hashes and rejects escaping or duplicate zip entries',
  { skip: process.platform !== 'darwin' },
  () => {
    const r = spawnSync(
      'go',
      ['test', 'scripts/verify-go-module.go', 'scripts/verify-go-module_test.go'],
      {
        cwd: new URL('../', import.meta.url),
        env: toolchainEnv(),
        encoding: 'utf8',
        timeout: 60000,
      },
    )
    assert.equal(r.status, 0, r.error?.message ?? r.stderr)
  },
)
