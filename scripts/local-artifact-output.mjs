import { createHash } from 'node:crypto'
import {
  cpSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, join, resolve } from 'node:path'

const marker = '.moomole-artifacts.json'
const allowed = (name) =>
  name === 'validation.json' ||
  name === 'MooMole.app' ||
  /^MooMole-[0-9][A-Za-z0-9.+-]*-mac-(arm64|x64)\.(dmg|app\.zip)$/.test(name)

// Refuse links, mount crossings and unexpected file types before replacing a
// previous delivery. A locally edited delivery is preserved for review.
function fingerprint(path) {
  const root = lstatSync(path),
    hash = createHash('sha512')
  let count = 0,
    bytes = 0
  function walk(path, name) {
    const m = lstatSync(path)
    if (++count > 20_000 || m.dev !== root.dev || m.isSymbolicLink())
      throw Error('Unsupported local artifact tree; retained unchanged')
    hash.update(JSON.stringify([name, m.mode & 0o777, m.isDirectory() ? 'directory' : 'file']))
    if (m.isDirectory()) {
      for (const child of readdirSync(path).sort()) walk(join(path, child), `${name}/${child}`)
    } else if (m.isFile() && m.nlink === 1) {
      bytes += m.size
      if (bytes > 1024 ** 3) throw Error('Local artifact exceeds publication budget')
      hash.update(readFileSync(path))
    } else throw Error('Unsupported local artifact type; retained unchanged')
  }
  walk(path, '')
  return hash.digest('hex')
}
function previous(output) {
  const m = lstatSync(output, { throwIfNoEntry: false })
  if (!m) return false
  if (!m.isDirectory() || m.isSymbolicLink() || realpathSync(output) !== output)
    throw Error('Local output must be a physical directory')
  const names = readdirSync(output).sort()
  if (!names.length) return true
  const ownership = lstatSync(join(output, marker), { throwIfNoEntry: false })
  if (!ownership?.isFile() || ownership.isSymbolicLink() || ownership.size > 8192)
    throw Error('Unmanaged local artifacts were preserved; move them before publishing')
  const record = JSON.parse(readFileSync(join(output, marker), 'utf8'))
  if (
    record.schemaVersion !== 1 ||
    record.purpose !== 'moomole-local-artifacts' ||
    !Array.isArray(record.files) ||
    record.files.length > 4 ||
    record.files.some((f) => !allowed(f.name) || !/^[a-f0-9]{128}$/.test(f.sha512)) ||
    new Set(record.files.map((f) => f.name)).size !== record.files.length ||
    JSON.stringify(names) !== JSON.stringify([marker, ...record.files.map((f) => f.name)].sort())
  )
    throw Error('Local output ownership changed; existing files were preserved')
  for (const file of record.files)
    if (fingerprint(join(output, file.name)) !== file.sha512)
      throw Error('A local delivery changed; existing files were preserved')
  return true
}

export function publishLocalArtifacts(stage, output, names) {
  output = resolve(output)
  if (
    !names.includes('validation.json') ||
    names.length > 4 ||
    names.some((n) => !allowed(n)) ||
    new Set(names).size !== names.length
  )
    throw Error('Unexpected local artifact selection')
  mkdirSync(dirname(output), { recursive: true })
  if (realpathSync(dirname(output)) !== dirname(output))
    throw Error('Output parent must be physical')
  const hadPrevious = previous(output)
  // This short-lived sibling permits same-volume rename and rollback. All
  // signing, ZIP extraction and DMG work happens in the system temp directory.
  const work = mkdtempSync(join(dirname(output), '.MooMole-publish-'))
  const next = join(work, 'next'),
    backup = join(work, 'previous')
  let movedPrevious = false,
    installed = false,
    preserve = false
  try {
    mkdirSync(next)
    const files = names.map((name) => {
      const original = fingerprint(join(stage, name))
      cpSync(join(stage, name), join(next, name), {
        recursive: true,
        errorOnExist: true,
        force: false,
      })
      if (fingerprint(join(next, name)) !== original) throw Error('Local delivery copy changed')
      return { name, sha512: original }
    })
    writeFileSync(
      join(next, marker),
      JSON.stringify({ schemaVersion: 1, purpose: 'moomole-local-artifacts', files }, null, 2) +
        '\n',
      { flag: 'wx' },
    )
    if (hadPrevious) {
      previous(output)
      renameSync(output, backup)
      movedPrevious = true
    }
    renameSync(next, output)
    installed = true
  } catch (error) {
    if (movedPrevious && !installed) {
      try {
        renameSync(backup, output)
      } catch (rollback) {
        preserve = true
        throw new AggregateError([error, rollback], `Retained delivery recovery files: ${work}`)
      }
    }
    throw error
  } finally {
    if (!preserve) rmSync(work, { recursive: true, force: true })
  }
  return output
}
