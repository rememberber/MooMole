import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { toolchainEnv } from './env.mjs'
import { checkVersion } from './version.mjs'
import {
  command,
  fileDigest,
  signAppCopy,
  verifySignedApp,
  writeArtifactReport,
  assertProductionPolicy,
} from './app-artifact.mjs'

const root = resolve(import.meta.dirname, '..')
if (process.platform !== 'darwin' || !['arm64', 'x64'].includes(process.arch))
  throw new Error('This local App recipe requires macOS arm64/x64.')
if (process.argv.slice(2).some((arg) => arg !== '--offline') || process.argv.slice(2).length > 1)
  throw new Error('Usage: npm run build:local-app -- [--offline]')
const offline = process.argv.includes('--offline')
const version = checkVersion(root)
const config = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
assertProductionPolicy(config)
const env = toolchainEnv()
// This route never imports a certificate or submits notarization credentials.
for (const key of [
  'APPLE_ID',
  'APPLE_PASSWORD',
  'APPLE_CERTIFICATE',
  'APPLE_CERTIFICATE_PASSWORD',
  'APPLE_API_ISSUER',
  'APPLE_API_KEY',
  'APPLE_API_KEY_PATH',
  'APPLE_TEAM_ID',
  'APPLE_SIGNING_IDENTITY',
])
  delete env[key]
function run(program, args) {
  const result = spawnSync(program, args, {
    cwd: root,
    env,
    stdio: 'inherit',
    timeout: 45 * 60_000,
  })
  if (result.error || result.status !== 0)
    throw new Error(
      `${program} failed (${result.status}): ${result.error?.message ?? 'see output above'}`,
    )
}
run(process.execPath, ['scripts/prepare-engine.mjs', ...(offline ? ['--offline'] : [])])
run(process.execPath, [
  'scripts/tauri.mjs',
  'build',
  '--ci',
  '--no-sign',
  '--bundles',
  'app',
  '--',
  '--locked',
  ...(offline ? ['--offline'] : []),
])
const expected = { version, arch: process.arch, identifier: config.identifier }
const output = join(root, 'local-artifacts')
mkdirSync(output, { recursive: true })
const stage = mkdtempSync(join(output, `MooMole-${version}-mac-${process.arch}-`))
const app = join(stage, 'MooMole.app')
const report = signAppCopy(
  join(root, 'src-tauri/target/release/bundle/macos/MooMole.app'),
  app,
  expected,
)
const zipName = `MooMole-${version}-mac-${process.arch}.app.zip`
command('/usr/bin/ditto', [
  '-c',
  '-k',
  '--sequesterRsrc',
  '--keepParent',
  app,
  join(stage, zipName),
])
const unpacked = mkdtempSync(join(stage, '.archive-check-'))
try {
  command('/usr/bin/ditto', ['-x', '-k', join(stage, zipName), unpacked])
  verifySignedApp(join(unpacked, 'MooMole.app'), expected)
} finally {
  rmSync(unpacked, { recursive: true, force: true })
}
verifySignedApp(app, expected)
const git = command('/usr/bin/git', ['-C', root, 'rev-parse', 'HEAD']).stdout.trim()
const dirty = !!command('/usr/bin/git', [
  '-C',
  root,
  'status',
  '--porcelain',
  '--untracked-files=normal',
]).stdout.trim()
const probe = (program, args) => {
  const result = spawnSync(program, args, { env, encoding: 'utf8', timeout: 10_000 })
  if (result.error || result.status !== 0) throw new Error(`Cannot record ${program} version`)
  return result.stdout.trim()
}
const metadata = {
  schemaVersion: 1,
  purpose: 'local-validation-only',
  profile: 'release',
  createdAt: new Date().toISOString(),
  source: { revision: git, dirty },
  ...report,
  toolchains: {
    node: process.version,
    npm: probe('npm', ['--version']),
    rust: probe('rustc', ['--version']),
    go: probe('go', ['version']),
    sdk: command('/usr/bin/xcrun', ['--show-sdk-version']).stdout.trim(),
    macOS: command('/usr/bin/sw_vers', ['-productVersion']).stdout.trim(),
  },
  archive: { file: zipName, ...fileDigest(join(stage, zipName)), sha512Encoding: 'hex' },
  installationValidation: 'pending',
  gatekeeperValidation: 'pending',
}
writeArtifactReport(join(stage, 'validation.json'), metadata)
console.log(
  `Local ad-hoc App verified: ${stage}\nNot notarized; no DMG or public release was created.`,
)
