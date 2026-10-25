import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createHash } from 'node:crypto'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
  existsSync,
  symlinkSync,
  linkSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { stageCandidate, sealCandidate } from './candidate-artifacts.mjs'

// Metadata/transport fixtures only: these bytes are not an App, DMG or license.
// Native validation remains in build-local-app and the real candidate CI run.
const digest = (body) => createHash('sha512').update(body).digest('hex')
const json = (v) => JSON.stringify(v, null, 2) + '\n'
function fixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'moomole-candidate-')))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const build = join(root, 'build'),
    output = join(root, 'output')
  mkdirSync(build)
  const expected = {
    version: '0.1.0',
    arch: 'arm64',
    revision: 'a'.repeat(40),
    engineRevision: 'b'.repeat(40),
    engineVersion: '1.59.1',
  }
  function asset(folder, file, body) {
    writeFileSync(join(folder, file), body)
    return { file, size: Buffer.byteLength(body), sha512: digest(body), sha512Encoding: 'hex' }
  }
  const archive = asset(build, 'MooMole-0.1.0-mac-arm64.app.zip', 'fixture archive')
  const image = asset(build, 'MooMole-0.1.0-mac-arm64.dmg', 'fixture image')
  const app = {
    version: expected.version,
    architecture: expected.arch,
    identifier: 'io.github.rememberber.moomole',
    bytes: 100,
    signing: 'ad-hoc',
    notarized: false,
    engineRevision: expected.engineRevision,
    engineVersion: expected.engineVersion,
    dependencyMaterials: {
      packages: 3,
      materials: 1,
      reviewStatus: 'pending',
      reviewRequired: 1,
      noticesSha256: 'c'.repeat(64),
    },
  }
  const report = {
    schemaVersion: 1,
    purpose: 'local-validation-only',
    profile: 'release',
    source: { revision: expected.revision, dirty: false },
    ...app,
    archive,
    diskImage: {
      ...image,
      format: 'UDZO',
      filesystem: 'HFS+',
      readOnly: true,
      validation: 'mounted-and-detached',
      appFingerprint: 'd'.repeat(128),
      appFingerprintEncoding: 'sha512-hex',
      app,
    },
    installationValidation: 'pending',
    gatekeeperValidation: 'pending',
  }
  const reportPath = join(build, 'validation.json')
  const save = () => writeFileSync(reportPath, json(report))
  save()
  function stage() {
    return stageCandidate({ reportPath, output, expected })
  }
  function packets() {
    const src = join(output, 'source'),
      deps = join(output, 'dependencies')
    mkdirSync(src)
    mkdirSync(deps)
    const binding = {
      reportSha512: digest(readFileSync(join(output, 'validation.json'))),
      architecture: expected.arch,
      sourceRevision: expected.revision,
      archive: { file: archive.file, size: archive.size, sha512: archive.sha512 },
    }
    const appSource = {
      ...asset(src, 'MooMole-0.1.0-source.tar.gz', 'fixture client'),
      revision: expected.revision,
    }
    const moleSource = {
      ...asset(src, 'Mole-1.59.1-bbbbbbbbbbbb-source.tar.gz', 'fixture engine'),
      revision: expected.engineRevision,
      version: expected.engineVersion,
    }
    const source = {
      schemaVersion: 1,
      productId: 'io.github.rememberber.moomole',
      purpose: 'source-archive-validation',
      version: expected.version,
      digestEncoding: 'hex',
      binaryBinding: binding,
      app: appSource,
      mole: moleSource,
      releaseAcceptance: 'pending',
    }
    const dependency = {
      schemaVersion: 1,
      ...asset(deps, 'MooMole-0.1.0-mac-arm64-dependencies.tar.gz', 'fixture dependency'),
      status: 'verified',
      source: report.source,
      binaryBinding: { ...binding, noticesSha256: app.dependencyMaterials.noticesSha256 },
      records: 1,
      reviewStatus: 'pending',
    }
    const savePackets = () => {
      writeFileSync(join(src, 'SOURCE.json'), json(source))
      writeFileSync(join(deps, 'validation.json'), json(dependency))
    }
    writeFileSync(join(src, 'BUILD.md'), 'fixture instructions')
    savePackets()
    return { source, dependency, savePackets }
  }
  return {
    root,
    build,
    output,
    expected,
    report,
    reportPath,
    save,
    stage,
    packets,
    seal: () => sealCandidate({ output, expected }),
  }
}

