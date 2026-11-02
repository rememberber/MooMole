import { archivedJournals } from '../../fixtures/archived-journals'
import { journalSummary, isJournalKind } from '../../../src/lib/archived-journals'
import { useLanguage } from '../../../src/i18n/locale'
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
  const details = {
    cleanup,
    uninstall,
    maintenance,
    ...(['v4', 'v5'].includes(mode) ? archivedJournals(at) : {}),
  }
  if (mode === 'v5' && details.managedApp) {
    details.managedApp.draft.upgrade = {
      version: '2.0',
      url: 'https://example.org/owned.zip',
      sha256: 'a'.repeat(64),
      publisher: 'adHoc',
    }
    details.managedApp.state = 'upgraded'
    details.managedApp.upgradeResult = {
      version: '2.0',
      publisher: 'adHoc',
      bundleSha256: 'b'.repeat(64),
      receiptSha256: 'c'.repeat(64),
    }
  }
  const summaries = Object.entries(details).map(([kind, detail]) => {
    if (isJournalKind(kind))
      return {
        kind,
        id: detail.id,
        summary: journalSummary(kind, detail as ReturnType<typeof archivedJournals>['moleClean']),
      }
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
    version: mode === 'v5' ? 5 : ['v4', 'v5'].includes(mode) ? 4 : 1,
    id: archiveId,
    createdAt: at,
    counts: {
      cleanup: 1,
      uninstall: 1,
      maintenance: 1,
      ...(['v4', 'v5'].includes(mode) ? { moleClean: 1, extraction: 1, managedApp: 1 } : {}),
    },
    payloadSha256: 'a'.repeat(64),
    payloadBytes: 12345,
  }
  const preview = {
    counts: header.counts,
    fingerprint: header.payloadSha256,
    reservedBytes: 1024 * 1024,
  }
  const calls: { command: string; args: unknown }[] = []
  let retired = false
  let saved = mode.startsWith('existing') || mode.startsWith('retirement'),
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
      return { archives: saved && !retired ? [header] : [], complete: true, unreadable: 0 }
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
  if (mode.startsWith('retirement')) {
    const draft = {
      id: 'b1000000-0000-4000-8000-000000000001',
      header,
      path: `/Users/demo/Library/Application Support/MooMole/execution-archives/${archiveId}.moomole-history`,
      exported: '/Volumes/Owned/执行记录.moomole-history',
      bytes: header.payloadBytes + 300,
      sha256: 'b'.repeat(64),
      createdAt: at - (mode === 'retirement-expired' ? 120001 : 0),
      expiresAt: at + 120000 - (mode === 'retirement-expired' ? 120001 : 0),
    }
    const record = {
      id: draft.id,
      requestId: 'c1000000-0000-4000-8000-000000000001',
      draft,
      state: 'moved',
      phase: 'finished',
      reason: '已移入废纸篓，并核对实际目标身份与原路径范围。',
      createdAt: at,
      finishedAt: (at + 1) as number | null,
      receipt: {
        destination: `/Users/demo/.Trash/${archiveId}.moomole-history`,
        device: '16777240',
        inode: '3268991',
      } as Record<string, string> | null,
      reviewedAt: null as number | null,
    }
    let rows =
      mode === 'retirement-history-unknown'
        ? [
            {
              ...record,
              state: 'unknown',
              receipt: null,
              reason: '上次归档副本整理已中断，请核对本机副本、导出副本和废纸篓；不会重放。',
            },
          ]
        : mode === 'retirement-history-running'
          ? [{ ...record, state: 'running', phase: 'intent', finishedAt: null, receipt: null }]
          : ([] as (typeof record)[])
    let release: (() => void) | undefined
    Object.assign(window, {
      __TAURI_INTERNALS__: {
        invoke: async (command: string, args: Record<string, unknown> = {}) => {
          call(command, args)
          if (command === 'get_archive_retirements')
            return {
              records: rows,
              activeRequests: rows.filter((r) => r.state === 'running').map((r) => r.requestId),
            }
          if (command === 'prepare_archive_retirement') {
            if (mode === 'retirement-dialog-cancel') return null
            if (mode === 'retirement-prepare-stop')
              return new Promise((_, reject) => {
                release = () => reject(Error('归档副本整理已停止，尚未处理的文件保留。'))
              })
            return {
              ...draft,
              ...(mode === 'retirement-mismatch'
                ? { header: { ...header, payloadSha256: 'c'.repeat(64) } }
                : {}),
            }
          }
          if (command === 'execute_archive_retirement') {
            const result = { ...record, requestId: String(args.requestId) }
            if (mode === 'retirement-run-stop')
              return new Promise((resolve) => {
                rows = [
                  { ...result, state: 'running', phase: 'intent', finishedAt: null, receipt: null },
                ]
                release = () => {
                  rows = [
                    {
                      ...result,
                      state: 'cancelled',
                      receipt: null,
                      reason: '归档副本整理已停止，尚未处理的文件保留。',
                    },
                  ]
                  resolve(rows[0])
                }
              })
            retired = true
            rows = [result]
            return result
          }
          if (command === 'discard_archive_retirement') return null
          if (command === 'acknowledge_archive_retirement') {
            rows = rows.map((r) => (r.id === args.id ? { ...r, reviewedAt: Date.now() } : r))
            return null
          }
          if (command === 'cancel_task') {
            release?.()
            rows = rows.map((r) =>
              r.state === 'running'
                ? { ...r, state: 'cancelled', phase: 'finished', finishedAt: Date.now() }
                : r,
            )
            return null
          }
          throw Error(command)
        },
      },
    })
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
    useLanguage()
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
