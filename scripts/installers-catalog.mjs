import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { revision, sourceVersion } from './engine-manifest.mjs'
export const installerCatalog = JSON.parse(
  readFileSync(new URL('../src/lib/installers-catalog.json', import.meta.url), 'utf8'),
)
export function verifyInstallerCatalog(source, reference = installerCatalog) {
  const paths = Object.keys(reference.sourceHashes)
  if (
    reference.revision !== revision ||
    sourceVersion(readFileSync(resolve(source, 'mole'), 'utf8')) !== reference.version ||
    paths.join('|') !== 'bin/installer.sh'
  )
    throw new Error('Installer baseline or source coverage mismatch')
  const script = readFileSync(resolve(source, 'bin/installer.sh'), 'utf8')
  if (
    createHash('sha256').update(script).digest('hex') !== reference.sourceHashes['bin/installer.sh']
  )
    throw new Error('Installer source requires review')
  const extensions = [...script.matchAll(/\*\.([a-z]+)(?=\s*\||\s*\))/g)].map((m) => m[1])
  const unique = [...new Set(extensions)]
  if (
    JSON.stringify(unique) !== JSON.stringify(reference.extensions) ||
    Number(script.match(/^readonly INSTALLER_SCAN_MAX_DEPTH_DEFAULT=(\d+)$/m)?.[1]) !==
      reference.maxDepth ||
    reference.maxDepth !== 2
  )
    throw new Error('Installer extension or depth coverage mismatch')
  const zipTypes = script.match(/\\\.\(([a-z|]+)\)/)?.[1].split('|')
  if (
    Number(script.match(/^readonly MAX_ZIP_ENTRIES=(\d+)$/m)?.[1]) !== reference.zipWindow ||
    reference.zipWindow !== 50 ||
    JSON.stringify(zipTypes) !== JSON.stringify(reference.zipHintTypes)
  )
    throw new Error('Installer ZIP window or hint coverage mismatch')
  return unique.length
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3)
    throw new Error('Usage: node scripts/installers-catalog.mjs SOURCE_ROOT')
  console.log(
    `Verified ${verifyInstallerCatalog(resolve(process.argv[2]))} installer extensions against pinned source`,
  )
}
