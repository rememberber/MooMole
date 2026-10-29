import { createHash } from 'node:crypto'
import {
  closeSync,
  constants,
  lstatSync,
  openSync,
  readdirSync,
  readFileSync,
  readSync,
  writeFileSync,
} from 'node:fs'
import { join, relative } from 'node:path'

export const revision = '308bf9911179ddfa40e5df9fdc42095a18ba5cfc'
export const repository = 'https://github.com/tw93/Mole'
export const requiredFiles = [
  'mole',
  'mo',
  'LICENSE',
  'TRADEMARK.md',
  'bin/analyze-go',
  'bin/status-go',
  'bin/clean.sh',
  'bin/uninstall.sh',
  'moomole-clean-preview.sh',
  'moomole-review-rules.sh',
]

export function sourceVersion(text) {
  const matches = [...text.matchAll(/^VERSION="(\d+\.\d+\.\d+)"$/gm)]
  if (matches.length !== 1) throw new Error('Cannot identify the pinned Mole CLI version')
  return matches[0][1]
}

export function inventory(root) {
  const files = Object.create(null)
  let entries = 0
  let total = 0
  function walk(directory, depth = 0) {
    if (depth > 64) throw new Error('Engine directory nesting exceeds limit')
    for (const name of readdirSync(directory).sort()) {
      if (++entries > 1000) throw new Error('Engine contains too many entries')
      const path = join(directory, name)
      const rel = relative(root, path)
      const meta = lstatSync(path)
      if (meta.isSymbolicLink()) throw new Error(`Engine links are not allowed: ${rel}`)
      if (meta.isDirectory()) walk(path, depth + 1)
      else if (meta.isFile() && meta.nlink === 1) {
        if (rel === 'UPSTREAM.json') continue
        total += meta.size
        if (meta.size > 64 * 1024 ** 2 || total > 128 * 1024 ** 2)
          throw new Error('Engine exceeds size budget')
        files[rel] = {
          size: meta.size,
          sha256: createHash('sha256').update(readFileSync(path)).digest('hex'),
          executable: !!(meta.mode & 0o111),
        }
      } else throw new Error(`Engine requires unshared regular files: ${rel}`)
    }
  }
  if (!lstatSync(root).isDirectory() || lstatSync(root).isSymbolicLink())
    throw new Error('Engine root must be a physical directory')
  walk(root)
  for (const file of requiredFiles)
    if (!Object.hasOwn(files, file)) throw new Error(`Missing engine resource: ${file}`)
  if (Object.keys(files).length > 512) throw new Error('Engine file count exceeds limit')
  return Object.fromEntries(Object.entries(files).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
}

export function checkMachO(path, arch) {
  if (!(lstatSync(path).mode & 0o111))
    throw new Error(`Engine executable permission missing: ${path}`)
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW)
  try {
    const header = Buffer.alloc(8)
    const cpu = { arm64: 0x0100000c, x64: 0x01000007 }[arch]
    if (
      !cpu ||
      readSync(fd, header, 0, 8, 0) !== 8 ||
      header.readUInt32LE(0) !== 0xfeedfacf ||
      header.readUInt32LE(4) !== cpu
    )
      throw new Error(`Engine executable architecture mismatch: ${path}`)
  } finally {
    closeSync(fd)
  }
}

export function writeManifest(root, goVersion, arch = process.arch) {
  const version = sourceVersion(readFileSync(join(root, 'mole'), 'utf8'))
  for (const name of ['analyze', 'status']) checkMachO(join(root, `bin/${name}-go`), arch)
  const manifest = {
    schemaVersion: 1,
    repository,
    revision,
    version,
    platform: 'darwin',
    arch,
    cleanProtocol: 2,
    rulesProtocol: 4,
    build: { recipeVersion: 1, goVersion, goos: 'darwin', goarch: arch === 'x64' ? 'amd64' : arch },
    files: inventory(root),
  }
  writeFileSync(join(root, 'UPSTREAM.json'), JSON.stringify(manifest, null, 2) + '\n')
  return manifest
}

export function verifyPrepared(root) {
  const manifest = JSON.parse(readFileSync(join(root, 'UPSTREAM.json'), 'utf8'))
  if (
    manifest.schemaVersion !== 1 ||
    manifest.repository !== repository ||
    manifest.revision !== revision ||
    manifest.platform !== 'darwin' ||
    !['arm64', 'x64'].includes(manifest.arch) ||
    manifest.cleanProtocol !== 2 ||
    manifest.rulesProtocol !== 4 ||
    manifest.build?.recipeVersion !== 1 ||
    manifest.build.goos !== 'darwin' ||
    manifest.build.goarch !== (manifest.arch === 'x64' ? 'amd64' : 'arm64') ||
    typeof manifest.build.goVersion !== 'string' ||
    !manifest.build.goVersion.startsWith('go version go')
  )
    throw new Error('Unsupported engine manifest')
  if (manifest.version !== sourceVersion(readFileSync(join(root, 'mole'), 'utf8')))
    throw new Error('Engine version mismatch')
  const actual = inventory(root)
  if (
    !manifest.files ||
    Object.keys(manifest.files).length !== Object.keys(actual).length ||
    !Object.entries(actual).every(
      ([name, file]) =>
        manifest.files[name]?.size === file.size &&
        manifest.files[name]?.sha256 === file.sha256 &&
        manifest.files[name]?.executable === file.executable,
    )
  )
    throw new Error('Engine resources do not match the build manifest')
  for (const name of ['analyze', 'status']) checkMachO(join(root, `bin/${name}-go`), manifest.arch)
  return manifest
}
