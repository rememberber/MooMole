import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  closeSync,
  constants,
  cpSync,
  lstatSync,
  openSync,
  readdirSync,
  readSync,
  writeFileSync,
} from 'node:fs'
import { isAbsolute, join, relative, resolve, sep } from 'node:path'
import { checkMachO, verifyPrepared, writeManifest } from './engine-manifest.mjs'
import { verifyNotices } from './dependency-notices.mjs'

export function command(program, args) {
  const result = spawnSync(program, args, {
    encoding: 'utf8',
    timeout: 60_000,
    maxBuffer: 2 * 1024 ** 2,
    env: { ...process.env, LC_ALL: 'C' },
  })
  if (result.error || result.status !== 0)
    throw new Error(`${program} failed: ${result.error?.message || result.stderr || result.stdout}`)
  return { stdout: result.stdout, stderr: result.stderr }
}

const codePaths = [
  'Contents/MacOS/moomole',
  'Contents/Resources/mole/bin/analyze-go',
  'Contents/Resources/mole/bin/status-go',
]
export function assertProductionPolicy(config) {
  const csp = config.app?.security?.csp
  if (typeof csp !== 'string') throw new Error('A production CSP is required')
  const directives = new Map()
  for (const part of csp.split(';')) {
    const [rawName, ...sources] = part.trim().split(/\s+/)
    const name = rawName.toLowerCase()
    if (!name) continue
    if (directives.has(name)) throw new Error('Duplicate CSP directives are not supported')
    directives.set(name, sources)
  }
  if (JSON.stringify(directives.get('default-src')) !== JSON.stringify(["'self'"]))
    throw new Error('Production default-src must be self')
  const connect = directives.get('connect-src')
  if (
    !connect?.length ||
    connect.some((source) => !['ipc:', 'http://ipc.localhost'].includes(source))
  )
    throw new Error('Production connect-src may only reach Tauri IPC')
  for (const name of ['script-src', 'script-src-elem', 'script-src-attr']) {
    const sources = directives.get(name)
    if (sources?.some((source) => !["'self'", "'none'"].includes(source)))
      throw new Error('Production script sources are outside this recipe')
  }
}
function inventoryApp(app) {
  const code = []
  let entries = 0,
    total = 0
  function walk(path, depth) {
    if (++entries > 2048 || depth > 64) throw new Error('App inventory exceeds entry/depth budget')
    const meta = lstatSync(path)
    if (meta.isSymbolicLink()) throw new Error('App links are not supported by this signing recipe')
    if (meta.isDirectory()) {
      for (const name of readdirSync(path)) walk(join(path, name), depth + 1)
      return
    }
    if (!meta.isFile() || meta.nlink !== 1) throw new Error('App requires unshared regular files')
    total += meta.size
    if (meta.size > 128 * 1024 ** 2 || total > 256 * 1024 ** 2)
      throw new Error('App exceeds size budget')
    const header = Buffer.alloc(4)
    const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
    try {
      readSync(fd, header, 0, 4, 0)
    } finally {
      closeSync(fd)
    }
    if (
      [
        0xfeedface, 0xfeedfacf, 0xcafebabe, 0xbebafeca, 0xcafebabf, 0xbfbafeca, 0xcefaedfe,
        0xcffaedfe,
      ].includes(header.readUInt32LE(0))
    )
      code.push(relative(app, path).split(sep).join('/'))
  }
  if (!lstatSync(app).isDirectory()) throw new Error('Expected an App directory')
  walk(app, 0)
  if (JSON.stringify(code.sort()) !== JSON.stringify([...codePaths].sort()))
    throw new Error('Unexpected or missing native code in App; review the signing recipe')
  return total
}

export function inspectApp(app, expected) {
  if (process.platform !== 'darwin') throw new Error('App verification requires macOS')
  app = resolve(app)
  const bytes = inventoryApp(app)
  const info = JSON.parse(
    command('/usr/bin/plutil', ['-convert', 'json', '-o', '-', join(app, 'Contents/Info.plist')])
      .stdout,
  )
  if (
    info.CFBundleIdentifier !== expected.identifier ||
    info.CFBundleExecutable !== 'moomole' ||
    info.CFBundleShortVersionString !== expected.version ||
    info.CFBundleVersion !== expected.version
  )
    throw new Error('App identifier, executable or version does not match the build')
  for (const path of codePaths) checkMachO(join(app, path), expected.arch)
  const engine = verifyPrepared(join(app, 'Contents/Resources/mole'))
  if (engine.arch !== expected.arch) throw new Error('App and engine manifest architectures differ')
  const materials = verifyNotices(join(app, 'Contents/Resources/legal'), {
    version: expected.version,
    revision: engine.revision,
    arch: expected.arch,
  })
  if (materials.engineVersion !== engine.version)
    throw new Error('Dependency material engine version differs')
  return { bytes, engine, materials }
}

