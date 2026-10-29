import assert from 'node:assert/strict'
import { test } from 'node:test'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readlinkSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { createSourceBundle } from './source-bundle.mjs'

function command(program, args, cwd) {
  const r = spawnSync(program, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: '/dev/null' },
  })
  assert.equal(r.status, 0, r.stderr)
  return r.stdout.trim()
}
const git = (repo, ...args) => command('git', ['-C', repo, ...args])
function file(root, path, value, mode = 0o644) {
  mkdirSync(dirname(join(root, path)), { recursive: true })
  writeFileSync(join(root, path), value, { mode })
}
function commit(repo) {
  git(repo, 'add', '.')
  git(
    repo,
    '-c',
    'user.name=Source fixture',
    '-c',
    'user.email=source@example.invalid',
    'commit',
    '-qm',
    'source fixture',
  )
  return git(repo, 'rev-parse', 'HEAD')
}
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'moomole-source-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const app = join(root, 'app'),
    upstream = join(root, 'upstream'),
    output = join(root, 'bundle')
  mkdirSync(app)
  mkdirSync(upstream)
  git(app, 'init', '-q')
  git(upstream, 'init', '-q')
  for (const name of [
    'LICENSE',
    'TRADEMARK.md',
    'go.mod',
    'go.sum',
    'cmd/analyze/main.go',
    'cmd/status/main.go',
  ])
    file(upstream, name, `fixture ${name}\n`)
  file(upstream, 'mole', 'VERSION="1.65.0"\n', 0o755)
  file(upstream, '.claude/skills/fixture/a.md', 'upstream fixture\n')
  mkdirSync(join(upstream, '.agents/skills'), { recursive: true })
  symlinkSync('../../.claude/skills/fixture', join(upstream, '.agents/skills/fixture'))
  const engine = commit(upstream)
  const json = (value) => JSON.stringify(value, null, 2) + '\n'
  file(app, 'package.json', json({ name: 'moomole', version: '0.1.0' }))
  file(
    app,
    'package-lock.json',
    json({
      name: 'moomole',
      version: '0.1.0',
      lockfileVersion: 3,
      packages: { '': { name: 'moomole', version: '0.1.0', dependencies: {} } },
    }),
  )
  file(app, 'src-tauri/tauri.conf.json', json({ version: '0.1.0' }))
  file(app, 'src-tauri/Cargo.toml', '[package]\nname = "moomole"\nversion = "0.1.0"\n')
  file(
    app,
    'src-tauri/Cargo.lock',
    'version = 4\n\n[[package]]\nname = "moomole"\nversion = "0.1.0"\n',
  )
  file(
    app,
    'third-party/npm-source-origins.json',
    json({
      schemaVersion: 1,
      purpose: 'npm-source-input-origins',
      packageLockSha256: createHash('sha256')
        .update(readFileSync(join(app, 'package-lock.json')))
        .digest('hex'),
    }),
  )
  file(
    app,
    'scripts/engine-manifest.mjs',
    `export const revision = '${engine}'\nexport const repository = 'https://github.com/tw93/Mole'\n`,
  )
  for (const name of [
    'LICENSE',
    'third-party/NOTICE.md',
    'scripts/prepare-engine.mjs',
    'scripts/build-local-app.mjs',
  ])
    file(app, name, `fixture ${name}\n`)
  file(app, 'third-party/Mole-LICENSE', readFileSync(join(upstream, 'LICENSE')))
  file(app, 'src/space name-中文.ts', 'export const original = true\n')
  const revision = commit(app)
  return {
    root,
    app,
    upstream,
    output,
    revision,
    engine,
    build: (extra = {}) =>
      createSourceBundle({ root: app, upstream, output, ref: revision, ...extra }),
  }
}

