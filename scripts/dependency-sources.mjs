import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { basename, dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { toolchainEnv } from './env.mjs'
import { checkVersion, validVersion } from './version.mjs'
import { verifyPrepared } from './engine-manifest.mjs'
import { prepareNotices, releasePackages } from './dependency-notices.mjs'
import { fileDigest } from './app-artifact.mjs'

const sha = (bytes, algorithm = 'sha256', encoding = 'hex') =>
  createHash(algorithm).update(bytes).digest(encoding)
const limit = 128 * 1024 ** 2
const sourceKinds = {
  'cargo-published-crate': ['cargo:', 'sha256', 'crates.io'],
  'npm-published-distribution': ['npm:', 'npm-sha512', 'registry.npmjs.org'],
  'go-published-module': ['go:', 'go-h1', 'proxy.golang.org'],
  'rust-standard-library-source': ['stdlib:rust@', 'sha256', 'static.rust-lang.org'],
  'go-toolchain-source': ['stdlib:go@', 'sha256', 'go.dev'],
}
function readArchive(file) {
  const before = lstatSync(file)
  if (!before.isFile() || before.isSymbolicLink() || !before.size || before.size > limit)
    throw Error('Source artifact is not a bounded regular file')
  const bytes = readFileSync(file),
    after = lstatSync(file)
  if (
    before.dev !== after.dev ||
    before.ino !== after.ino ||
    before.size !== after.size ||
    before.mtimeMs !== after.mtimeMs ||
    bytes.length !== before.size
  )
    throw Error('Source artifact changed while reading')
  return bytes
}
function run(program, args, cwd, env) {
  const r = spawnSync(program, args, {
    cwd,
    env,
    encoding: 'utf8',
    timeout: 120000,
    maxBuffer: 32 * 1024 ** 2,
  })
  if (r.error || r.status !== 0) throw Error(`${program} failed: ${r.error?.message ?? r.stderr}`)
  return r.stdout.trim()
}
export function cargoChecksums(text) {
  if (!/^version = [34]$/m.test(text)) throw Error('Unsupported Cargo.lock format')
  const result = new Map()
  for (const block of text
    .split(/(?=^\[\[package\]\])/m)
    .filter((b) => b.startsWith('[[package]]'))) {
    const field = (name) => {
      const m = [...block.matchAll(new RegExp(`^${name} = "([^"\\r\\n]+)"$`, 'gm'))]
      if (m.length !== 1) throw Error('Ambiguous Cargo lock entry')
      return m[0][1]
    }
    if (!/^source = /m.test(block)) continue
    const name = field('name'),
      version = field('version'),
      source = field('source'),
      checksum = field('checksum'),
      key = `${name}@${version}|${source}`
    if (
      source !== 'registry+https://github.com/rust-lang/crates.io-index' ||
      !/^[a-f0-9]{64}$/.test(checksum) ||
      result.has(key)
    )
      throw Error('Unsupported or duplicate Cargo source')
    result.set(key, checksum)
  }
  return result
}
function authentic(bytes, verification) {
  if (verification.algorithm === 'sha256') {
    if (!/^[a-f0-9]{64}$/.test(verification.value) || sha(bytes) !== verification.value)
      throw Error('Published source SHA-256 mismatch')
  } else if (verification.algorithm === 'npm-sha512') {
    if (verification.value !== 'sha512-' + sha(bytes, 'sha512', 'base64'))
      throw Error('Published npm integrity mismatch')
  } else if (verification.algorithm !== 'go-h1') throw Error('Unsupported source verification')
}
export function dependencyAppBinding(reportPath, metadata) {
  if (!reportPath) return null
  const reportMeta = lstatSync(reportPath)
  if (
    !reportMeta.isFile() ||
    reportMeta.isSymbolicLink() ||
    reportMeta.nlink !== 1 ||
    reportMeta.size > 1024 ** 2
  )
    throw Error('App report is not a bounded private regular file')
  const bytes = readArchive(reportPath),
    report = JSON.parse(bytes)
  if (
    report.schemaVersion !== 1 ||
    report.profile !== 'release' ||
    report.identifier !== 'io.github.rememberber.moomole' ||
    metadata.source?.dirty !== false ||
    report.source?.dirty !== false ||
    !/^[a-f0-9]{40}$/.test(metadata.source?.revision ?? '') ||
    report.source?.revision !== metadata.source.revision ||
    report.version !== metadata.version ||
    report.architecture !== metadata.arch ||
    report.engineRevision !== metadata.engineRevision ||
    !validVersion(metadata.engineVersion) ||
    report.engineVersion !== metadata.engineVersion ||
    !/^[a-f0-9]{64}$/.test(metadata.noticesSha256 ?? '') ||
    report.dependencyMaterials?.noticesSha256 !== metadata.noticesSha256 ||
    report.dependencyMaterials?.packages !== metadata.expectedIds.length + 2
  )
    throw Error(
      'App report does not match clean source, engine, architecture or dependency notices',
    )
  const archive = report.archive
  if (
    archive?.file !== `MooMole-${metadata.version}-mac-${metadata.arch}.app.zip` ||
    archive.sha512Encoding !== 'hex' ||
    !Number.isSafeInteger(archive.size) ||
    archive.size <= 0 ||
    archive.size > limit ||
    !/^[a-f0-9]{128}$/.test(archive.sha512 ?? '')
  )
    throw Error('App archive metadata is invalid')
  const archivePath = join(dirname(resolve(reportPath)), archive.file),
    meta = lstatSync(archivePath)
  if (!meta.isFile() || meta.isSymbolicLink() || meta.nlink !== 1 || meta.size !== archive.size)
    throw Error('App archive is missing or changed')
  const actual = readArchive(archivePath)
  if (sha(actual, 'sha512') !== archive.sha512)
    throw Error('App archive digest does not match report')
  return {
    reportSha512: sha(bytes, 'sha512'),
    sourceRevision: metadata.source.revision,
    architecture: metadata.arch,
    noticesSha256: metadata.noticesSha256,
    archive: { file: archive.file, size: actual.length, sha512: archive.sha512 },
  }
}

export function writeDependencyPacket({ output, metadata, inputs, verifyGo, appReport }) {
  if (
    !validVersion(metadata.version) ||
    !['arm64', 'x64'].includes(metadata.arch) ||
    !/^[a-f0-9]{40}$/.test(metadata.engineRevision) ||
    !Array.isArray(metadata.expectedIds) ||
    !metadata.expectedIds.length
  )
    throw Error('Invalid dependency packet metadata')
  mkdirSync(output, { mode: 0o700 })
  try {
    const binaryBinding = dependencyAppBinding(appReport, metadata)
    const work = mkdtempSync(join(output, '.packet-')),
      prefix = `MooMole-${metadata.version}-mac-${metadata.arch}-dependencies`
    if (!/^[\w.+-]+$/.test(prefix)) throw Error('Invalid dependency packet name')
    const folder = join(work, prefix),
      archives = join(folder, 'archives')
    mkdirSync(archives, { recursive: true })
    const ids = new Set(),
      files = new Set(),
      records = []
    let total = 0
    for (const input of inputs) {
      const kind = sourceKinds[input.kind],
        origin = new URL(input.origin)
      if (
        !kind ||
        !input.id.startsWith(kind[0]) ||
        input.verification.algorithm !== kind[1] ||
        origin.protocol !== 'https:' ||
        origin.hostname !== kind[2] ||
        origin.username ||
        origin.password ||
        origin.port ||
        origin.search ||
        origin.hash ||
        (input.kind === 'go-published-module' && input.id !== 'go:' + input.module)
      )
        throw Error('Source kind, verification or origin mismatch')
      if (
        typeof input.id !== 'string' ||
        /[\x00-\x20\x7f]/.test(input.id) ||
        ids.has(input.id) ||
        !/^[\w.+-]+$/.test(input.file) ||
        ['.', '..'].includes(input.file) ||
        files.has(input.file)
      )
        throw Error('Duplicate or unsafe source artifact identity')
      ids.add(input.id)
      files.add(input.file)
      const bytes = readArchive(input.path)
      total += bytes.length
      if (total > 512 * 1024 ** 2 || inputs.length > 1024)
        throw Error('Dependency packet exceeds budget')
      authentic(bytes, input.verification)
      const destination = join(archives, input.file)
      writeFileSync(destination, bytes, { flag: 'wx', mode: 0o644 })
      let proof = null
      if (input.verification.algorithm === 'go-h1') {
        proof = verifyGo(destination, input.module, input.verification.value)
        if (
          proof.version !== 1 ||
          proof.sum !== input.verification.value ||
          !Number.isSafeInteger(proof.files) ||
          proof.files < 1 ||
          !Number.isSafeInteger(proof.bytes) ||
          proof.bytes < 0
        )
          throw Error('Go source proof does not match expected module')
      }
      const digest = { size: bytes.length, sha512: sha(bytes, 'sha512') }
      const copied = fileDigest(destination)
      if (copied.sha512 !== digest.sha512 || copied.size !== digest.size)
        throw Error('Copied source changed during verification')
      records.push({
        id: input.id,
        kind: input.kind,
        file: 'archives/' + input.file,
        origin: input.origin,
        verification: input.verification,
        ...digest,
        ...(proof ? { goContent: proof } : {}),
      })
    }
    const expected = new Set(metadata.expectedIds)
    if (
      expected.size !== metadata.expectedIds.length ||
      expected.size !== ids.size ||
      [...expected].some((id) => !ids.has(id))
    )
      throw Error('Source artifacts do not cover the declared dependency inventory')
    records.sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    const manifest = {
      ...metadata,
      binaryBinding,
      schemaVersion: 1,
      purpose: 'moomole-dependency-artifacts',
      records,
      preferredSourceReview: 'pending',
      licenseReview: 'pending',
      releaseAcceptance: 'pending',
    }
    writeFileSync(join(folder, 'DEPENDENCIES.json'), JSON.stringify(manifest, null, 2) + '\n', {
      flag: 'wx',
    })
    writeFileSync(
      join(folder, 'README.txt'),
      `MooMole dependency artifacts\n\nKeep this packet with the matching application and Mole source archives. Verify the SHA-512 entries in DEPENDENCIES.json before extracting any provider archive. Cargo artifacts match Cargo.lock SHA-256, npm distribution tarballs match package-lock integrity, Go zip contents match the engine's embedded h1, and standard-library source archives match their versioned distribution metadata.\n\nThese are original published artifacts, not a recursive copy of a user's caches. Published npm or generated binding packages are not automatically the preferred form for modification; preferred-source, full standard-library dependency closure, licensing and release review remain pending. No scripts are run from these archives by this packager. This packet does not prove clean-machine installation, Gatekeeper, execution acceptance or bit-reproducible builds.\n`,
      { flag: 'wx' },
    )
    const archive = join(output, prefix + '.tar.gz')
    run('tar', ['-czf', archive, '-C', work, prefix], work, {
      ...process.env,
      COPYFILE_DISABLE: '1',
    })
    const extracted = join(work, 'verify')
    mkdirSync(extracted)
    run('tar', ['-xzf', archive, '-C', extracted], work, { ...process.env, COPYFILE_DISABLE: '1' })
    const actualFolder = join(extracted, prefix),
      actual = JSON.parse(readFileSync(join(actualFolder, 'DEPENDENCIES.json'), 'utf8'))
    if (JSON.stringify(actual) !== JSON.stringify(manifest))
      throw Error('Dependency manifest changed in archive')
    if (readdirSync(actualFolder).sort().join('|') !== 'DEPENDENCIES.json|README.txt|archives')
      throw Error('Unexpected files in dependency packet')
    const names = readdirSync(join(actualFolder, 'archives')).sort()
    if (JSON.stringify(names) !== JSON.stringify([...files].sort()))
      throw Error('Dependency packet file inventory changed')
    for (const r of records) {
      const d = fileDigest(join(actualFolder, r.file))
      if (d.size !== r.size || d.sha512 !== r.sha512)
        throw Error('Dependency archive did not preserve source bytes')
    }
    if (JSON.stringify(dependencyAppBinding(appReport, metadata)) !== JSON.stringify(binaryBinding))
      throw Error('App binding changed while packaging sources')
    const report = {
      schemaVersion: 1,
      status: 'verified',
      file: basename(archive),
      ...fileDigest(archive),
      sha512Encoding: 'hex',
      records: records.length,
      source: metadata.source,
      binaryBinding,
      reviewStatus: 'pending',
    }
    writeFileSync(join(output, 'validation.json'), JSON.stringify(report, null, 2) + '\n', {
      flag: 'wx',
    })
    rmSync(work, { recursive: true })
    return { manifest, report }
  } catch (error) {
    rmSync(output, { recursive: true, force: true })
    throw error
  }
}
async function httpsBytes(url, hosts, max = limit) {
  for (let attempt = 0; attempt < 4; attempt++) {
    const parsed = new URL(url)
    if (
      parsed.protocol !== 'https:' ||
      !hosts.includes(parsed.hostname) ||
      parsed.username ||
      parsed.password ||
      parsed.port
    )
      throw Error('Unexpected source distribution origin')
    const response = await fetch(url, { redirect: 'manual', signal: AbortSignal.timeout(120000) })
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      const next = response.headers.get('location')
      await response.body?.cancel()
      if (!next) throw Error('Source redirect missing location')
      url = new URL(next, url).href
      continue
    }
    if (response.status !== 200) throw Error(`Source download failed (${response.status})`)
    let size = 0
    const parts = []
    for await (const bytes of response.body) {
      size += bytes.length
      if (size > max) {
        await response.body.cancel().catch(() => {})
        throw Error('Source response exceeds budget')
      }
      parts.push(bytes)
    }
    return Buffer.concat(parts)
  }
  throw Error('Source redirect budget exceeded')
}
async function cachedSource(root, descriptor, offline) {
  const cache = join(root, '.cache/source-distribution')
  mkdirSync(cache, { recursive: true })
  const file = join(cache, descriptor.sha256 + '.archive')
  if (!lstatSync(file, { throwIfNoEntry: false })) {
    if (offline)
      throw Error('Standard-library source is not cached; prepare once without --offline')
    const bytes = await httpsBytes(descriptor.url, descriptor.hosts)
    if (sha(bytes) !== descriptor.sha256 || (descriptor.size && bytes.length !== descriptor.size))
      throw Error('Standard-library source checksum mismatch')
    writeFileSync(file, bytes, { flag: 'wx', mode: 0o644 })
  }
  const bytes = readArchive(file)
  if (sha(bytes) !== descriptor.sha256 || (descriptor.size && bytes.length !== descriptor.size))
    throw Error('Cached standard-library source changed')
  return file
}
function rustSourceDescriptor(text, version) {
  const blocks = text.split(/(?=^\[)/m),
    table = blocks.find((b) => b.startsWith('[pkg.rust-src.target."*"]\n'))
  if (
    !table ||
    !blocks.find((b) => b.startsWith('[pkg.rust-src]\n'))?.includes(`version = "${version} `)
  )
    throw Error('Rust source metadata does not match compiler')
  const field = (name) => {
    const m = table.match(new RegExp(`^${name} = "([^"\\r\\n]+)"$`, 'm'))
    if (!m) throw Error('Rust source descriptor missing')
    return m[1]
  }
  const url = field('xz_url'),
    checksum = field('xz_hash')
  if (
    !url.startsWith('https://static.rust-lang.org/dist/') ||
    !url.endsWith(`/rust-src-${version}.tar.xz`) ||
    !/^[a-f0-9]{64}$/.test(checksum)
  )
    throw Error('Unexpected Rust source descriptor')
  return {
    url,
    sha256: checksum,
    hosts: ['static.rust-lang.org'],
    file: `rust-src-${version}.tar.xz`,
  }
}
export async function prepareDependencySources(root, output, offline = false, appReport) {
  const version = checkVersion(root),
    env = { ...toolchainEnv(), GOFLAGS: '', GOTOOLCHAIN: 'local' },
    book = prepareNotices(root, offline),
    engine = verifyPrepared(join(root, 'src-tauri/resources/mole'))
  const watched = [
    'package.json',
    'package-lock.json',
    'src-tauri/Cargo.toml',
    'src-tauri/Cargo.lock',
    'src-tauri/tauri.conf.json',
    'src-tauri/resources/mole/UPSTREAM.json',
    'src-tauri/resources/legal/THIRD_PARTY_NOTICES.json',
  ]
  const initial = Object.fromEntries(
    watched.map((file) => [file, sha(readFileSync(join(root, file)))]),
  )
  const unchanged = () =>
    watched.every((file) => sha(readFileSync(join(root, file))) === initial[file])
  const target = { arm64: 'aarch64-apple-darwin', x64: 'x86_64-apple-darwin' }[process.arch]
  if (!target) throw Error('Unsupported source target')
  const metadata = JSON.parse(
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
  const sums = cargoChecksums(readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8'))
  const temp = mkdtempSync(join(root, '.cache/dependency-source-'))
  try {
    const inputs = []
    for (const p of releasePackages(metadata)) {
      const registry = dirname(dirname(p.manifest_path)),
        src = dirname(registry),
        registryRoot = dirname(src),
        checksum = sums.get(`${p.name}@${p.version}|${p.source}`)
      if (
        basename(src) !== 'src' ||
        basename(registryRoot) !== 'registry' ||
        !checksum ||
        !/^[A-Za-z0-9_-]+$/.test(p.name) ||
        !/^[A-Za-z0-9.+-]+$/.test(p.version)
      )
        throw Error('Unsupported Cargo source location')
      inputs.push({
        id: `cargo:${p.name}@${p.version}`,
        kind: 'cargo-published-crate',
        file: `cargo-${p.name}-${p.version}.crate`,
        path: join(registryRoot, 'cache', basename(registry), `${p.name}-${p.version}.crate`),
        origin: `https://crates.io/crates/${p.name}/${p.version}`,
        verification: { algorithm: 'sha256', value: checksum },
      })
    }
    const lock = JSON.parse(readFileSync(join(root, 'package-lock.json'), 'utf8')),
      npmFolder = join(temp, 'npm')
    mkdirSync(npmFolder)
    for (const [location, p] of Object.entries(lock.packages)) {
      if (!location || p.dev) continue
      const name = location.slice(location.lastIndexOf('node_modules/') + 'node_modules/'.length),
        url = new URL(p.resolved)
      if (
        url.protocol !== 'https:' ||
        url.hostname !== 'registry.npmjs.org' ||
        url.username ||
        url.password ||
        url.port ||
        url.search ||
        url.hash ||
        !p.integrity?.startsWith('sha512-')
      )
        throw Error('Unsupported npm source origin')
      const data = JSON.parse(
        run(
          'npm',
          [
            'pack',
            '--ignore-scripts',
            '--offline',
            '--json',
            '--pack-destination',
            npmFolder,
            url.href,
          ],
          root,
          env,
        ),
      )
      if (
        data.length !== 1 ||
        !/^[\w.+-]+$/.test(data[0].filename) ||
        data[0].name !== name ||
        data[0].version !== p.version
      )
        throw Error('npm pack identity mismatch')
      const id = `npm:${name}@${p.version}`
      inputs.push({
        id,
        kind: 'npm-published-distribution',
        file: `npm-${sha(id).slice(0, 16)}.tgz`,
        path: join(npmFolder, data[0].filename),
        origin: url.href,
        verification: { algorithm: 'npm-sha512', value: p.integrity },
      })
    }
    const goEnv = { ...env, GOPROXY: 'off', GOSUMDB: 'off' },
      goModules = new Map()
    for (const name of ['analyze', 'status']) {
      const build = JSON.parse(
        run(
          'go',
          ['version', '-m', '-json', join(root, `src-tauri/resources/mole/bin/${name}-go`)],
          root,
          env,
        ),
      )
      for (const p of build.Deps) {
        const id = `go:${p.Path}@${p.Version}`
        if (p.Replace || (goModules.has(id) && goModules.get(id).Sum !== p.Sum))
          throw Error('Unsupported or conflicting Go source')
        goModules.set(id, p)
      }
    }
    for (const [id, p] of goModules) {
      const download = JSON.parse(
        run(
          'go',
          ['mod', 'download', '-json', p.Path + '@' + p.Version],
          join(root, '.cache/mole-source'),
          goEnv,
        ),
      )
      if (download.Sum !== p.Sum || !download.Zip) throw Error('Missing exact Go module source zip')
      inputs.push({
        id,
        kind: 'go-published-module',
        module: p.Path + '@' + p.Version,
        file: `go-${sha(id).slice(0, 16)}.zip`,
        path: download.Zip,
        origin: `https://proxy.golang.org/${p.Path.replace(/[A-Z]/g, (c) => '!' + c.toLowerCase())}/@v/${p.Version.replace(/[A-Z]/g, (c) => '!' + c.toLowerCase())}.zip`,
        verification: { algorithm: 'go-h1', value: p.Sum },
      })
    }
    const sysroot = run('rustc', ['--print', 'sysroot'], root, env),
      rust = rustSourceDescriptor(
        readFileSync(join(sysroot, 'lib/rustlib/multirust-channel-manifest.toml'), 'utf8'),
        book.rustVersion,
      )
    const cache = join(root, '.cache/source-distribution')
    mkdirSync(cache, { recursive: true })
    const goMetaFile = join(cache, book.goVersion + '.json')
    let go
    if (lstatSync(goMetaFile, { throwIfNoEntry: false }))
      go = JSON.parse(readFileSync(goMetaFile, 'utf8'))
    else {
      if (offline) throw Error('Go source descriptor is not cached')
      const index = JSON.parse(
        (
          await httpsBytes('https://go.dev/dl/?mode=json&include=all', ['go.dev'], 8 * 1024 ** 2)
        ).toString('utf8'),
      )
      go = index
        .find((p) => p.version === book.goVersion)
        ?.files.find((p) => p.kind === 'source' && p.filename === book.goVersion + '.src.tar.gz')
      if (!go) throw Error('Exact Go source release unavailable')
      writeFileSync(goMetaFile, JSON.stringify(go, null, 2) + '\n', { flag: 'wx' })
    }
    if (
      go.version !== book.goVersion ||
      go.filename !== book.goVersion + '.src.tar.gz' ||
      go.kind !== 'source' ||
      !/^[a-f0-9]{64}$/.test(go.sha256) ||
      !Number.isSafeInteger(go.size) ||
      go.size <= 0 ||
      go.size > limit
    )
      throw Error('Unexpected Go source descriptor')
    const goDescriptor = {
      url: 'https://go.dev/dl/' + go.filename,
      sha256: go.sha256,
      size: go.size,
      hosts: ['go.dev', 'dl.google.com'],
      file: go.filename,
    }
    const stdlibs = await Promise.allSettled([
      cachedSource(root, rust, offline),
      cachedSource(root, goDescriptor, offline),
    ])
    for (const r of stdlibs) if (r.status === 'rejected') throw r.reason
    inputs.push(
      {
        id: `stdlib:rust@${book.rustVersion}`,
        kind: 'rust-standard-library-source',
        file: rust.file,
        path: stdlibs[0].value,
        origin: rust.url,
        verification: { algorithm: 'sha256', value: rust.sha256 },
      },
      {
        id: `stdlib:go@${book.goVersion}`,
        kind: 'go-toolchain-source',
        file: go.filename,
        path: stdlibs[1].value,
        origin: goDescriptor.url,
        verification: { algorithm: 'sha256', value: go.sha256 },
      },
    )
    const helper = join(temp, 'verify-go-module')
    run('go', ['build', '-o', helper, join(root, 'scripts/verify-go-module.go')], root, {
      ...env,
      GOOS: 'darwin',
      GOARCH: process.arch === 'x64' ? 'amd64' : 'arm64',
    })
    if (!unchanged()) throw Error('Build inputs changed while collecting sources')
    verifyPrepared(join(root, 'src-tauri/resources/mole'))
    const sourceState = () => ({
      revision: run('git', ['rev-parse', 'HEAD'], root, env),
      dirty: !!run('git', ['status', '--porcelain', '--untracked-files=normal'], root, env),
    })
    const source = sourceState()
    const result = writeDependencyPacket({
      output,
      metadata: {
        version,
        arch: engine.arch,
        engineRevision: engine.revision,
        engineVersion: engine.version,
        source,
        inputSha256: initial,
        noticesSha256: book.notices.sha256,
        expectedIds: book.packages
          .filter((p) => !['app', 'engine'].includes(p.id.split(':')[0]))
          .map((p) => p.id),
        separateSources: ['application Git archive', 'fixed Mole Git archive'],
        standardLibraryDependencyClosure: 'pending',
      },
      inputs,
      appReport,
      verifyGo: (file, module, sum) =>
        JSON.parse(run(helper, ['-zip', file, '-module', module, '-sum', sum], root, env)),
    })
    if (!unchanged() || JSON.stringify(sourceState()) !== JSON.stringify(source)) {
      rmSync(output, { recursive: true })
      throw Error('Build inputs changed while packaging sources')
    }
    return result
  } finally {
    rmSync(temp, { recursive: true, force: true })
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2)
    let output,
      appReport,
      offline = false
    for (let i = 0; i < args.length; i++) {
      if (args[i] === '--offline' && !offline) offline = true
      else if (args[i] === '--output' && !output && args[i + 1] && !args[i + 1].startsWith('--'))
        output = resolve(args[++i])
      else if (
        args[i] === '--app-report' &&
        !appReport &&
        args[i + 1] &&
        !args[i + 1].startsWith('--')
      )
        appReport = resolve(args[++i])
      else
        throw Error(
          'Usage: node scripts/dependency-sources.mjs --output NEW_DIRECTORY [--offline] [--app-report validation.json]',
        )
    }
    if (!output) throw Error('A fresh output directory is required')
    if (lstatSync(output, { throwIfNoEntry: false })) throw Error('Output directory already exists')
    const result = await prepareDependencySources(
      resolve(dirname(fileURLToPath(import.meta.url)), '..'),
      output,
      offline,
      appReport,
    )
    console.log(
      `Verified ${result.report.records} published dependency artifacts. Preferred-source and complete release review remain pending.`,
    )
  } catch (e) {
    console.error(e.message)
    process.exitCode = 1
  }
}
