import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createHash } from 'node:crypto'
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import {
  assertProductionPolicy,
  command,
  fileDigest,
  inspectApp,
  signAppCopy,
  verifySignedApp,
} from './app-artifact.mjs'
import { requiredFiles, verifyPrepared, writeManifest } from './engine-manifest.mjs'

const native = { skip: process.platform !== 'darwin' }
const expected = {
  version: '0.1.0',
  identifier: 'io.github.rememberber.moomole',
  arch: process.arch,
}
function scratch(t) {
  const root = mkdtempSync(join(tmpdir(), 'moomole-app-artifact-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  return root
}
function fixture(t) {
  const root = scratch(t),
    source = join(root, 'source.app'),
    engine = join(source, 'Contents/Resources/mole')
  mkdirSync(join(source, 'Contents/MacOS'), { recursive: true })
  const c = join(root, 'fixture.c')
  writeFileSync(c, 'int main(void) { return 0; }\n')
  const main = join(source, 'Contents/MacOS/moomole')
  command('/usr/bin/cc', ['-O2', c, '-o', main])
  writeFileSync(
    join(source, 'Contents/Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${expected.identifier}</string><key>CFBundleExecutable</key><string>moomole</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>0.1.0</string><key>CFBundleVersion</key><string>0.1.0</string></dict></plist>`,
  )
  for (const name of requiredFiles) {
    const path = join(engine, name)
    mkdirSync(dirname(path), { recursive: true })
    if (name.endsWith('-go')) cpSync(main, path)
    else writeFileSync(path, name === 'mole' ? 'VERSION="1.59.1"\n' : 'isolated fixture\n')
  }
  writeManifest(engine, `go version go1.26.0 darwin/${process.arch}`)
  return { root, source, engine, signed: join(root, 'signed.app') }
}

test('production CSP excludes dev/remote connections and unsafe script sources while allowing Tauri IPC', () => {
  const config = JSON.parse(
    readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
  )
  assertProductionPolicy(config)
  assert.ok(config.app.security.devCsp.includes('127.0.0.1:1420'))
  for (const csp of [
    null,
    'default-src *; connect-src ipc:',
    "default-src 'self'; connect-src *",
    "default-src 'self'; connect-src http://127.0.0.1:1420",
    "default-src 'self'; connect-src ipc: ws://127.0.0.1:1420",
    "default-src 'self'; connect-src ipc:; script-src 'self' 'unsafe-eval'",
    "default-src 'self'; connect-src ipc:; connect-src https:",
    "default-src 'self'; CONNECT-SRC https:; connect-src ipc:",
    "default-src 'self'; connect-src ipc:; SCRIPT-SRC https:",
  ])
    assert.throws(() => assertProductionPolicy({ app: { security: { csp } } }))
})

test('streaming archive digest preserves exact byte size and SHA-512', (t) => {
  const path = join(scratch(t), 'archive.zip')
  const content = Buffer.alloc(384 * 1024 + 17, 0x34)
  writeFileSync(path, content)
  assert.deepEqual(fileDigest(path), {
    size: content.length,
    sha512: createHash('sha512').update(content).digest('hex'),
  })
})

test(
  'signing uses a fresh copy, renews post-sign engine hashes, seals resources and preserves the source',
  native,
  (t) => {
    const f = fixture(t)
    const original = readFileSync(join(f.engine, 'UPSTREAM.json'))
    const originalGo = readFileSync(join(f.engine, 'bin/analyze-go'))
    const report = signAppCopy(f.source, f.signed, expected)
    assert.equal(report.signing, 'ad-hoc')
    assert.equal(report.notarized, false)
    assert.equal(report.signatures.length, 3)
    assert.ok(report.signatures.every((item) => item.entitlements === 'none'))
    assert.notDeepEqual(
      readFileSync(join(f.signed, 'Contents/Resources/mole/UPSTREAM.json')),
      original,
    )
    verifyPrepared(join(f.signed, 'Contents/Resources/mole'))
    assert.deepEqual(readFileSync(join(f.engine, 'UPSTREAM.json')), original)
    assert.deepEqual(readFileSync(join(f.engine, 'bin/analyze-go')), originalGo)
    verifySignedApp(f.signed, expected)
    const zip = join(f.root, 'signed.app.zip'),
      unpacked = join(f.root, 'unpacked')
    command('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', f.signed, zip])
    command('/usr/bin/ditto', ['-x', '-k', zip, unpacked])
    verifySignedApp(join(unpacked, 'signed.app'), expected)
  },
)

test(
  'final verification refuses changed native bytes, ordinary resources and forged manifests after signing',
  native,
  (t) => {
    const f = fixture(t)
    signAppCopy(f.source, f.signed, expected)
    for (const mode of ['native', 'resource', 'manifest']) {
      const app = join(f.root, `${mode}.app`)
      cpSync(f.signed, app, { recursive: true })
      const engine = join(app, 'Contents/Resources/mole')
      if (mode === 'native') {
        const p = join(engine, 'bin/status-go')
        const b = readFileSync(p)
        b[b.length - 1] ^= 1
        writeFileSync(p, b)
      }
      if (mode === 'resource') writeFileSync(join(engine, 'mo'), 'changed')
      if (mode === 'manifest') {
        writeFileSync(join(engine, 'mo'), 'changed')
        const manifest = verifyPrepared(f.engine)
        writeManifest(engine, manifest.build.goVersion)
        verifyPrepared(engine)
      }
      assert.throws(() => verifySignedApp(app, expected))
    }
  },
)

test(
  'layout checks refuse extra native code, symlinks, wrong architecture, version and reused destinations',
  native,
  (t) => {
    const f = fixture(t)
    assert.throws(() => inspectApp(f.source, { ...expected, version: '0.2.0' }), /version/)
    assert.throws(
      () => inspectApp(f.source, { ...expected, arch: process.arch === 'arm64' ? 'x64' : 'arm64' }),
      /architecture/,
    )
    assert.throws(() => signAppCopy(f.source, f.source, expected), /fresh destination/)
    assert.throws(
      () => signAppCopy(f.source, join(f.source, 'copy.app'), expected),
      /fresh destination/,
    )
    symlinkSync('missing.app', f.signed)
    assert.throws(() => signAppCopy(f.source, f.signed, expected), /fresh destination/)
    rmSync(f.signed)
    const extra = join(f.source, 'Contents/Resources/extra')
    cpSync(join(f.engine, 'bin/status-go'), extra)
    assert.throws(() => inspectApp(f.source, expected), /Unexpected/)
    rmSync(extra)
    symlinkSync('Info.plist', join(f.source, 'Contents/link'))
    assert.throws(() => inspectApp(f.source, expected), /links/)
  },
)
