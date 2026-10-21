import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { revision, sourceVersion } from './engine-manifest.mjs'
export const catalog = JSON.parse(
  readFileSync(new URL('../src/lib/project-artifacts-catalog.json', import.meta.url), 'utf8'),
)
export function verifyProjectCatalog(source, reference = catalog) {
  if (
    reference.revision !== revision ||
    sourceVersion(readFileSync(resolve(source, 'mole'), 'utf8')) !== reference.version
  )
    throw new Error('Project catalog baseline mismatch')
  const paths = ['bin/purge.sh', 'lib/clean/project.sh', 'lib/clean/purge_shared.sh']
  if (Object.keys(reference.sourceHashes).sort().join('|') !== [...paths].sort().join('|'))
    throw new Error('Project catalog source coverage mismatch')
  for (const path of paths)
    if (
      createHash('sha256')
        .update(readFileSync(resolve(source, path)))
        .digest('hex') !== reference.sourceHashes[path]
    )
      throw new Error(`Project catalog requires source review: ${path}`)
  const sourceText = readFileSync(resolve(source, paths[2]), 'utf8')
  for (const [field, declarationName] of [
    ['targets', 'MOLE_PURGE_TARGETS'],
    ['workspaceIndicators', 'MOLE_PURGE_MONOREPO_INDICATORS'],
    ['projectIndicators', 'MOLE_PURGE_PROJECT_INDICATORS'],
  ]) {
    const declaration = sourceText.match(
      new RegExp(`^readonly ${declarationName}=\\(\\n([\\s\\S]*?)^\\)`, 'm'),
    )
    const values = declaration
      ? [...declaration[1].matchAll(/^\s+"([A-Za-z0-9_.-]+)"/gm)].map((m) => m[1])
      : []
    if (
      !values.length ||
      new Set(values).size !== values.length ||
      JSON.stringify(values) !== JSON.stringify(reference[field])
    )
      throw new Error(`Project ${field} coverage mismatch`)
  }
  return reference.targets.length
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error('Usage: node scripts/project-artifacts-catalog.mjs SOURCE_ROOT')
  console.log(
    `Verified ${verifyProjectCatalog(resolve(process.argv[2]))} project artifact names against pinned source`,
  )
}
