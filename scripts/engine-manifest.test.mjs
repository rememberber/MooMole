import assert from 'node:assert/strict'
import { after, test } from 'node:test'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import {
  checkMachO,
  requiredFiles,
  sourceVersion,
  verifyPrepared,
  writeManifest,
} from './engine-manifest.mjs'
import { prepareTransaction } from './engine-transaction.mjs'

const root = mkdtempSync(join(tmpdir(), 'moomole-engine-tests-'))
after(() => rmSync(root, { recursive: true, force: true }))
const work = () => mkdtempSync(join(root, 'case-'))
function fixture(folder, content = 'fixture') {
  mkdirSync(folder, { recursive: true })
  for (const name of requiredFiles) {
    const path = join(folder, name)
    mkdirSync(dirname(path), { recursive: true })
    if (name.endsWith('-go')) {
      const header = Buffer.alloc(8)
      header.writeUInt32LE(0xfeedfacf)
      header.writeUInt32LE(process.arch === 'arm64' ? 0x0100000c : 0x01000007, 4)
      writeFileSync(path, header)
      chmodSync(path, 0o755)
    } else writeFileSync(path, name === 'mole' ? 'VERSION="1.59.1"\n' : content)
  }
  return writeManifest(folder, `go version go1.26.0 darwin/${process.arch}`)
}

test('manifest detects equal-size corruption, unexpected files, missing files, links and old schemas', () => {
  for (const failure of [
    'changed',
    'extra',
    'missing',
    'link',
    'old',
    'rules-v1',
    'rules-v2',
    'clean-v1',
  ]) {
    const folder = work()
    fixture(folder)
    assert.equal(verifyPrepared(folder).version, '1.59.1')
    const manifest = verifyPrepared(folder)
    manifest.files = Object.fromEntries(Object.entries(manifest.files).reverse())
    writeFileSync(join(folder, 'UPSTREAM.json'), JSON.stringify(manifest))
    assert.equal(verifyPrepared(folder).version, '1.59.1')
    if (failure === 'changed') writeFileSync(join(folder, 'mo'), 'changed')
    if (failure === 'extra') writeFileSync(join(folder, 'unexpected'), '')
    if (failure === 'missing' || failure === 'link') rmSync(join(folder, 'mo'))
    if (failure === 'link') symlinkSync('mole', join(folder, 'mo'))
    if (failure === 'old') writeFileSync(join(folder, 'UPSTREAM.json'), '{}')
    if (failure === 'rules-v1') {
      manifest.rulesProtocol = 1
      writeFileSync(join(folder, 'UPSTREAM.json'), JSON.stringify(manifest))
    }
    if (failure === 'rules-v2' || failure === 'clean-v1') {
      manifest[failure === 'clean-v1' ? 'cleanProtocol' : 'rulesProtocol'] =
        failure === 'clean-v1' ? 1 : 2
      writeFileSync(join(folder, 'UPSTREAM.json'), JSON.stringify(manifest))
    }
    assert.throws(() => verifyPrepared(folder), undefined, failure)
  }
})

test('version extraction and architecture checks reject ambiguous or unusable executables', () => {
  for (const text of ['', 'VERSION="$(echo 1.2.3)"', 'VERSION="1.2.3"\nVERSION="1.2.3"'])
    assert.throws(() => sourceVersion(text))
  const folder = work()
  fixture(folder)
  const binary = join(folder, 'bin/status-go')
  assert.throws(() => checkMachO(binary, process.arch === 'arm64' ? 'x64' : 'arm64'))
  chmodSync(binary, 0o644)
  assert.throws(() => verifyPrepared(folder))
})

test('failed builds, failed validation and failed publication preserve the old complete engine', () => {
  for (const failure of ['build', 'validation', 'publish']) {
    const parent = work()
    fixture(join(parent, 'mole'), 'old')
    const before = readFileSync(join(parent, 'mole/UPSTREAM.json'))
    assert.throws(() =>
      prepareTransaction(
        parent,
        (stage) => {
          fixture(stage, 'new')
          if (failure === 'build') throw new Error('injected build failure')
          if (failure === 'validation') writeFileSync(join(stage, 'mo'), 'bad')
        },
        () => {
          throw new Error('injected publication failure')
        },
      ),
    )
    assert.deepEqual(readFileSync(join(parent, 'mole/UPSTREAM.json')), before)
    assert.equal(readFileSync(join(parent, 'mole/mo'), 'utf8'), 'old')
    assert.deepEqual(readdirSync(parent), ['mole'])
  }
})

test('concurrent preparations and foreign destinations are refused without replacing them', () => {
  const parent = work()
  assert.throws(
    () =>
      prepareTransaction(
        parent,
        () =>
          prepareTransaction(
            parent,
            () => {},
            () => {},
          ),
        () => {},
      ),
    /locked/,
  )
  assert.deepEqual(readdirSync(parent), [])
  mkdirSync(join(parent, 'mole'))
  writeFileSync(join(parent, 'mole/keep'), 'not an engine')
  assert.throws(() =>
    prepareTransaction(
      parent,
      (stage) => fixture(stage),
      () => assert.fail('must not publish'),
    ),
  )
  assert.equal(readFileSync(join(parent, 'mole/keep'), 'utf8'), 'not an engine')
})

test(
  'native publication atomically installs and exchanges real fixture directories',
  { skip: process.platform !== 'darwin' },
  () => {
    const parent = work()
    const binary = join(root, 'engine-install')
    const compiled = spawnSync(
      '/usr/bin/cc',
      [
        '-Wall',
        '-Wextra',
        '-Werror',
        '-O2',
        resolve(import.meta.dirname, 'engine-install.c'),
        '-o',
        binary,
      ],
      { encoding: 'utf8' },
    )
    assert.equal(compiled.status, 0, compiled.stderr)
    const publish = (base, stage) => {
      const result = spawnSync(binary, [base, stage], { encoding: 'utf8' })
      assert.equal(result.status, 0, result.stderr)
    }
    prepareTransaction(parent, (stage) => fixture(stage, 'first'), publish)
    prepareTransaction(
      parent,
      (stage) => {
        fixture(stage, 'second')
        assert.equal(readFileSync(join(parent, 'mole/mo'), 'utf8'), 'first')
      },
      publish,
    )
    assert.equal(readFileSync(join(parent, 'mole/mo'), 'utf8'), 'second')
    assert.equal(verifyPrepared(join(parent, 'mole')).schemaVersion, 1)
    assert.deepEqual(readdirSync(parent), ['mole'])
    rmSync(join(parent, 'mole'), { recursive: true })
    const outside = work()
    symlinkSync(outside, join(parent, 'mole'))
    assert.throws(() => prepareTransaction(parent, (stage) => fixture(stage), publish))
    assert.ok(existsSync(join(parent, 'mole')))
  },
)
