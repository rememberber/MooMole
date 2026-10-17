import { spawn } from 'node:child_process'
import { toolchainEnv } from './env.mjs'
import { checkVersion } from './version.mjs'
import { fileURLToPath } from 'node:url'

checkVersion(fileURLToPath(new URL('../', import.meta.url)))

const child = spawn('node_modules/.bin/tauri', process.argv.slice(2), {
  stdio: 'inherit',
  env: toolchainEnv(),
})
child.on('error', (error) => {
  console.error(error.message)
  process.exitCode = 1
})
child.on('exit', (code) => {
  process.exitCode = code ?? 1
})
