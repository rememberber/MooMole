import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { catalog, verifyMaintenanceCatalog } from './maintenance-catalog.mjs'

test('source review gate rejects version, code and task coverage drift before engine preparation', () => {
  const root = mkdtempSync(join(tmpdir(), 'moomole-maintenance-'))
  try {
    const reference = structuredClone(catalog)
    mkdirSync(join(root, 'lib/optimize'), { recursive: true })
    writeFileSync(join(root, 'mole'), `VERSION="${catalog.version}"\n`)
    const sources = {
      'lib/optimize/catalog.sh': catalog.items
        .map((item) => `_optimize_catalog_register ${item.id} ${item.handler} \\\n`)
        .join(''),
      'lib/optimize/tasks.sh': '# reviewed handlers fixture\n',
      'lib/optimize/maintenance.sh': '# reviewed preference helper fixture\n',
    }
    for (const [path, content] of Object.entries(sources)) {
      writeFileSync(join(root, path), content)
      reference.sourceHashes[path] = createHash('sha256').update(content).digest('hex')
    }
    assert.equal(verifyMaintenanceCatalog(root, reference), 19)
    const changedVersion = { ...reference, version: '0.0.0' }
    assert.throws(() => verifyMaintenanceCatalog(root, changedVersion), /baseline mismatch/)
    assert.throws(
      () => verifyMaintenanceCatalog(root, { ...reference, revision: 'other' }),
      /baseline mismatch/,
    )
    assert.throws(
      () => verifyMaintenanceCatalog(root, { ...reference, sourceHashes: {} }),
      /source coverage mismatch/,
    )
    assert.throws(
      () => verifyMaintenanceCatalog(root, { ...reference, items: reference.items.slice(1) }),
      /task coverage mismatch/,
    )
    const changedHandler = structuredClone(reference)
    changedHandler.items[0].handler = 'opt_other'
    assert.throws(() => verifyMaintenanceCatalog(root, changedHandler), /task coverage mismatch/)
    writeFileSync(join(root, 'lib/optimize/tasks.sh'), '# changed\n')
    assert.throws(() => verifyMaintenanceCatalog(root, reference), /requires source review/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
