import { spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { toolchainEnv } from './env.mjs'
import { verifyPrepared } from './engine-manifest.mjs'

const sha = (bytes) => createHash('sha256').update(bytes).digest('hex')
const names = /^(?:licen[cs]es?|copying|notice|copyright|authors|patents)(?:[._-].*)?$/i
const order = (a, b) => (a < b ? -1 : a > b ? 1 : 0)
function read(base, file, limit = 4 * 1024 ** 2) {
  const target = resolve(base, file),
    rel = relative(resolve(base), target)
  if (!rel || rel.startsWith(`..${sep}`) || rel === '..' || isAbsolute(rel))
    throw Error('Material path escapes its package')
  let current = resolve(base)
  if (!lstatSync(current).isDirectory() || lstatSync(current).isSymbolicLink())
    throw Error('Material package must be a physical directory')
  for (const part of rel.split(sep)) {
    current = join(current, part)
    if (lstatSync(current).isSymbolicLink()) throw Error('Material links require explicit review')
  }
  const before = lstatSync(target)
  if (!before.isFile() || before.size === 0 || before.size > limit)
    throw Error('Material must be a bounded regular file')
  const bytes = readFileSync(target),
    after = lstatSync(target)
  if (
    before.dev !== after.dev ||
    before.ino !== after.ino ||
    before.size !== after.size ||
    before.mtimeMs !== after.mtimeMs ||
    bytes.length !== before.size
  )
    throw Error('Material changed while reading')
  const text = bytes.toString('utf8')
  if (!Buffer.from(text).equals(bytes) || text.includes('\0'))
    throw Error('Material is not complete UTF-8 text')
  return { bytes, text }
}
function candidates(base, licenseFile) {
  const files = []
  function folder(directory, prefix = '', depth = 0) {
    if (depth > 4) throw Error('Material directory exceeds depth budget')
    for (const name of readdirSync(directory).sort(order)) {
      const file = prefix ? prefix + '/' + name : name,
        m = lstatSync(join(directory, name))
      if (m.isDirectory()) folder(join(directory, name), file, depth + 1)
      else files.push(file)
      if (files.length > 256) throw Error('Material entry count exceeds budget')
    }
  }
  for (const name of readdirSync(base).sort(order)) {
    if (!names.test(name)) continue
    const meta = lstatSync(join(base, name))
    if (meta.isSymbolicLink()) throw Error('Material links require explicit review')
    if (meta.isDirectory()) folder(join(base, name), name)
    else files.push(name)
  }
  if (licenseFile) files.push(isAbsolute(licenseFile) ? relative(base, licenseFile) : licenseFile)
  return [...new Set(files)].sort(order)
}
function releasePackages(metadata) {
  const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n])),
    found = new Set()
  function walk(id) {
    if (found.has(id)) return
    const node = nodes.get(id)
    if (!node) throw Error('Incomplete Cargo resolution')
    found.add(id)
    for (const dep of node.deps) if (dep.dep_kinds.some((k) => k.kind !== 'dev')) walk(dep.pkg)
  }
  walk(metadata.resolve.root)
  return metadata.packages.filter((p) => found.has(p.id) && p.id !== metadata.resolve.root)
}
export function collectNotices({
  root,
  engine,
  cargo,
  goBuilds,
  goModules,
  rustRoot,
  rustVersion,
  goRoot,
  supplements,
}) {
  const texts = new Map(),
    packages = [],
    ids = new Set()
  let total = 0
  const supplementMap = new Map(supplements.entries.map((e) => [e.id, e]))
  if (supplements.version !== 1 || supplementMap.size !== supplements.entries.length)
    throw Error('Invalid supplement catalog')
  function material(base, file, origin, role = 'license') {
    let observed
    try {
      observed = read(base, file)
    } catch (e) {
      throw Error(`${origin}/${file}: ${e.message}`)
    }
    const { bytes, text } = observed,
      hash = sha(bytes)
    if (!texts.has(hash)) {
      total += bytes.length
      if (total > 32 * 1024 ** 2) throw Error('Notices exceed total size budget')
      texts.set(hash, text)
    }
    return { name: file, sha256: hash, size: bytes.length, origin, role }
  }
  function add(entry, base, licenseFile, binding, selectedFiles) {
    if (!/^[a-z]+:[^\s\0]{1,240}$/.test(entry.id) || ids.has(entry.id))
      throw Error('Duplicate or invalid dependency identity')
    ids.add(entry.id)
    const materials = (selectedFiles ?? candidates(base, licenseFile)).map((file) => {
      const explicit =
        entry.id.startsWith('cargo:') &&
        licenseFile &&
        file === (isAbsolute(licenseFile) ? relative(base, licenseFile) : licenseFile)
      const role = /\.spdx$/i.test(file)
        ? 'metadata'
        : explicit || /(?:^|\/)licen[cs]es?(?:[._/-]|$)|(?:^|\/)copying(?:[._-]|$)/i.test(file)
          ? 'license'
          : 'notice'
      return material(base, file, entry.id, role)
    })
    const supplement = supplementMap.get(entry.id)
    if (supplement) {
      if (
        supplement.declaredLicense !== entry.declaredLicense ||
        (binding.vcs && supplement.vcs !== binding.vcs) ||
        (binding.integrity && supplement.integrity !== binding.integrity)
      )
        throw Error('Supplement no longer matches dependency source')
      for (const f of supplement.files) {
        if (
          !/^[a-f0-9]{64}\.txt$/.test(f.file) ||
          !['license', 'notice', 'reference', 'policy'].includes(f.role) ||
          !f.source.startsWith('https://')
        )
          throw Error('Invalid supplement material')
        const m = material(join(root, 'third-party/license-supplements'), f.file, f.source, f.role)
        if (m.sha256 !== f.sha256 || m.size !== f.size) throw Error('Supplement contents changed')
        materials.push(m)
      }
    }
    if (!materials.length) throw Error(`Missing license material: ${entry.id}`)
    if (!materials.some((m) => ['license', 'reference', 'policy'].includes(m.role)))
      throw Error(`Missing license text or pinned supplement: ${entry.id}`)
    packages.push({ ...entry, reviewRequired: !!supplement?.reviewRequired, materials })
  }
  add(
    {
      id: `app:moomole@${engine.appVersion}`,
      declaredLicense: 'GPL-3.0-only',
      scope: 'application-source',
    },
    root,
    'third-party/NOTICE.md',
    {},
  )
  add(
    {
      id: `engine:mole@${engine.version}`,
      declaredLicense: 'GPL-3.0',
      revision: engine.revision,
      scope: 'fixed-upstream',
    },
    join(root, 'src-tauri/resources/mole'),
    'TRADEMARK.md',
    {},
  )
  const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8'))
  for (const [location, p] of Object.entries(lock.packages)) {
    if (!location || p.dev) continue
    if (!location.startsWith('node_modules/') || p.link || !p.integrity)
      throw Error('Unsupported npm source')
    const base = resolve(root, location),
      pkg = JSON.parse(read(base, 'package.json').text)
    if (pkg.version !== p.version || typeof pkg.name !== 'string' || pkg.license !== p.license)
      throw Error('npm package differs from locked identity or license')
    add(
      {
        id: `npm:${pkg.name}@${pkg.version}`,
        declaredLicense: pkg.license ?? null,
        integrity: p.integrity,
        scope: 'production-lock-graph',
      },
      base,
      null,
      { integrity: p.integrity },
    )
  }
  for (const p of releasePackages(cargo)) {
    if (!p.source?.startsWith('registry+https://'))
      throw Error('Non-registry Rust dependency requires review')
    const base = dirname(p.manifest_path),
      supplement = supplementMap.get(`cargo:${p.name}@${p.version}`)
    const vcs = supplement ? JSON.parse(read(base, '.cargo_vcs_info.json').text).git.sha1 : null
    add(
      {
        id: `cargo:${p.name}@${p.version}`,
        declaredLicense: p.license ?? null,
        source: p.source,
        authors: p.authors,
        scope: 'release-resolution-including-build',
      },
      base,
      p.license_file,
      { vcs },
    )
  }
  const modules = new Map(),
    goVersions = new Set()
  for (const build of goBuilds) {
    if (build.Main?.Path !== 'github.com/tw93/mole') throw Error('Wrong Go main module')
    goVersions.add(build.GoVersion)
    for (const dep of build.Deps) {
      if (dep.Replace || !/^h1:[A-Za-z0-9+/]{43}=$/.test(dep.Sum ?? ''))
        throw Error('Unsupported Go module source')
      const key = dep.Path + '@' + dep.Version
      if (modules.has(key) && modules.get(key).Sum !== dep.Sum)
        throw Error('Conflicting Go module checksums')
      modules.set(key, dep)
    }
  }
  for (const [key, dep] of [...modules.entries()].sort(([a], [b]) => order(a, b))) {
    const downloaded = goModules.get(key)
    if (!downloaded?.Dir || downloaded.Sum !== dep.Sum)
      throw Error('Go module cache does not match embedded binary metadata')
    add(
      { id: `go:${key}`, declaredLicense: null, sum: dep.Sum, scope: 'embedded-engine-module' },
      downloaded.Dir,
      null,
      {},
    )
  }
  const goVersion = read(goRoot, 'VERSION').text.split('\n')[0]
  if (goVersions.size !== 1 || !goVersions.has(goVersion))
    throw Error('Go toolchain materials differ from built engine')
  add(
    { id: `stdlib:go@${goVersion}`, declaredLicense: null, scope: 'go-runtime-materials' },
    goRoot,
    null,
    {},
  )
  add(
    { id: `stdlib:rust@${rustVersion}`, declaredLicense: null, scope: 'rust-library-materials' },
    rustRoot,
    null,
    {},
    [
      'COPYRIGHT-library.html',
      ...readdirSync(join(rustRoot, 'licenses'))
        .sort(order)
        .map((name) => 'licenses/' + name),
    ],
  )
  packages.sort((a, b) => order(a.id, b.id))
  if (packages.length > 1024) throw Error('Dependency count exceeds budget')
  const manifest = {
    schemaVersion: 1,
    purpose: 'moomole-dependency-materials',
    appVersion: engine.appVersion,
    engineVersion: engine.version,
    engineRevision: engine.revision,
    platform: 'darwin',
    arch: engine.arch,
    rustVersion,
    goVersion,
    scope:
      'Includes build-time Rust dependencies and production npm graph; not an exact dead-code or compatibility analysis.',
    reviewStatus: 'pending',
    reviewRequired: packages.filter((p) => p.reviewRequired).map((p) => p.id),
    packages,
    materialCount: texts.size,
  }
  const blocks = [
    'MooMole — third-party notices and license materials',
    'These materials retain upstream wording. License compatibility, source distribution and complete release review remain pending.',
    '',
  ]
  for (const p of packages) {
    blocks.push(
      p.id,
      `Declared license: ${p.declaredLicense ?? 'not machine-declared; see original material'}`,
      `Scope: ${p.scope}`,
    )
    if (p.authors?.length) blocks.push(`Authors as declared: ${JSON.stringify(p.authors)}`)
    if (p.reviewRequired)
      blocks.push(
        'Policy/reference supplement: review required; no invented copyright attribution.',
      )
    for (const m of p.materials)
      blocks.push(`  ${m.name} — SHA-256 ${m.sha256} (${m.role}; ${m.origin})`)
    blocks.push('')
  }
  const chunks = [Buffer.from(blocks.join('\n') + '\n')],
    materials = []
  let offset = chunks[0].length
  for (const [hash, text] of [...texts.entries()].sort(([a], [b]) => order(a, b))) {
    const heading = Buffer.from(`\n=== Material SHA-256 ${hash} ===\n\n`),
      content = Buffer.from(text)
    chunks.push(heading, content, Buffer.from('\n'))
    offset += heading.length
    materials.push({ sha256: hash, offset, size: content.length })
    offset += content.length + 1
  }
  const bytes = Buffer.concat(chunks)
  if (bytes.length > 32 * 1024 ** 2) throw Error('Rendered notices exceed size budget')
  manifest.materials = materials
  manifest.notices = { file: 'THIRD_PARTY_NOTICES.txt', size: bytes.length, sha256: sha(bytes) }
  return { manifest, bytes }
}
export function verifyNotices(folder, expected) {
  if (lstatSync(folder).isSymbolicLink() || !lstatSync(folder).isDirectory())
    throw Error('Notices directory is not physical')
  if (readdirSync(folder).sort().join('|') !== 'THIRD_PARTY_NOTICES.json|THIRD_PARTY_NOTICES.txt')
    throw Error('Unexpected notices files')
  const manifest = JSON.parse(read(folder, 'THIRD_PARTY_NOTICES.json').text),
    text = read(folder, 'THIRD_PARTY_NOTICES.txt', 32 * 1024 ** 2).bytes
  if (
    manifest.schemaVersion !== 1 ||
    manifest.purpose !== 'moomole-dependency-materials' ||
    manifest.appVersion !== expected.version ||
    manifest.engineRevision !== expected.revision ||
    manifest.arch !== expected.arch ||
    manifest.notices?.file !== 'THIRD_PARTY_NOTICES.txt' ||
    manifest.notices.size !== text.length ||
    manifest.notices.sha256 !== sha(text) ||
    !Array.isArray(manifest.packages) ||
    !manifest.packages.length ||
    manifest.packages.length > 1024 ||
    !Array.isArray(manifest.materials) ||
    !manifest.materials.length ||
    manifest.materials.length !== manifest.materialCount ||
    manifest.reviewStatus !== 'pending' ||
    !Array.isArray(manifest.reviewRequired)
  )
    throw Error('Notices manifest or digest mismatch')
  const materials = new Map(),
    ids = new Set()
  let previousEnd = 0
  for (const m of manifest.materials) {
    if (
      !/^[a-f0-9]{64}$/.test(m.sha256) ||
      materials.has(m.sha256) ||
      !Number.isSafeInteger(m.offset) ||
      !Number.isSafeInteger(m.size) ||
      m.size <= 0 ||
      m.offset < previousEnd ||
      m.offset + m.size > text.length ||
      sha(text.subarray(m.offset, m.offset + m.size)) !== m.sha256
    )
      throw Error('Invalid or changed individual notice material')
    materials.set(m.sha256, m)
    previousEnd = m.offset + m.size
  }
  for (const p of manifest.packages) {
    if (
      typeof p.id !== 'string' ||
      !/^[a-z]+:[^\s\0]{1,240}$/.test(p.id) ||
      typeof p.reviewRequired !== 'boolean' ||
      ids.has(p.id) ||
      !Array.isArray(p.materials) ||
      !p.materials.length
    )
      throw Error('Invalid dependency material coverage')
    ids.add(p.id)
    for (const m of p.materials)
      if (
        !materials.has(m.sha256) ||
        materials.get(m.sha256).size !== m.size ||
        !['license', 'notice', 'metadata', 'reference', 'policy'].includes(m.role)
      )
        throw Error('Missing dependency material contents')
    if (!p.materials.some((m) => ['license', 'reference', 'policy'].includes(m.role)))
      throw Error('Only metadata/attribution was supplied without license text')
    if (p.materials.some((m) => ['policy', 'reference'].includes(m.role)) && !p.reviewRequired)
      throw Error('Policy/reference review flag missing')
  }
  if (
    JSON.stringify([...manifest.reviewRequired].sort(order)) !==
    JSON.stringify(
      manifest.packages
        .filter((p) => p.reviewRequired)
        .map((p) => p.id)
        .sort(order),
    )
  )
    throw Error('Review coverage differs from dependency records')
  for (const id of [
    `app:moomole@${expected.version}`,
    `engine:mole@${manifest.engineVersion}`,
    `stdlib:go@${manifest.goVersion}`,
    `stdlib:rust@${manifest.rustVersion}`,
  ])
    if (!ids.has(id)) throw Error('Missing application, engine or standard-library notices')
  return manifest
}
export function publishNotices(folder, result) {
  const parent = dirname(folder)
  mkdirSync(parent, { recursive: true })
  const stage = mkdtempSync(join(parent, '.legal-build-')),
    backup = join(parent, '.legal-previous-' + randomUUID())
  let moved = false
  const expected = {
    version: result.manifest.appVersion,
    revision: result.manifest.engineRevision,
    arch: result.manifest.arch,
  }
  try {
    writeFileSync(
      join(stage, 'THIRD_PARTY_NOTICES.json'),
      JSON.stringify(result.manifest, null, 2) + '\n',
      { flag: 'wx' },
    )
    writeFileSync(join(stage, 'THIRD_PARTY_NOTICES.txt'), result.bytes, { flag: 'wx' })
    verifyNotices(stage, expected)
    if (lstatSync(folder, { throwIfNoEntry: false })) {
      // Old generated versions may be different, but must prove their own
      // identity and material digest before this recipe replaces the directory.
      const old = JSON.parse(read(folder, 'THIRD_PARTY_NOTICES.json').text)
      verifyNotices(folder, {
        version: old.appVersion,
        revision: old.engineRevision,
        arch: old.arch,
      })
      renameSync(folder, backup)
      moved = true
    }
    try {
      renameSync(stage, folder)
    } catch (e) {
      if (moved) renameSync(backup, folder)
      moved = false
      throw e
    }
    if (moved) {
      const old = JSON.parse(read(backup, 'THIRD_PARTY_NOTICES.json').text)
      verifyNotices(backup, {
        version: old.appVersion,
        revision: old.engineRevision,
        arch: old.arch,
      })
      rmSync(backup, { recursive: true })
    }
  } finally {
    rmSync(stage, { recursive: true, force: true })
  }
  return verifyNotices(folder, expected)
}
function run(program, args, cwd, env) {
  const r = spawnSync(program, args, {
    cwd,
    env,
    encoding: 'utf8',
    timeout: 120_000,
    maxBuffer: 32 * 1024 ** 2,
  })
  if (r.error || r.status !== 0) throw Error(`${program} failed: ${r.error?.message ?? r.stderr}`)
  return r.stdout.trim()
}
export function prepareNotices(root, offline = false) {
  const env = toolchainEnv(),
    engine = verifyPrepared(join(root, 'src-tauri/resources/mole'))
  const app = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
  const target = { arm64: 'aarch64-apple-darwin', x64: 'x86_64-apple-darwin' }[process.arch]
  if (process.platform !== 'darwin' || !target || engine.arch !== process.arch)
    throw Error('Notices preparation requires matching macOS engine/host')
  const cargo = JSON.parse(
    run(
      'cargo',
      [
        'metadata',
        '--locked',
        ...(offline ? ['--offline'] : []),
        '--filter-platform',
        target,
        '--features',
        'custom-protocol',
        '--format-version',
        '1',
      ],
      join(root, 'src-tauri'),
      env,
    ),
  )
  const goBuilds = ['analyze', 'status'].map((n) =>
    JSON.parse(
      run(
        'go',
        ['version', '-m', '-json', join(root, `src-tauri/resources/mole/bin/${n}-go`)],
        root,
        env,
      ),
    ),
  )
  const goModules = new Map(),
    goEnv = { ...env, GOPROXY: 'off', GOSUMDB: 'off', GOTOOLCHAIN: 'local', GOFLAGS: '' }
  for (const build of goBuilds)
    for (const dep of build.Deps) {
      const key = dep.Path + '@' + dep.Version
      if (!goModules.has(key))
        goModules.set(
          key,
          JSON.parse(
            run('go', ['mod', 'download', '-json', key], join(root, '.cache/mole-source'), goEnv),
          ),
        )
    }
  const sysroot = run('rustc', ['--print', 'sysroot'], root, env),
    rustVersion = run('rustc', ['--version'], root, env)
  const goRoot = run('go', ['env', 'GOROOT'], root, env)
  const result = collectNotices({
    root,
    engine: { ...engine, appVersion: app.version },
    cargo,
    goBuilds,
    goModules,
    rustRoot: join(sysroot, 'share/doc/rust'),
    rustVersion: rustVersion.split(' ')[1],
    goRoot,
    supplements: JSON.parse(
      readFileSync(join(root, 'third-party/license-supplements.json'), 'utf8'),
    ),
  })
  return publishNotices(join(root, 'src-tauri/resources/legal'), result)
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.slice(2).some((a) => a !== '--offline'))
      throw Error('Usage: node scripts/dependency-notices.mjs [--offline]')
    const m = prepareNotices(
      resolve(dirname(fileURLToPath(import.meta.url)), '..'),
      process.argv.includes('--offline'),
    )
    console.log(
      `Prepared ${m.packages.length} dependency records and ${m.materialCount} distinct materials; ${m.reviewRequired.length} policy/reference records require review. Distribution review pending.`,
    )
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
}
