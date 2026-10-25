// Assemble already-validated build outputs. This is not a signing authority or
// release approval; build-local-app performs native signature/ZIP/DMG checks.
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  constants,
  copyFileSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isDeepStrictEqual } from 'node:util'
import { fileDigest } from './app-artifact.mjs'
import { revision as engineRevision, sourceVersion } from './engine-manifest.mjs'
import { checkVersion } from './version.mjs'

const hash = (bytes) => createHash('sha512').update(bytes).digest('hex')
function physical(path, directory = false) {
  path = resolve(path)
  let current = path
  for (;;) {
    const meta = lstatSync(current)
    if (meta.isSymbolicLink()) throw Error('Candidate paths must not contain links')
    if (current === path) {
      if (directory ? !meta.isDirectory() : !meta.isFile() || meta.nlink !== 1)
        throw Error('Candidate inputs require private regular files and physical directories')
    } else if (!meta.isDirectory()) throw Error('Candidate parent is not a directory')
    const parent = dirname(current)
    if (parent === current) break
    current = parent
  }
  return path
}
function readMetadata(path) {
  physical(path)
  const before = lstatSync(path)
  if (before.size > 4 * 1024 ** 2) throw Error('Candidate metadata exceeds budget')
  const bytes = readFileSync(path),
    after = lstatSync(path)
  if (
    before.dev !== after.dev ||
    before.ino !== after.ino ||
    before.size !== after.size ||
    before.mtimeMs !== after.mtimeMs ||
    bytes.length !== before.size
  )
    throw Error('Candidate metadata changed while reading')
  return bytes
}
const readJson = (path) => JSON.parse(readMetadata(path))
function checkedFile(root, record, name) {
  if (
    record?.file !== name ||
    !Number.isSafeInteger(record.size) ||
    record.size <= 0 ||
    record.size > 256 * 1024 ** 2 ||
    !/^[a-f0-9]{128}$/.test(record.sha512 ?? '')
  )
    throw Error('Invalid candidate asset metadata or filename')
  const path = physical(join(root, name))
  const before = lstatSync(path)
  if (before.size !== record.size) throw Error('Candidate asset size differs from report')
  const observed = fileDigest(path)
  const after = lstatSync(path)
  if (
    before.dev !== after.dev ||
    before.ino !== after.ino ||
    before.mtimeMs !== after.mtimeMs ||
    before.size !== after.size ||
    observed.size !== record.size ||
    observed.sha512 !== record.sha512
  )
    throw Error('Candidate asset is missing, changed or has the wrong digest')
  return path
}
function validateReport(report, expected) {
  if (
    report.schemaVersion !== 1 ||
    report.purpose !== 'local-validation-only' ||
    report.profile !== 'release' ||
    report.signing !== 'ad-hoc' ||
    report.notarized !== false ||
    report.identifier !== 'io.github.rememberber.moomole' ||
    report.source?.dirty !== false ||
    !/^[a-f0-9]{40}$/.test(expected.revision ?? '') ||
    report.source.revision !== expected.revision ||
    report.version !== expected.version ||
    !['arm64', 'x64'].includes(expected.arch) ||
    report.architecture !== expected.arch ||
    report.engineRevision !== expected.engineRevision ||
    report.engineVersion !== expected.engineVersion ||
    report.installationValidation !== 'pending' ||
    report.gatekeeperValidation !== 'pending' ||
    report.dependencyMaterials?.reviewStatus !== 'pending' ||
    report.archive?.sha512Encoding !== 'hex' ||
    !/^[a-f0-9]{64}$/.test(report.dependencyMaterials?.noticesSha256 ?? '')
  )
    throw Error(
      'Candidate must match this clean release build, architecture, engine and pending acceptance',
    )
  const image = report.diskImage
  if (
    image?.format !== 'UDZO' ||
    image.filesystem !== 'HFS+' ||
    image.readOnly !== true ||
    image.sha512Encoding !== 'hex' ||
    image.validation !== 'mounted-and-detached' ||
    image.appFingerprintEncoding !== 'sha512-hex' ||
    !/^[a-f0-9]{128}$/.test(image.appFingerprint ?? '') ||
    !isDeepStrictEqual(
      image.app,
      Object.fromEntries(Object.keys(image.app ?? {}).map((key) => [key, report[key]])),
    ) ||
    image.app?.architecture !== expected.arch ||
    image.app?.version !== expected.version ||
    image.app?.signing !== 'ad-hoc' ||
    image.app?.notarized !== false ||
    !isDeepStrictEqual(image.app?.dependencyMaterials, report.dependencyMaterials)
  )
    throw Error('Candidate requires the matching validated DMG')
  return report
}
export function stageCandidate({ reportPath, output, expected }) {
  const original = readMetadata(reportPath)
  const report = validateReport(JSON.parse(original), expected)
  const assets = [
    [report.archive, `MooMole-${expected.version}-mac-${expected.arch}.app.zip`],
    [report.diskImage, `MooMole-${expected.version}-mac-${expected.arch}.dmg`],
  ]
  for (const [record, name] of assets) checkedFile(dirname(reportPath), record, name)
  physical(dirname(output), true)
  mkdirSync(output)
  for (const [record, name] of assets) {
    copyFileSync(join(dirname(reportPath), name), join(output, name), constants.COPYFILE_EXCL)
    checkedFile(output, record, name)
    checkedFile(dirname(reportPath), record, name)
  }
  if (!original.equals(readMetadata(reportPath))) throw Error('App report changed while staging')
  writeFileSync(join(output, 'validation.json'), original, { flag: 'wx' })
  return report
}
export function sealCandidate({ output, expected }) {
  physical(output, true)
  const reportPath = join(output, 'validation.json')
  const report = validateReport(readJson(reportPath), expected)
  const source = readJson(join(output, 'source/SOURCE.json'))
  const deps = readJson(join(output, 'dependencies/validation.json'))
  const binding = {
    reportSha512: hash(readFileSync(reportPath)),
    architecture: expected.arch,
    sourceRevision: expected.revision,
    archive: {
      file: report.archive.file,
      size: report.archive.size,
      sha512: report.archive.sha512,
    },
  }
  if (
    source.schemaVersion !== 1 ||
    source.productId !== report.identifier ||
    source.purpose !== 'source-archive-validation' ||
    deps.schemaVersion !== 1 ||
    source.mole?.version !== expected.engineVersion ||
    !isDeepStrictEqual(source.binaryBinding, binding) ||
    !isDeepStrictEqual(deps.binaryBinding, {
      ...binding,
      noticesSha256: report.dependencyMaterials.noticesSha256,
    }) ||
    source.app?.revision !== expected.revision ||
    source.mole?.revision !== expected.engineRevision ||
    source.version !== expected.version ||
    source.releaseAcceptance !== 'pending' ||
    source.digestEncoding !== 'hex' ||
    deps.status !== 'verified' ||
    deps.sha512Encoding !== 'hex' ||
    !isDeepStrictEqual(deps.source, report.source) ||
    deps.reviewStatus !== 'pending' ||
    !Number.isSafeInteger(deps.records) ||
    deps.records <= 0 ||
    deps.records > 1024 ||
    deps.records + 2 !== report.dependencyMaterials.packages
  )
    throw Error('Candidate source/dependency packets do not match the same App')
  const assets = [
    ['', report.archive, `MooMole-${expected.version}-mac-${expected.arch}.app.zip`],
    ['', report.diskImage, `MooMole-${expected.version}-mac-${expected.arch}.dmg`],
    ['source', source.app, `MooMole-${expected.version}-source.tar.gz`],
    [
      'source',
      source.mole,
      `Mole-${expected.engineVersion}-${expected.engineRevision.slice(0, 12)}-source.tar.gz`,
    ],
    ['dependencies', deps, `MooMole-${expected.version}-mac-${expected.arch}-dependencies.tar.gz`],
  ]
  for (const [folder, record, name] of assets) checkedFile(join(output, folder), record, name)
  const files = [
    'validation.json',
    'source/SOURCE.json',
    'source/BUILD.md',
    'dependencies/validation.json',
    ...assets.map(([folder, , name]) => (folder ? folder + '/' + name : name)),
  ].sort()
  const found = []
  let totalBytes = 0
  function walk(folder, prefix = '') {
    for (const name of readdirSync(folder)) {
      const path = join(folder, name),
        rel = prefix + name
      if (lstatSync(path).isDirectory()) {
        if (!['source', 'dependencies'].includes(rel)) throw Error('Unknown candidate directory')
        physical(path, true)
        walk(path, rel + '/')
      } else {
        physical(path)
        const size = lstatSync(path).size
        totalBytes += size
        if (size > 256 * 1024 ** 2 || totalBytes > 512 * 1024 ** 2)
          throw Error('Candidate payload exceeds size budget')
        found.push(rel)
      }
    }
  }
  walk(output)
  if (!isDeepStrictEqual(found.sort(), files)) throw Error('Unknown or missing candidate payload')
  const payload = files.map((file) => ({ file, ...fileDigest(join(output, file)) }))
  const candidate = {
    schemaVersion: 1,
    purpose: 'release-candidate-validation',
    version: expected.version,
    architecture: expected.arch,
    source: report.source,
    binaryBinding: binding,
    signing: report.signing,
    notarized: false,
    payload,
    licenseReview: 'pending',
    installationValidation: 'pending',
    gatekeeperValidation: 'pending',
    releaseAcceptance: 'pending',
  }
  writeFileSync(join(output, 'CANDIDATE.json'), JSON.stringify(candidate, null, 2) + '\n', {
    flag: 'wx',
  })
  const sums = [
    ...payload,
    { file: 'CANDIDATE.json', ...fileDigest(join(output, 'CANDIDATE.json')) },
  ]
    .sort((a, b) => a.file.localeCompare(b.file))
    .map((r) => `${r.sha512}  ${r.file}\n`)
    .join('')
  writeFileSync(join(output, 'SHA512SUMS'), sums, { flag: 'wx' })
  return candidate
}
function sourceState(root) {
  function git(args) {
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')),
    )
    const r = spawnSync('git', ['-c', 'core.fsmonitor=false', '-C', root, ...args], {
      env: { ...env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' },
      encoding: 'utf8',
      timeout: 30_000,
    })
    if (r.error || r.status !== 0) throw Error('Cannot verify candidate source state')
    return r.stdout.trim()
  }
  const revision = git(['rev-parse', 'HEAD'])
  if (
    git(['status', '--porcelain', '--untracked-files=normal']) ||
    (process.env.GITHUB_SHA && process.env.GITHUB_SHA !== revision)
  )
    throw Error('Candidate requires clean source matching the CI commit')
  return revision
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [mode, ...args] = process.argv.slice(2),
      flags = new Map()
    for (let i = 0; i < args.length; i += 2) {
      if (!['--report', '--output'].includes(args[i]) || !args[i + 1] || flags.has(args[i]))
        throw Error(
          'Usage: candidate-artifacts.mjs stage --report FILE --output NEW_DIR | seal --output DIR',
        )
      flags.set(args[i], resolve(args[i + 1]))
    }
    if (
      process.platform !== 'darwin' ||
      !['stage', 'seal'].includes(mode) ||
      !flags.has('--output') ||
      (mode === 'stage') !== flags.has('--report')
    )
      throw Error('Invalid candidate command or host')
    const root = resolve(import.meta.dirname, '..'),
      revision = sourceState(root)
    const output = flags.get('--output'),
      rel = relative(root, output).split(sep)
    if (
      isAbsolute(relative(root, output)) ||
      rel.length < 2 ||
      !['ci-artifacts', 'local-artifacts'].includes(rel[0])
    )
      throw Error('Candidate output must be inside an ignored build output directory')
    if (!lstatSync(dirname(output), { throwIfNoEntry: false })) mkdirSync(dirname(output))
    const expected = {
      revision,
      arch: process.arch,
      version: checkVersion(root),
      engineRevision,
      engineVersion: sourceVersion(
        readFileSync(join(root, 'src-tauri/resources/mole/mole'), 'utf8'),
      ),
    }
    const result =
      mode === 'stage'
        ? stageCandidate({ reportPath: flags.get('--report'), output, expected })
        : sealCandidate({ output, expected })
    if (sourceState(root) !== revision) throw Error('Source changed during candidate assembly')
    console.log(
      `Candidate ${mode} verified for ${result.version}/${expected.arch}; release acceptance pending.`,
    )
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
}
