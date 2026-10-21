import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { installerCatalog, verifyInstallerCatalog } from './installers-catalog.mjs'
test('installer source review binds extensions and depth without executing its lifecycle', () => {
  const root = mkdtempSync(join(tmpdir(), 'moomole-installers-'))
  try {
    mkdirSync(join(root, 'bin'))
    writeFileSync(join(root, 'mole'), `VERSION="${installerCatalog.version}"\n`)
    const script = `readonly MAX_ZIP_ENTRIES=50\n# \\.(app|pkg|dmg|xip)(\\/|$)\nreadonly INSTALLER_SCAN_MAX_DEPTH_DEFAULT=2\nreadonly INSTALLER_SCAN_PATHS=(\n${installerCatalog.locations.map((l) => `    "${l.template}"`).join('\n')}\n)\ncase "$file" in\n${installerCatalog.extensions.map((v) => `*.${v}`).join(' | ')}) ;;\nesac\n`
    writeFileSync(join(root, 'bin/installer.sh'), script)
    const ref = {
      ...installerCatalog,
      sourceHashes: { 'bin/installer.sh': createHash('sha256').update(script).digest('hex') },
    }
    assert.equal(verifyInstallerCatalog(root, ref), 6)
    for (const changed of [
      { ...ref, revision: 'other' },
      { ...ref, maxDepth: 3 },
      { ...ref, extensions: ['dmg'] },
      { ...ref, zipWindow: 51 },
      { ...ref, zipHintTypes: ['app'] },
      { ...ref, locations: ref.locations.slice(1) },
      { ...ref, locations: ref.locations.toReversed() },
      {
        ...ref,
        locations: ref.locations.map((l, i) =>
          i ? l : { ...l, template: '$HOME/Library/Updates' },
        ),
      },
      { ...ref, sourceHashes: {} },
    ])
      assert.throws(() => verifyInstallerCatalog(root, changed))
    writeFileSync(join(root, 'bin/installer.sh'), script + '# changed\n')
    assert.throws(() => verifyInstallerCatalog(root, ref), /requires review/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
