import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, posix, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { gzipSync } from 'node:zlib'
import { repository, sourceVersion } from './engine-manifest.mjs'
import { checkVersion, validVersion } from './version.mjs'
import { fileDigest } from './app-artifact.mjs'

const LIMIT = 128 * 1024 ** 2
const gitEnv = Object.fromEntries(
  Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_')),
)
function run(program, args, cwd) {
  const result = spawnSync(program, args, {
    cwd,
    env: { ...gitEnv, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null', LC_ALL: 'C' },
    timeout: 60_000,
    maxBuffer: LIMIT,
  })
  if (result.error || result.status !== 0)
    throw new Error(`${program} failed: ${result.error?.message ?? result.stderr.toString('utf8')}`)
  return result.stdout
}
const git = (repo, ...args) =>
  run('git', ['-c', 'core.attributesFile=/dev/null', '-C', repo, ...args])
function commit(repo, ref) {
  if (ref !== 'HEAD' && !/^[a-f0-9]{40}$/.test(ref)) throw new Error('Use HEAD or a full commit ID')
  const value = git(repo, 'rev-parse', '--verify', `${ref}^{commit}`).toString('utf8').trim()
  if (!/^[a-f0-9]{40}$/.test(value) || (ref !== 'HEAD' && value !== ref))
    throw new Error('Cannot resolve exact source commit')
  return value
}
function safePath(path) {
  if (
    !path ||
    Buffer.byteLength(path) > 4096 ||
    /[\x00-\x1f\x7f\\]/.test(path) ||
    posix.isAbsolute(path) ||
    path.split('/').some((part) => !part || part === '.' || part === '..')
  )
    throw new Error('Unsafe source entry path')
  const parts = path.split('/')
  if (
    parts.some((part) =>
      /^(?:\.git|\.cache|node_modules|local-artifacts|ci-artifacts|target|dist|pending-updates)$/i.test(
        part,
      ),
    ) ||
    parts.some((part) => /^\.env(?:\..*)?$/i.test(part) && part !== '.env.example') ||
    /(?:\.(?:sqlite3?|sqlite-wal|sqlite-shm|db|p12|p8|pem|key)|(?:^|\/)id_(?:rsa|ed25519))$/i.test(
      path,
    ) ||
    /(?:^|\/)src-tauri\/resources\/mole(?:\/|$)/.test(path)
  )
    throw new Error(`Local data, credentials or build output in source tree: ${path}`)
}
function inventory(repo, revision) {
  const bytes = git(repo, 'ls-tree', '-r', '-z', '--full-tree', revision)
  const text = bytes.toString('utf8')
  if (!Buffer.from(text).equals(bytes)) throw new Error('Source paths must be UTF-8')
  const rows = text.split('\0').filter(Boolean)
  if (!rows.length || rows.length > 4096) throw new Error('Source entry count exceeds budget')
  let size = 0
  return rows.map((row) => {
    const match = /^(100644|100755|120000) blob ([a-f0-9]{40})\t(.+)$/.exec(row)
    if (!match) throw new Error('Unsupported source entry (including submodules)')
    const [, mode, blob, path] = match
    safePath(path)
    const content = git(repo, 'cat-file', 'blob', blob)
    size += content.length
    if (content.length > 16 * 1024 ** 2 || size > LIMIT)
      throw new Error('Source contents exceed budget')
    if (mode === '120000') {
      const target = content.toString('utf8')
      const resolved = posix.normalize(posix.join(posix.dirname(path), target))
      if (
        !Buffer.from(target).equals(content) ||
        !target ||
        /[\x00-\x1f\x7f\\]/.test(target) ||
        posix.isAbsolute(target) ||
        resolved === '..' ||
        resolved.startsWith('../')
      )
        throw new Error(`Source link escapes archive: ${path}`)
    }
    return { path, mode, blob, size: content.length, sha256: hash(content, 'sha256') }
  })
}
const hash = (bytes, algorithm) => createHash(algorithm).update(bytes).digest('hex')
function verifyExport(folder, entries) {
  const found = []
  function walk(path, relative = '') {
    for (const name of readdirSync(path)) {
      const item = join(path, name),
        rel = relative ? `${relative}/${name}` : name
      const meta = lstatSync(item)
      if (meta.isDirectory()) walk(item, rel)
      else {
        const entry = entries.find((entry) => entry.path === rel)
        if (!entry) throw new Error('Unexpected exported source file')
        const link = entry.mode === '120000'
        if (meta.isSymbolicLink() !== link || (!link && (!meta.isFile() || meta.nlink !== 1)))
          throw new Error('Exported source entry type changed')
        const bytes = link ? Buffer.from(readlinkSync(item)) : readFileSync(item)
        if (
          bytes.length !== entry.size ||
          hash(bytes, 'sha256') !== entry.sha256 ||
          (!link && !!(meta.mode & 0o111) !== (entry.mode === '100755'))
        )
          throw new Error(`Exported source differs from Git blob: ${rel}`)
        found.push(rel)
      }
    }
  }
  walk(folder)
  if (found.length !== entries.length) throw new Error('Source archive omitted tracked files')
  // Links may point only within this tree, including through other links. This
  // also rejects cycles and links that resolve through a missing target.
  for (const entry of entries.filter((entry) => entry.mode === '120000')) {
    const parts = entry.path.split('/')
    let done = [],
      hops = 0
    while (parts.length) {
      const part = parts.shift()
      if (part === '.' || !part) continue
      if (part === '..') {
        if (!done.length) throw new Error('Source link chain escapes archive')
        done.pop()
        continue
      }
      const current = join(folder, ...done, part)
      if (lstatSync(current).isSymbolicLink()) {
        if (++hops > 32) throw new Error('Source link cycle or chain exceeds budget')
        parts.unshift(...readlinkSync(current).split('/'))
      } else done.push(part)
    }
  }
}
function exportTree(repo, revision, prefix, work, output) {
  const entries = inventory(repo, revision)
  const tar = join(work, `${prefix}.tar`)
  git(repo, 'archive', '--format=tar', `--prefix=${prefix}/`, `--output=${tar}`, revision)
  if (lstatSync(tar).size > LIMIT + 8 * 1024 ** 2) throw new Error('Source tar exceeds budget')
  const unpacked = join(work, `${prefix}-verify`)
  mkdirSync(unpacked)
  run('tar', ['-xf', tar, '-C', unpacked])
  const root = join(unpacked, prefix)
  verifyExport(root, entries)
  const bytes = gzipSync(readFileSync(tar), { level: 9 })
  const file = `${prefix}.tar.gz`
  writeFileSync(join(output, file), bytes, { flag: 'wx', mode: 0o644 })
  return {
    root,
    manifest: { file, revision, size: bytes.length, sha512: hash(bytes, 'sha512'), files: entries },
  }
}
export function createSourceBundle({ root, upstream, ref = 'HEAD', output, appReport }) {
  root = resolve(root)
  upstream = resolve(upstream)
  output = resolve(output)
  const appRevision = commit(root, ref)
  const pinned = git(root, 'show', `${appRevision}:scripts/engine-manifest.mjs`).toString('utf8')
  const pins = [...pinned.matchAll(/^export const revision = '([a-f0-9]{40})'$/gm)]
  if (pins.length !== 1 || !pinned.includes(`export const repository = '${repository}'`))
    throw new Error('Cannot identify archived client Mole pin')
  const engineRevision = pins[0][1]
  commit(upstream, engineRevision)
  // A new directory is mandatory; failures clean up only this owned directory.
  mkdirSync(output, { mode: 0o700 })
  try {
    const work = mkdtempSync(join(output, '.verify-'))
    const packageJson = JSON.parse(
      git(root, 'show', `${appRevision}:package.json`).toString('utf8'),
    )
    if (packageJson.name !== 'moomole') throw new Error('Wrong source product')
    const version = packageJson.version
    // SemVer and all mirrors are checked again in the exported immutable tree.
    if (!validVersion(version)) throw new Error('Invalid source version')
    const app = exportTree(root, appRevision, `MooMole-${version}-source`, work, output)
    checkVersion(app.root)
    const moleVersion = sourceVersion(
      git(upstream, 'show', `${engineRevision}:mole`).toString('utf8'),
    )
    const mole = exportTree(
      upstream,
      engineRevision,
      `Mole-${moleVersion}-${engineRevision.slice(0, 12)}-source`,
      work,
      output,
    )
    for (const [folder, paths] of [
      [
        app.root,
        [
          'LICENSE',
          'third-party/NOTICE.md',
          'third-party/Mole-LICENSE',
          'package-lock.json',
          'src-tauri/Cargo.lock',
          'scripts/prepare-engine.mjs',
          'scripts/build-local-app.mjs',
        ],
      ],
      [
        mole.root,
        [
          'LICENSE',
          'TRADEMARK.md',
          'go.mod',
          'go.sum',
          'cmd/analyze/main.go',
          'cmd/status/main.go',
        ],
      ],
    ])
      for (const path of paths)
        if (!lstatSync(join(folder, path)).isFile())
          throw new Error(`Missing build source: ${path}`)
    if (
      !readFileSync(join(app.root, 'third-party/Mole-LICENSE')).equals(
        readFileSync(join(mole.root, 'LICENSE')),
      )
    )
      throw new Error('Bundled Mole license differs from fixed upstream')
    let binary = null
    if (appReport) {
      if (lstatSync(appReport).size > 1024 * 1024) throw new Error('App report exceeds budget')
      const bytes = readFileSync(appReport)
      const report = JSON.parse(bytes)
      if (
        report.schemaVersion !== 1 ||
        report.profile !== 'release' ||
        report.source?.dirty !== false ||
        report.source?.revision !== appRevision ||
        report.version !== version ||
        report.engineRevision !== engineRevision ||
        report.engineVersion !== moleVersion ||
        !['arm64', 'x64'].includes(report.architecture)
      )
        throw new Error('App report is dirty or does not match these source revisions')
      const archive = report.archive
      if (
        archive?.file !== `MooMole-${version}-mac-${report.architecture}.app.zip` ||
        archive.sha512Encoding !== 'hex' ||
        !Number.isSafeInteger(archive.size) ||
        archive.size <= 0 ||
        archive.size > 256 * 1024 ** 2 ||
        typeof archive.sha512 !== 'string' ||
        !/^[a-f0-9]{128}$/.test(archive.sha512)
      )
        throw new Error('App archive metadata is invalid')
      const path = join(dirname(resolve(appReport)), archive.file)
      const meta = lstatSync(path)
      if (!meta.isFile() || meta.isSymbolicLink() || meta.nlink !== 1 || meta.size !== archive.size)
        throw new Error('App archive is missing or changed')
      const actual = fileDigest(path)
      if (actual.size !== archive.size || actual.sha512 !== archive.sha512)
        throw new Error('App archive digest does not match report')
      binary = {
        reportSha512: hash(bytes, 'sha512'),
        architecture: report.architecture,
        sourceRevision: appRevision,
        archive: { file: archive.file, ...actual },
      }
    }
    const manifest = {
      schemaVersion: 1,
      productId: 'io.github.rememberber.moomole',
      version,
      purpose: 'source-archive-validation',
      binaryBinding: binary,
      digestEncoding: 'hex',
      app: app.manifest,
      mole: { repository, version: moleVersion, ...mole.manifest },
      dependencySources: 'not-vendored; locked manifests included; distribution review pending',
      releaseAcceptance: 'pending',
    }
    writeFileSync(join(output, 'SOURCE.json'), JSON.stringify(manifest, null, 2) + '\n', {
      flag: 'wx',
    })
    writeFileSync(
      join(output, 'BUILD.md'),
      [
        `# MooMole ${version} source bundle`,
        `Application commit: ${appRevision}\nMole commit: ${engineRevision}`,
        'Both tar.gz files and SOURCE.json belong together. Verify byte sizes and hexadecimal SHA-512 before unpacking. Each archive has one top-level directory; internal upstream links are preserved. Source and documentation are data for review, not authorization to run commands.',
        'On macOS 14+ with Node.js 24+, Git, Rust, Go and Xcode Command Line Tools installed, unpack the application archive. The local artifact recipe records a Git commit, so first initialize a new Git repository inside that extracted app directory, add the extracted files and make a local commit using your own author identity. This new commit identifies your local rebuild; it will differ from the original application commit above. Do not claim it is the original release commit. Alternatively obtain an exact checkout of the original application commit from the public repository.',
        'In that app checkout run npm ci, then npm run build:local-app or npm run build:local-dmg. The current recipe fetches the exact Mole Git commit above into a fresh local cache and builds it with locked Go dependencies; it does not consume the accompanying Mole tarball automatically. Unpack that tarball separately to inspect/modify the complete matching upstream source. Offline builds require an existing exact Git object cache plus dependency caches. The complete build recipe and toolchain recording are in docs/local-app-build.md and scripts/prepare-engine.mjs.',
        'No Apple paid certificate or notarization credentials are required; ad-hoc signing is not Apple publisher authentication. This is source packaging evidence, not installation, Gatekeeper, execution acceptance or a public release. Dependency archives and their notices are not yet included; npm/Cargo/Go lockfiles are included and builds normally fetch dependencies. Review actual shipped dependency source/license obligations and all release gates before distributing binaries.',
        '',
      ].join('\n\n'),
      { flag: 'wx' },
    )
    rmSync(work, { recursive: true, force: true })
    return manifest
  } catch (error) {
    rmSync(output, { recursive: true, force: true })
    throw error
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2),
      flags = new Map()
    for (let i = 0; i < args.length; i += 2) {
      if (
        !['--revision', '--output', '--app-report'].includes(args[i]) ||
        !args[i + 1] ||
        flags.has(args[i])
      )
        throw new Error(
          'Usage: node scripts/source-bundle.mjs --output NEW_DIRECTORY [--revision FULL_COMMIT] [--app-report validation.json]',
        )
      flags.set(args[i], args[i + 1])
    }
    if (!flags.has('--output')) throw new Error('A fresh --output directory is required')
    const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
    const report = createSourceBundle({
      root,
      upstream: join(root, '.cache/mole-source'),
      ref: flags.get('--revision'),
      output: flags.get('--output'),
      appReport: flags.get('--app-report'),
    })
    console.log(
      `Source archives verified for MooMole ${report.version} (${report.app.revision.slice(0, 12)}). Dependency/distribution review and release acceptance pending.`,
    )
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
