import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import {
  ZipExtractionCenter,
  ZipExtractionHistory,
} from '../../../src/components/ZipExtractionCenter'
// Browser IPC fixture only: never extracts or opens user files.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'zip-extraction-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
    requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd',
    now = Date.now(),
    createdAt = now - (mode === 'expired' ? 120001 : 0)
  const draft = {
    id,
    root: '/Users/demo/Archives/Trip.zip',
    outputPath: '/Users/demo/Extracted/Trip',
    files: 3,
    directories: 3,
    hidden: 1,
    bytes: 123456,
    metadataBytes: 4096,
    sourceSha256: 'a'.repeat(64),
    samples: [
      { name: 'Trip', directory: true },
      { name: 'Trip/.notes', directory: false },
      { name: 'Trip/Empty', directory: true },
      { name: 'Trip/Nested', directory: true },
      { name: 'Trip/Nested/photo.txt', directory: false },
      { name: 'Trip/overview.txt', directory: false },
    ],
    createdAt,
    expiresAt: createdAt + 120000,
  }
  let record: any = {
    id,
    requestId,
    draft,
    temporaryPath: `/Users/demo/Extracted/.MooMole-zip-extract-${id}.partial`,
    createdAt: now,
    finishedAt: now,
    state: 'completed',
    phase: 'finished',
    artifact: { device: '1', inode: '20', bytes: 123456, sha256: 'b'.repeat(64) },
    reason: 'owned ZIP extraction result',
    reviewedAt: null,
  }
  let records: any[] =
      mode === 'unknown'
        ? [{ ...record, state: 'unknown', phase: 'publishing' }]
        : mode === 'resume'
          ? [{ ...record, state: 'running', phase: 'building', finishedAt: null, artifact: null }]
          : [],
    active = mode === 'resume' ? [requestId] : [],
    release: ((v?: unknown) => void) | null = null
  const calls: { command: string; args: Record<string, unknown> }[] = []
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        host.dataset.executions = String(
          calls.filter((c) => c.command === 'execute_zip_extraction').length,
        )
        if (command === 'get_zip_extractions') return { records, activeRequests: active }
        if (command === 'prepare_zip_extraction') {
          if (mode === 'refused')
            throw Error('owned folder contains protected content; no partial archive')
          if (mode === 'checkingCancel')
            return new Promise((_, reject) => {
              release = () => reject(Error('owned folder preparation cancelled'))
            })
          return { ...draft, ...(mode === 'mismatch' ? { root: '/Users/demo/Other' } : {}) }
        }
        if (command === 'execute_zip_extraction') {
          record = { ...record, requestId: String(args.requestId) }
          if (mode === 'cancel')
            return new Promise((resolve) => {
              release = () => {
                records = [{ ...record, state: 'cancelled', artifact: null }]
                resolve(records[0])
              }
            })
          records = [record]
          if (mode === 'badResult')
            return { ...record, draft: { ...draft, sourceSha256: 'c'.repeat(64) } }
          return record
        }
        if (command === 'cancel_task') {
          release?.()
          if (mode === 'resume') {
            records = [{ ...records[0], state: 'interrupted', finishedAt: Date.now() }]
            active = []
          }
          return null
        }
        if (command === 'acknowledge_zip_extraction') {
          records = records.map((r) => (r.id === args.id ? { ...r, reviewedAt: Date.now() } : r))
          return null
        }
        if (command === 'discard_zip_extraction' || command === 'reveal_path') return null
        if (command === 'plugin:dialog|open')
          return String((args.options as any)?.title ?? args.title).includes('目标')
            ? '/Users/demo/Extracted'
            : draft.root
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [signal, setSignal] = useState(0),
      [busy, setBusy] = useState(false),
      [epoch, setEpoch] = useState(0)
    return (
      <>
        <button disabled={busy} onClick={() => setSignal((v) => v + 1)}>
          解压 ZIP…
        </button>
        <p role="status">{busy ? 'fixture busy' : 'fixture idle'}</p>
        <ZipExtractionCenter
          openSignal={signal}
          home="/Users/demo"
          onActiveChange={setBusy}
          onOutcome={() => setEpoch((v) => v + 1)}
          onHistory={() => {}}
        />
        <ZipExtractionHistory key={epoch} />
      </>
    )
  }
  host.dataset.calls = '[]'
  host.dataset.executions = '0'
  createRoot(host).render(
    <StrictMode>
      <Fixture />
    </StrictMode>,
  )
}
