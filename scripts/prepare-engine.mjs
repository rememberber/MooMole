import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { toolchainEnv } from './env.mjs'

export const revision = 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f'
const root = resolve(import.meta.dirname, '..')
const source = resolve(root, '.cache/mole-source')
const destination = resolve(root, 'src-tauri/resources/mole')
const env = toolchainEnv()

function run(program, args, cwd = root, extraEnv = {}) {
  const result = spawnSync(program, args, { cwd, env: { ...env, ...extraEnv }, stdio: 'inherit' })
  if (result.error || result.status !== 0)
    throw result.error ?? new Error(`${program} exited ${result.status}`)
}

if (process.platform !== 'darwin') throw new Error('当前引擎仅支持 macOS。')
mkdirSync(source, { recursive: true })
if (!existsSync(resolve(source, '.git'))) run('git', ['init', source])
run('git', ['fetch', '--depth', '1', 'https://github.com/tw93/Mole.git', revision], source)
run('git', ['checkout', '--detach', 'FETCH_HEAD'], source)
const head = spawnSync('git', ['rev-parse', 'HEAD'], {
  cwd: source,
  encoding: 'utf8',
}).stdout.trim()
if (head !== revision) throw new Error('Mole source revision does not match the pinned revision')
run('git', ['diff', '--exit-code', 'HEAD'], source)
mkdirSync(destination, { recursive: true })
for (const name of ['mole', 'mo', 'bin', 'lib', 'LICENSE', 'TRADEMARK.md']) {
  cpSync(resolve(source, name), resolve(destination, name), { recursive: true })
}
for (const name of ['analyze', 'status']) {
  run(
    'go',
    ['build', '-trimpath', '-o', resolve(destination, `bin/${name}-go`), `./cmd/${name}`],
    source,
  )
}
cpSync(
  resolve(root, 'scripts/mole-clean-preview.sh'),
  resolve(destination, 'moomole-clean-preview.sh'),
)
cpSync(
  resolve(root, 'scripts/mole-review-rules.sh'),
  resolve(destination, 'moomole-review-rules.sh'),
)
writeFileSync(
  resolve(destination, 'UPSTREAM.json'),
  JSON.stringify(
    {
      repository: 'https://github.com/tw93/Mole',
      revision,
      platform: process.platform,
      arch: process.arch,
      cleanProtocol: 1,
      rulesProtocol: 1,
    },
    null,
    2,
  ) + '\n',
)
mkdirSync(resolve(root, 'third-party'), { recursive: true })
writeFileSync(resolve(root, 'third-party/Mole-LICENSE'), readFileSync(resolve(source, 'LICENSE')))
console.log(`Mole ${revision.slice(0, 8)} prepared for ${process.arch}.`)