test('stages exact bytes and seals only matching App/source/dependency packets with pending acceptance', (t) => {
  const f = fixture(t)
  f.stage()
  f.packets()
  assert.equal(
    readFileSync(join(f.output, 'validation.json'), 'utf8'),
    readFileSync(f.reportPath, 'utf8'),
  )
  const result = f.seal()
  assert.equal(result.releaseAcceptance, 'pending')
  assert.equal(result.licenseReview, 'pending')
  assert.equal(result.notarized, false)
  assert.equal(result.payload.length, 9)
  const sums = readFileSync(join(f.output, 'SHA512SUMS'), 'utf8').trim().split('\n')
  assert.equal(sums.length, 10)
  for (const line of sums) {
    const [hash, file] = line.split('  ')
    assert.equal(hash, digest(readFileSync(join(f.output, file))))
  }
  assert.throws(() => f.seal(), /Unknown or missing/)
})

test('refuses stale, dirty, wrong-architecture, unsigned, unvalidated or mixed build metadata before creating output', (t) => {
  const changes = [
    (r) => (r.source.dirty = true),
    (r) => (r.source.revision = 'e'.repeat(40)),
    (r) => (r.architecture = 'x64'),
    (r) => (r.version = '0.2.0'),
    (r) => (r.profile = 'debug'),
    (r) => (r.engineRevision = 'e'.repeat(40)),
    (r) => (r.engineVersion = '1.60.0'),
    (r) => (r.signing = 'none'),
    (r) => (r.notarized = true),
    (r) => (r.installationValidation = 'complete'),
    (r) => (r.gatekeeperValidation = 'complete'),
    (r) => (r.dependencyMaterials.reviewStatus = 'complete'),
    (r) => (r.archive.file = '../private.sqlite'),
    (r) => (r.archive.sha512 = '0'.repeat(128)),
    (r) => (r.archive.sha512Encoding = 'base64'),
    (r) => (r.diskImage.validation = 'pending'),
    (r) => (r.diskImage.readOnly = false),
    (r) => (r.diskImage.app.architecture = 'x64'),
  ]
  for (const change of changes) {
    const f = fixture(t)
    change(f.report)
    f.save()
    assert.throws(() => f.stage())
    assert.equal(existsSync(f.output), false)
  }
})

test('refuses replaced, linked, shared or oversized inputs and preserves existing output', (t) => {
  for (const kind of ['bytes', 'symlink', 'hardlink', 'parentLink', 'largeReport', 'existing']) {
    const f = fixture(t),
      file = join(f.build, f.report.archive.file)
    if (kind === 'bytes') writeFileSync(file, 'replaced')
    if (kind === 'symlink') {
      rmSync(file)
      symlinkSync(f.reportPath, file)
    }
    if (kind === 'hardlink') linkSync(file, join(f.root, 'shared'))
    if (kind === 'largeReport') writeFileSync(f.reportPath, ' '.repeat(4 * 1024 ** 2 + 1))
    if (kind === 'existing') {
      mkdirSync(f.output)
      writeFileSync(join(f.output, 'keep'), 'original')
    }
    if (kind === 'parentLink') {
      symlinkSync(f.build, join(f.root, 'alias'))
      assert.throws(
        () =>
          stageCandidate({
            reportPath: join(f.root, 'alias/validation.json'),
            output: f.output,
            expected: f.expected,
          }),
        /links/,
      )
    } else assert.throws(() => f.stage())
    if (kind === 'existing') assert.equal(readFileSync(join(f.output, 'keep'), 'utf8'), 'original')
    assert.equal(existsSync(join(f.output, 'validation.json')), false)
  }
})

test('sealing refuses cross-build packets, changed assets, missing or unrelated payloads without blessing partial output', (t) => {
  for (const kind of [
    'sourceBinding',
    'depBinding',
    'revision',
    'count',
    'review',
    'asset',
    'unknown',
    'missing',
    'link',
  ]) {
    const f = fixture(t)
    f.stage()
    const p = f.packets()
    if (kind === 'sourceBinding') p.source.binaryBinding.reportSha512 = 'f'.repeat(128)
    if (kind === 'depBinding') p.dependency.binaryBinding.noticesSha256 = 'f'.repeat(64)
    if (kind === 'revision') p.source.app.revision = 'f'.repeat(40)
    if (kind === 'count') p.dependency.records = 2
    if (kind === 'review') p.dependency.reviewStatus = 'complete'
    p.savePackets()
    if (kind === 'asset') writeFileSync(join(f.output, f.report.diskImage.file), 'changed image')
    if (kind === 'unknown') writeFileSync(join(f.output, 'unrelated.sqlite'), 'do not ship')
    if (kind === 'missing') rmSync(join(f.output, 'source/BUILD.md'))
    if (kind === 'link') {
      rmSync(join(f.output, 'source/BUILD.md'))
      symlinkSync(f.reportPath, join(f.output, 'source/BUILD.md'))
    }
    assert.throws(() => f.seal(), undefined, kind)
    assert.equal(existsSync(join(f.output, 'CANDIDATE.json')), false, kind)
    assert.equal(existsSync(join(f.output, 'SHA512SUMS')), false, kind)
  }
})
