import { createHash } from 'node:crypto'
import {
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { basename, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

const keepCache = new Set([
  'cargo',
  'rustup',
  'toolchains',
  'playwright',
  'mole-source',
  'license-sources',
  'verification',
])
const disposable = [
  'src-tauri/target',
  'dist',
  'test-results',
  'playwright-report',
  'tsconfig.tsbuildinfo',
]

export function generatedTreeBytes(path) {
  const first = lstatSync(path)
  let bytes = 0
  const seen = new Set()
  function walk(path) {
    const m = lstatSync(path)
    if (m.dev !== first.dev) throw Error(`Mounted content retained: ${path}`)
    if (m.isSymbolicLink()) return
    if (m.isDirectory()) for (const name of readdirSync(path)) walk(join(path, name))
    else if (!m.isFile()) throw Error(`Unknown file type retained: ${path}`)
    const id = `${m.dev}:${m.ino}`
    if (!seen.has(id)) {
      seen.add(id)
      bytes += m.blocks * 512
    }
  }
  walk(path)
  return bytes
}
function compactEvidence(root, cacheNames) {
  const folder = join(root, '.cache/verification'),
    output = join(folder, 'native-evidence.json')
  const fm = lstatSync(folder, { throwIfNoEntry: false })
  if (fm && (!fm.isDirectory() || fm.isSymbolicLink()))
    throw Error('Verification directory is redirected; retained')
  let evidence = {}
  const om = lstatSync(output, { throwIfNoEntry: false })
  if (om) {
    if (!om.isFile() || om.isSymbolicLink() || om.nlink !== 1 || om.size > 4 * 1024 ** 2)
      throw Error('Verification record changed; retained')
    const old = JSON.parse(readFileSync(output, 'utf8'))
    if (
      old.schemaVersion !== 1 ||
      !old.entries ||
      typeof old.entries !== 'object' ||
      Array.isArray(old.entries)
    )
      throw Error('Unknown verification record; retained')
    evidence = old.entries
  }
  for (const name of cacheNames) {
    if (
      !name.endsWith('.json') ||
      !/(native|evidence|protocol|baseline|scheduled-files|validation)/.test(name) ||
      /(translations|messages|wip)/.test(name)
    )
      continue
    const path = join(root, '.cache', name),
      m = lstatSync(path)
    if (!m.isFile() || m.isSymbolicLink() || m.size > 1024 ** 2) continue
    const bytes = readFileSync(path)
    try {
      evidence[name] = {
        sha256: createHash('sha256').update(bytes).digest('hex'),
        data: JSON.parse(bytes),
      }
    } catch {
      /* incomplete transient evidence is discarded */
    }
  }
  const names = Object.keys(evidence).sort()
  if (names.length > 128) throw Error('Evidence needs consolidation before cleanup')
  const encoded =
    JSON.stringify({
      schemaVersion: 1,
      entries: Object.fromEntries(names.map((n) => [n, evidence[n]])),
    }) + '\n'
  if (Buffer.byteLength(encoded) > 4 * 1024 ** 2)
    throw Error('Evidence exceeds compact retention budget')
  mkdirSync(folder, { recursive: true })
  writeFileSync(output, encoded)
}
export function cleanWorkspace(root, { apply = false } = {}) {
  root = resolve(root)
  if (
    realpathSync(root) !== root ||
    JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).name !== 'moomole'
  )
    throw Error('Expected the physical MooMole project directory')
  const cache = join(root, '.cache'),
    cm = lstatSync(cache, { throwIfNoEntry: false })
  if (cm && (!cm.isDirectory() || cm.isSymbolicLink()))
    throw Error('Cache directory is redirected; retained')
  const cacheNames = cm ? readdirSync(cache) : []
  const paths = [
    ...disposable,
    ...cacheNames.filter((name) => !keepCache.has(name)).map((name) => `.cache/${name}`),
  ].filter((name) => lstatSync(join(root, name), { throwIfNoEntry: false }))
  // Validate the full plan before removing anything. Never follow a symlink or
  // recurse through a volume that happens to be mounted inside a fixture.
  const entries = paths.map((name) => {
    const components = name.split('/')
    for (let i = 1; i < components.length; i++) {
      const parent = join(root, ...components.slice(0, i)),
        m = lstatSync(parent)
      if (!m.isDirectory() || m.isSymbolicLink())
        throw Error(`Redirected parent retained: ${parent}`)
    }
    return {
      name,
      bytes: generatedTreeBytes(join(root, name)),
      identity: lstatSync(join(root, name)),
    }
  })
  if (apply) {
    compactEvidence(root, cacheNames)
    for (const entry of entries) {
      const path = join(root, entry.name),
        current = lstatSync(path)
      if (current.dev !== entry.identity.dev || current.ino !== entry.identity.ino)
        throw Error(`Generated path changed; retained: ${entry.name}`)
      rmSync(path, { recursive: true, force: false })
    }
  }
  return {
    applied: apply,
    paths: paths.length,
    bytes: entries.reduce((sum, e) => sum + e.bytes, 0),
    retained: ['local-artifacts', 'node_modules', ...[...keepCache].map((n) => `.cache/${n}`)],
  }
}
function assertIdle(root) {
  const result = spawnSync('/bin/ps', ['-axo', 'pid=,comm='], { encoding: 'utf8', timeout: 5000 })
  if (result.status !== 0) throw Error('Cannot check active build processes')
  for (const line of result.stdout.split('\n')) {
    const match = line.trim().match(/^(\d+)\s+(.*)$/)
    if (!match || Number(match[1]) === process.pid) continue
    const command = match[2]
    if (
      ['cargo', 'rustc', 'rustdoc'].includes(basename(command)) ||
      command.startsWith(`${root}/src-tauri/target/`) ||
      command.startsWith(`${root}/.cache/`)
    )
      throw Error(
        `Active project/build process retained: PID ${match[1]}; finish it before cleanup`,
      )
  }
}
if (import.meta.url === pathToFileURL(resolve(process.argv[1] ?? '')).href) {
  try {
    const args = process.argv.slice(2)
    if (args.some((a) => a !== '--apply') || args.length > 1)
      throw Error('Usage: npm run clean:workspace -- [--apply]')
    const root = realpathSync(resolve(import.meta.dirname, '..'))
    if (args.includes('--apply')) assertIdle(root)
    console.log(JSON.stringify(cleanWorkspace(root, { apply: args.includes('--apply') }), null, 2))
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
