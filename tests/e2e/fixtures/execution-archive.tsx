import { useState, StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { ExecutionArchiveCenter } from '../../../src/components/ExecutionArchiveCenter'
import type { ArchiveTransport, ArchiveKind } from '../../../src/lib/execution-archive'
import { interrupted } from './execution-history'
import { detailRaw, requestId } from '../../fixtures/uninstall'
import { stoppedRefreshDetailRaw } from '../../fixtures/quicklook'
export function mount(mode = 'normal') {
  const archiveId = 'a1000000-0000-4000-8000-000000000001',
    at = Date.now()
  const cleanup = { ...structuredClone(interrupted), reviewedAt: at }
  const uninstall = { ...detailRaw(requestId, 'uncertain'), reviewedAt: at }
  const maintenance = {
    ...stoppedRefreshDetailRaw('timedOut'),
    reviewedAt: at,
    retainedForReview: false,
  }
  const details = { cleanup, uninstall, maintenance }
  const summaries = Object.entries(details).map(([kind, detail]) => {
    const summary = { ...detail } as Record<string, unknown>
    for (const k of [
      'items',
      'events',
      'revision',
      'snapshotSha256',
      'protocol',
      'receipt',
      'reason',
    ])
      delete summary[k]
    return { kind, id: detail.id, summary }
  })
  const header = {
    format: 'MooMole execution evidence',
    version: 1,
    id: archiveId,
    createdAt: at,
    counts: { cleanup: 1, uninstall: 1, maintenance: 1 },
    payloadSha256: 'a'.repeat(64),
    payloadBytes: 12345,
  }
  const preview = {
    counts: header.counts,
    fingerprint: header.payloadSha256,
    reservedBytes: 1024 * 1024,
  }
  const calls: { command: string; args: unknown }[] = []
  let saved = mode.startsWith('existing'),
    changed = 0,
    reject = mode === 'save-error',
    release: (() => void) | null = null
  const call = (command: string, args: unknown = {}) => {
    calls.push({ command, args })
  }
  const transport: ArchiveTransport = {
    prepare: async () => {
      call('prepare')
      if (mode === 'unknown') throw Error('请先核查未知结果，再归档。')
      return preview
    },
    archive: async (fingerprint) => {
      call('archive', { fingerprint })
      if (reject) throw Error('保存归档失败，原记录保持。')
      saved = true
      return header
    },
    list: async () => {
      call('list')
      return { archives: saved ? [header] : [], complete: true, unreadable: 0 }
    },
    view: async (id) => {
      call('view', { id })
      return {
        header:
          mode === 'existing-invalid'
            ? { ...header, id: 'a1000000-0000-4000-8000-000000000099' }
            : header,
        entries: summaries,
      }
    },
    detail: async (id, kind, runId) => {
      call('detail', { id, kind, runId })
      if (mode === 'existing-slow')
        await new Promise<void>((r) => {
          release = r
        })
      return details[kind]
    },
    import: async () => {
      call('import')
      saved = true
      return header
    },
    export: async (id) => {
      call('export', { id })
      return true
    },
    reveal: async () => {
      call('reveal')
    },
  }
  Object.assign(window, {
    archiveFixture: {
      calls,
      allowSave: () => {
        reject = false
      },
      release: () => release?.(),
      changed: () => changed,
    },
  })
  const host = document.createElement('div')
  host.id = 'archive-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px'
  document.body.append(host)
  function Fixture() {
    const [signal, setSignal] = useState(0)
    return (
      <>
        <button onClick={() => setSignal((v) => v + 1)}>打开执行归档</button>
        <ExecutionArchiveCenter
          openSignal={signal}
          enabled
          transport={transport}
          onChanged={async () => {
            changed++
          }}
        />
      </>
    )
  }
  createRoot(host).render(
    <StrictMode>
      <Fixture />
    </StrictMode>,
  )
}
declare global {
  interface Window {
    archiveFixture: {
      calls: { command: string; args: unknown }[]
      allowSave: () => void
      release: () => void
      changed: () => number
    }
  }
}
