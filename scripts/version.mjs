import { readFileSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const semver =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/
export function validVersion(value) {
  return typeof value === 'string' && value.length <= 128 && !/\s/.test(value) && semver.test(value)
}

function jsonVersion(text, indent, label) {
  const fields = [
    ...text.matchAll(new RegExp(`^${indent}"version"[ \\t]*:[ \\t]*"([^"\\r\\n]*)"`, 'gm')),
  ]
  if (fields.length !== 1) throw new Error(`${label}: expected one literal version field`)
  const field = fields[0]
  const offset = field.index + field[0].lastIndexOf('"') - field[1].length
  return {
    version: field[1],
    replace: (v) => text.slice(0, offset) + v + text.slice(offset + field[1].length),
  }
}

function versionField(text, label) {
  const fields = [...text.matchAll(/^version[ \t]*=[ \t]*"([^"\r\n]*)"[ \t]*(?:#.*)?$/gm)]
  if (fields.length !== 1) throw new Error(`${label}: expected one literal version field`)
  const field = fields[0]
  const offset = field.index + field[0].indexOf('"') + 1
  return {
    version: field[1],
    replace: (next) => text.slice(0, offset) + next + text.slice(offset + field[1].length),
  }
}

function cargoPackage(text) {
  const sections = [...text.matchAll(/^\[package\][ \t]*(?:#.*)?$/gm)]
  if (sections.length !== 1) throw new Error('Cargo.toml: expected one [package] section')
  const start = sections[0].index + sections[0][0].length
  const rest = text.slice(start)
  const next = rest.search(/^\[/m)
  const end = next < 0 ? text.length : start + next
  const field = versionField(text.slice(start, end), 'Cargo.toml [package]')
  return {
    version: field.version,
    replace: (v) => text.slice(0, start) + field.replace(v) + text.slice(end),
  }
}

function cargoLock(text) {
  const blocks = text.split(/(?=^\[\[package\]\])/m)
  const matching = blocks.flatMap((block, index) =>
    /^name[ \t]*=[ \t]*"moomole"[ \t]*$/m.test(block) ? [index] : [],
  )
  if (matching.length !== 1) throw new Error('Cargo.lock: expected one local moomole package')
  const index = matching[0]
  if (/^source[ \t]*=/m.test(blocks[index]))
    throw new Error('Cargo.lock: moomole must be a local package')
  const field = versionField(blocks[index], 'Cargo.lock moomole')
  return {
    version: field.version,
    replace: (v) => blocks.map((block, i) => (i === index ? field.replace(v) : block)).join(''),
  }
}

// Validate every target before writing any mirror. package.json is the sole
// editable version source; lockfiles and native manifests remain checked in.
export function versionPlan(root) {
  const sourceText = readFileSync(resolve(root, 'package.json'), 'utf8')
  const source = JSON.parse(sourceText)
  if (source.name !== 'moomole' || !validVersion(source.version))
    throw new Error('package.json: invalid MooMole SemVer')
  const version = source.version
  const files = []
  function add(path, parse) {
    const before = readFileSync(resolve(root, path), 'utf8')
    const field = parse(before)
    files.push({ path, before, current: field.version, after: field.replace(version) })
  }
  add('package-lock.json', (text) => {
    const json = JSON.parse(text)
    if (
      json.name !== 'moomole' ||
      json.packages?.['']?.name !== 'moomole' ||
      json.lockfileVersion !== 3
    )
      throw new Error('package-lock.json: unsupported root package or lock format')
    const sections = [...text.matchAll(/^    "": \{\r?\n[\s\S]*?^    \},?$/gm)]
    if (sections.length !== 1) throw new Error('package-lock.json: ambiguous root package layout')
    const section = sections[0]
    const local = jsonVersion(section[0], '      ', 'package-lock.json packages[""]')
    return {
      version: [json.version, json.packages[''].version].every((v) => v === version)
        ? version
        : `${json.version} / ${json.packages[''].version}`,
      replace: (v) =>
        jsonVersion(
          text.slice(0, section.index) +
            local.replace(v) +
            text.slice(section.index + section[0].length),
          '  ',
          'package-lock.json',
        ).replace(v),
    }
  })
  add('src-tauri/tauri.conf.json', (text) => {
    const json = JSON.parse(text)
    if (typeof json.version !== 'string')
      throw new Error('tauri.conf.json: expected a literal version')
    return jsonVersion(text, '  ', 'tauri.conf.json')
  })
  add('src-tauri/Cargo.toml', cargoPackage)
  add('src-tauri/Cargo.lock', cargoLock)
  const lock = files.find((file) => file.path === 'package-lock.json')
  const digest = (text) => createHash('sha256').update(text).digest('hex')
  const path = 'third-party/npm-source-origins.json'
  const before = readFileSync(resolve(root, path), 'utf8')
  const origins = JSON.parse(before)
  const fields = [...before.matchAll(/^  "packageLockSha256": "([a-f0-9]{64})"/gm)]
  if (
    origins.schemaVersion !== 1 ||
    origins.purpose !== 'npm-source-input-origins' ||
    fields.length !== 1 ||
    fields[0][1] !== origins.packageLockSha256
  )
    throw new Error('npm-source-origins.json: invalid lockfile binding')
  const sourceBindingMatches = origins.packageLockSha256 === digest(lock.before)
  const offset = fields[0].index + fields[0][0].lastIndexOf('"') - fields[0][1].length
  files.push({
    path,
    before,
    current: sourceBindingMatches ? version : 'package-lock SHA-256 mismatch',
    after: before.slice(0, offset) + digest(lock.after) + before.slice(offset + 64),
  })
  return { version, files, sourceText, sourceBindingMatches }
}

export function checkVersion(root, tag) {
  const plan = versionPlan(root)
  const mismatches = plan.files.filter((file) => file.current !== plan.version)
  if (mismatches.length)
    throw new Error(
      `Version mismatch (package.json = ${plan.version}):\n${mismatches.map((file) => `  ${file.path}: ${file.current}`).join('\n')}\nRun npm run version:sync.`,
    )
  if (tag != null && tag !== `v${plan.version}`)
    throw new Error(`Release tag must be v${plan.version}; received ${tag}`)
  return plan.version
}

export function syncVersion(root) {
  const plan = versionPlan(root)
  // A version-only sync may renew an already-verified lockfile binding. Never
  // approve dependency edits or a stale source inventory by replacing its hash.
  if (!plan.sourceBindingMatches)
    throw new Error(
      'npm source lockfile binding is stale; review dependency/source changes before updating it.',
    )
  // Refuse edits made after preparing this plan. This is a repository tool,
  // not a multi-file transaction; interrupted sync can be rerun from Git.
  if (readFileSync(resolve(root, 'package.json'), 'utf8') !== plan.sourceText)
    throw new Error('package.json changed during version sync')
  for (const file of plan.files)
    if (readFileSync(resolve(root, file.path), 'utf8') !== file.before)
      throw new Error(`${file.path} changed during version sync`)
  for (const file of plan.files)
    if (file.before !== file.after) writeFileSync(resolve(root, file.path), file.after)
  return checkVersion(root)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [mode = 'check', flag, tag, ...extra] = process.argv.slice(2)
    if (
      !['check', 'sync'].includes(mode) ||
      extra.length ||
      (mode === 'sync' ? flag != null : flag != null && (flag !== '--tag' || tag == null))
    )
      throw new Error('Usage: node scripts/version.mjs check [--tag vX.Y.Z] | sync')
    const root = resolve(import.meta.dirname, '..')
    const version = mode === 'sync' ? syncVersion(root) : checkVersion(root, tag)
    console.log(`MooMole version ${version} ${mode === 'sync' ? 'synchronized' : 'verified'}.`)
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
