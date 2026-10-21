import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { catalog, verifyProjectCatalog } from './project-artifacts-catalog.mjs'
test('project source review detects code and target drift without executing shell', () => {
  const root = mkdtempSync(join(tmpdir(), 'moomole-project-catalog-'))
  try {
    const reference = structuredClone(catalog)
    for (const path of ['bin', 'lib/clean']) mkdirSync(join(root, path), { recursive: true })
    writeFileSync(join(root, 'mole'), `VERSION="${catalog.version}"\n`)
    const source = {
      'bin/purge.sh': '# never execute\n',
      'lib/clean/project.sh': '# reviewed rules\n',
      'lib/clean/purge_shared.sh': `readonly MOLE_PURGE_TARGETS=(\n${catalog.targets.map((name) => `    "${name}"`).join('\n')}\n)\n`,
    }
    for (const [path, body] of Object.entries(source)) {
      writeFileSync(join(root, path), body)
      reference.sourceHashes[path] = createHash('sha256').update(body).digest('hex')
    }
    assert.equal(verifyProjectCatalog(root, reference), 34)
    for (const changed of [
      { ...reference, revision: 'other' },
      { ...reference, version: '2.0.0' },
      { ...reference, sourceHashes: {} },
      { ...reference, targets: reference.targets.slice(1) },
    ])
      assert.throws(() => verifyProjectCatalog(root, changed))
    writeFileSync(join(root, 'lib/clean/project.sh'), '# changed\n')
    assert.throws(() => verifyProjectCatalog(root, reference), /requires source review/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
