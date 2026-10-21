import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { resolve } from 'node:path'
import { toolchainEnv } from './env.mjs'
import { revision, writeManifest } from './engine-manifest.mjs'
import { prepareTransaction } from './engine-transaction.mjs'
import { verifyMaintenanceCatalog } from './maintenance-catalog.mjs'

const root = resolve(import.meta.dirname, '..')
const source = resolve(root, '.cache/mole-source')
const env = toolchainEnv()

function run(program, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(program, args, {
    cwd,
    env: { ...env, ...extraEnv },
    encoding: 'utf8',
    timeout: 10 * 60_000,
    maxBuffer: 8 * 1024 ** 2,
  })
  if (result.stdout) process.stdout.write(result.stdout)
  if (result.stderr) process.stderr.write(result.stderr)
  if (result.error || result.status !== 0)
    throw result.error ?? new Error(`${program} exited ${result.status}`)
  return result.stdout.trim()
}

if (process.platform !== 'darwin' || !['arm64', 'x64'].includes(process.arch))
  throw new Error('当前引擎仅支持 macOS arm64/x64。')
mkdirSync(resolve(root, '.cache'), { recursive: true })
const work = mkdtempSync(resolve(root, '.cache/mole-build-'))
try {
  const manifest = prepareTransaction(
    resolve(root, 'src-tauri/resources'),
    (destination) => {
      mkdirSync(source, { recursive: true })
      if (!existsSync(resolve(source, '.git'))) run('git', ['init', source])
      if (!process.argv.includes('--offline'))
        run('git', ['fetch', '--depth', '1', 'https://github.com/tw93/Mole.git', revision], source)
      run('git', ['checkout', '--detach', revision], source)
      if (run('git', ['rev-parse', 'HEAD'], source) !== revision)
        throw new Error('Mole source revision mismatch')
      run('git', ['diff', '--exit-code', 'HEAD'], source)
      const archive = resolve(work, 'source.tar')
      run('git', ['archive', '--format=tar', `--output=${archive}`, revision], source)
      const buildSource = resolve(work, 'source')
      mkdirSync(buildSource)
      run('/usr/bin/tar', ['-xf', archive, '-C', buildSource])
      verifyMaintenanceCatalog(buildSource)
      for (const name of ['mole', 'mo', 'bin', 'lib', 'LICENSE', 'TRADEMARK.md'])
        cpSync(resolve(buildSource, name), resolve(destination, name), {
          recursive: true,
          dereference: false,
        })
      const goVersion = run('go', ['version'])
      for (const name of ['analyze', 'status'])
        run(
          'go',
          [
            'build',
            '-mod=readonly',
            '-buildvcs=false',
            '-trimpath',
            '-o',
            resolve(destination, `bin/${name}-go`),
            `./cmd/${name}`,
          ],
          buildSource,
          {
            GOOS: 'darwin',
            GOARCH: process.arch === 'x64' ? 'amd64' : 'arm64',
            GOFLAGS: '',
            GOTOOLCHAIN: 'local',
            ...(process.argv.includes('--offline') ? { GOPROXY: 'off', GOSUMDB: 'off' } : {}),
          },
        )
      for (const name of ['mole-clean-preview.sh', 'mole-review-rules.sh'])
        cpSync(
          resolve(root, 'scripts', name),
          resolve(destination, name.replace('mole-', 'moomole-')),
        )
      writeManifest(destination, goVersion)
    },
    (parent, stage) => {
      const installer = resolve(work, 'engine-install')
      run('/usr/bin/cc', [
        '-Wall',
        '-Wextra',
        '-Werror',
        '-O2',
        resolve(root, 'scripts/engine-install.c'),
        '-o',
        installer,
      ])
      run(installer, [parent, stage])
    },
  )
  mkdirSync(resolve(root, 'third-party'), { recursive: true })
  writeFileSync(
    resolve(root, 'third-party/Mole-LICENSE'),
    readFileSync(resolve(root, 'src-tauri/resources/mole/LICENSE')),
  )
  console.log(
    `Mole v${manifest.version} (${revision.slice(0, 8)}) verified and prepared for ${process.arch}.`,
  )
} finally {
  rmSync(work, { recursive: true, force: true })
}
