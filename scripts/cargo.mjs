import { spawn } from 'node:child_process'
import { toolchainEnv } from './env.mjs'

const child = spawn('cargo', process.argv.slice(2), {
  cwd: new URL('../src-tauri/', import.meta.url),
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