test('exact immutable client/upstream trees preserve modes and internal links, excluding dirty and untracked data', (t) => {
  const f = fixture(t)
  file(f.app, 'src/space name-中文.ts', 'uncommitted replacement\n')
  file(f.app, 'local-artifacts/private.sqlite', 'private data\n')
  file(f.upstream, '.env', 'not exported\n')
  const report = f.build()
  assert.equal(report.binaryBinding, null)
  assert.equal(report.app.revision, f.revision)
  assert.equal(report.mole.revision, f.engine)
  assert.ok(!JSON.stringify(report).includes(f.root))
  for (const source of [report.app, report.mole]) {
    const bytes = readFileSync(join(f.output, source.file))
    assert.equal(bytes.length, source.size)
    assert.equal(createHash('sha512').update(bytes).digest('hex'), source.sha512)
    assert.ok(!source.files.some((row) => /private|\.env|\.git\//.test(row.path)))
  }
  const unpack = join(f.root, 'unpack')
  mkdirSync(unpack)
  command('tar', ['-xzf', join(f.output, report.app.file), '-C', unpack])
  assert.equal(
    readFileSync(join(unpack, 'MooMole-0.1.0-source/src/space name-中文.ts'), 'utf8'),
    'export const original = true\n',
  )
  command('tar', ['-xzf', join(f.output, report.mole.file), '-C', unpack])
  assert.equal(
    readlinkSync(
      join(unpack, `Mole-1.65.0-${f.engine.slice(0, 12)}-source/.agents/skills/fixture`),
    ),
    '../../.claude/skills/fixture',
  )
  const again = f.build({ output: join(f.root, 'again') })
  assert.equal(again.app.sha512, report.app.sha512)
  assert.equal(again.mole.sha512, report.mole.sha512)
})

test('dirty/wrong binary reports cannot bind sources; valid reports bind only selected public fields', (t) => {
  const f = fixture(t),
    path = join(f.root, 'app-report.json')
  const archive = Buffer.from('fixture archive bytes, not an executable')
  const archiveName = 'MooMole-0.1.0-mac-arm64.app.zip'
  file(f.root, archiveName, archive)
  const report = {
    schemaVersion: 1,
    profile: 'release',
    source: { revision: f.revision, dirty: false },
    version: '0.1.0',
    engineRevision: f.engine,
    engineVersion: '1.65.0',
    architecture: 'arm64',
    archive: {
      file: archiveName,
      size: archive.length,
      sha512: createHash('sha512').update(archive).digest('hex'),
      sha512Encoding: 'hex',
    },
    privatePath: f.root,
  }
  for (const changed of [
    { source: { ...report.source, dirty: true } },
    { version: '0.2.0' },
    { engineRevision: 'a'.repeat(40) },
    { source: { ...report.source, revision: 'b'.repeat(40) } },
  ]) {
    file(f.root, 'app-report.json', JSON.stringify({ ...report, ...changed }))
    assert.throws(() => f.build({ appReport: path }), /does not match/)
    assert.equal(existsSync(f.output), false)
  }
  file(f.root, 'app-report.json', JSON.stringify(report))
  const bundle = f.build({ appReport: path })
  assert.equal(bundle.binaryBinding.sourceRevision, f.revision)
  assert.ok(!JSON.stringify(bundle).includes(f.root))
  file(f.root, archiveName, Buffer.alloc(archive.length, 0))
  assert.throws(() => f.build({ appReport: path, output: join(f.root, 'changed') }), /digest/)
  assert.equal(existsSync(join(f.root, 'changed')), false)
})

test('tracked local data, escaping links and submodules refuse packaging without a partial bundle', (t) => {
  for (const kind of ['database', 'credential', 'escape', 'chain', 'cycle', 'submodule']) {
    const f = fixture(t)
    if (kind === 'database') file(f.app, 'private.sqlite', 'private')
    if (kind === 'credential') file(f.app, '.env.production', 'private')
    if (kind === 'escape') symlinkSync('../outside', join(f.app, 'escape'))
    if (kind === 'chain') {
      mkdirSync(join(f.app, 'nested'))
      symlinkSync('..', join(f.app, 'nested/back'))
      symlinkSync('back/../..', join(f.app, 'nested/escape'))
    }
    if (kind === 'cycle') {
      symlinkSync('b', join(f.app, 'a'))
      symlinkSync('a', join(f.app, 'b'))
    }
    if (kind === 'submodule')
      git(f.app, 'update-index', '--add', '--cacheinfo', `160000,${f.revision},submodule`)
    let revision
    if (kind === 'submodule') {
      git(
        f.app,
        '-c',
        'user.name=Source fixture',
        '-c',
        'user.email=source@example.invalid',
        'commit',
        '-qm',
        'gitlink fixture',
      )
      revision = git(f.app, 'rev-parse', 'HEAD')
    } else revision = commit(f.app)
    assert.throws(() => f.build({ ref: revision }), /Local data|escapes|Unsupported|cycle/)
    assert.equal(existsSync(f.output), false)
  }
})

test('Git export omissions or substitutions, version drift and reused destinations fail closed', (t) => {
  for (const kind of ['ignore', 'substitution', 'version', 'existing']) {
    const f = fixture(t)
    if (kind === 'ignore') file(f.app, '.gitattributes', 'LICENSE export-ignore\n')
    if (kind === 'substitution') {
      file(f.app, '.gitattributes', 'sub.txt export-subst\n')
      file(f.app, 'sub.txt', '$Format:%H$\n')
    }
    if (kind === 'version') file(f.app, 'src-tauri/tauri.conf.json', '{"version":"0.2.0"}\n')
    if (kind === 'existing') {
      mkdirSync(f.output)
      file(f.output, 'keep', 'keep')
    }
    const revision = kind === 'existing' ? f.revision : commit(f.app)
    assert.throws(() => f.build({ ref: revision }))
    if (kind === 'existing') assert.equal(readFileSync(join(f.output, 'keep'), 'utf8'), 'keep')
    else assert.equal(existsSync(f.output), false)
  }
})