function identifiers(identifier) {
  return [identifier, `${identifier}.mole.analyze`, `${identifier}.mole.status`]
}

export function verifySignedApp(app, expected) {
  const inspected = inspectApp(app, expected)
  const ids = identifiers(expected.identifier)
  const signatures = codePaths.map((path, i) => {
    const target = i === 0 ? app : join(app, path)
    command('/usr/bin/codesign', ['--verify', '--strict', '--verbose=2', target])
    const result = command('/usr/bin/codesign', ['--display', '--verbose=4', target])
    const lines = (result.stdout + result.stderr).split(/\r?\n/)
    if (
      !lines.includes('Signature=adhoc') ||
      !lines.includes(`Identifier=${ids[i]}`) ||
      lines.some((line) => line.startsWith('Authority=') || line.includes('linker-signed'))
    )
      throw new Error('Expected a final ad-hoc signature with the application identifier')
    const entitlements = command('/usr/bin/codesign', [
      '--display',
      '--entitlements',
      ':-',
      target,
    ]).stdout.trim()
    if (entitlements) throw new Error('This recipe does not permit custom entitlements')
    return { path, identifier: ids[i], signing: 'ad-hoc', entitlements: 'none' }
  })
  command('/usr/bin/codesign', ['--verify', '--deep', '--strict', '--verbose=2', app])
  return {
    version: expected.version,
    architecture: expected.arch,
    identifier: expected.identifier,
    bytes: inspected.bytes,
    signing: 'ad-hoc',
    notarized: false,
    engineVersion: inspected.engine.version,
    engineRevision: inspected.engine.revision,
    dependencyMaterials: {
      packages: inspected.materials.packages.length,
      materials: inspected.materials.materialCount,
      reviewStatus: inspected.materials.reviewStatus,
      reviewRequired: inspected.materials.reviewRequired.length,
      noticesSha256: inspected.materials.notices.sha256,
    },
    signatures,
  }
}

export function signAppCopy(source, destination, expected) {
  source = resolve(source)
  destination = resolve(destination)
  const within = (parent, child) => {
    const path = relative(parent, child)
    return path === '' || (!path.startsWith(`..${sep}`) && path !== '..' && !isAbsolute(path))
  }
  if (
    within(source, destination) ||
    within(destination, source) ||
    lstatSync(destination, { throwIfNoEntry: false })
  )
    throw new Error('Signing requires a fresh destination outside the source App')
  inspectApp(source, expected)
  cpSync(source, destination, {
    recursive: true,
    dereference: false,
    force: false,
    errorOnExist: true,
  })
  const { engine } = inspectApp(destination, expected)
  const ids = identifiers(expected.identifier)
  for (const i of [1, 2])
    command('/usr/bin/codesign', [
      '--force',
      '--sign',
      '-',
      '--timestamp=none',
      '--identifier',
      ids[i],
      join(destination, codePaths[i]),
    ])
  // Signing changes Mach-O bytes: update the engine copy before sealing the App.
  writeManifest(join(destination, 'Contents/Resources/mole'), engine.build.goVersion, expected.arch)
  verifyPrepared(join(destination, 'Contents/Resources/mole'))
  command('/usr/bin/codesign', [
    '--force',
    '--sign',
    '-',
    '--timestamp=none',
    '--identifier',
    expected.identifier,
    destination,
  ])
  return verifySignedApp(destination, expected)
}

export function fileDigest(path) {
  const hash = createHash('sha512')
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const buffer = Buffer.alloc(128 * 1024)
    let size = 0,
      count
    while ((count = readSync(fd, buffer, 0, buffer.length, null)) > 0) {
      size += count
      hash.update(buffer.subarray(0, count))
    }
    return { size, sha512: hash.digest('hex') }
  } finally {
    closeSync(fd)
  }
}

export function writeArtifactReport(path, value) {
  writeFileSync(path, JSON.stringify(value, null, 2) + '\n', { flag: 'wx' })
}
