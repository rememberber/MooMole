import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { revision, sourceVersion } from './engine-manifest.mjs'

export const catalog = JSON.parse(
  readFileSync(new URL('../src/lib/maintenance-catalog.json', import.meta.url), 'utf8'),
)

// Only read source bytes. Never source shell code or run optimize (including dry-run).
export function verifyMaintenanceCatalog(source, reference = catalog) {
  if (
    reference.revision !== revision ||
    sourceVersion(readFileSync(resolve(source, 'mole'), 'utf8')) !== reference.version
  )
    throw new Error('Maintenance catalog source baseline mismatch')
  const paths = [
    ...['catalog', 'tasks', 'maintenance'].map((name) => `lib/optimize/${name}.sh`),
    'lib/manage/whitelist.sh',
    'lib/core/base.sh',
    'lib/core/app_protection.sh',
  ]
  if (Object.keys(reference.sourceHashes).sort().join('|') !== [...paths].sort().join('|'))
    throw new Error('Maintenance catalog source coverage mismatch')
  for (const path of paths) {
    const hash = createHash('sha256')
      .update(readFileSync(resolve(source, path)))
      .digest('hex')
    if (reference.sourceHashes[path] !== hash)
      throw new Error(`Maintenance catalog requires source review: ${path}`)
  }
  const declarations = [
    ...readFileSync(resolve(source, paths[0]), 'utf8').matchAll(
      /^_optimize_catalog_register ([a-z0-9_]+) (opt_[a-z0-9_]+) \\$/gm,
    ),
  ].map((match) => [match[1], match[2]])
  const items = reference.items.map((item) => [item.id, item.handler])
  if (declarations.length === 0 || JSON.stringify(declarations) !== JSON.stringify(items))
    throw new Error('Maintenance catalog task coverage mismatch')
  return items.length
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error('Usage: node scripts/maintenance-catalog.mjs SOURCE_ROOT')
  console.log(
    `Verified ${verifyMaintenanceCatalog(resolve(process.argv[2]))} maintenance descriptions against pinned source`,
  )
}
