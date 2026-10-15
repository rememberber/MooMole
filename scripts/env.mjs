import { existsSync } from 'node:fs'
import { resolve, delimiter } from 'node:path'

export function toolchainEnv() {
  const root = resolve(import.meta.dirname, '..')
  const localCargo = resolve(root, '.cache/cargo')
  return {
    ...process.env,
    ...(existsSync(localCargo)
      ? { CARGO_HOME: localCargo, RUSTUP_HOME: resolve(root, '.cache/rustup') }
      : {}),
    PATH: [resolve(localCargo, 'bin'), resolve(root, '.cache/toolchains/go/bin'), process.env.PATH]
      .filter(Boolean)
      .join(delimiter),
  }
}
