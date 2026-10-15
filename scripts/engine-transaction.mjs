import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { basename, join } from 'node:path'
import { verifyPrepared } from './engine-manifest.mjs'

export function prepareTransaction(parent, build, publish) {
  mkdirSync(parent, { recursive: true })
  const lock = join(parent, '.mole-prepare-lock')
  try {
    mkdirSync(lock, { mode: 0o700 })
  } catch {
    throw new Error(
      `Engine preparation is locked. Check ${lock}/owner.json; remove this lock only after confirming its process has stopped.`,
    )
  }
  let stage
  try {
    writeFileSync(
      join(lock, 'owner.json'),
      JSON.stringify({ pid: process.pid, startedAt: new Date().toISOString() }),
    )
    stage = mkdtempSync(join(parent, '.mole-stage-'))
    build(stage)
    const manifest = verifyPrepared(stage)
    const destination = join(parent, 'mole')
    if (existsSync(destination)) {
      const meta = lstatSync(destination)
      if (!meta.isDirectory() || meta.isSymbolicLink())
        throw new Error('Existing engine target is not a physical directory')
      const old = JSON.parse(readFileSync(join(destination, 'UPSTREAM.json'), 'utf8'))
      if (old.repository !== manifest.repository || !/^[0-9a-f]{40}$/.test(old.revision))
        throw new Error(
          'Existing engine is not the expected generated resource; preserve it for inspection',
        )
    }
    chmodSync(stage, 0o755)
    publish(parent, basename(stage))
    return manifest
  } finally {
    // After exchange, only this invocation's private stage holds the old engine.
    try {
      if (stage) rmSync(stage, { recursive: true, force: true })
    } finally {
      rmSync(lock, { recursive: true, force: true })
    }
  }
}
