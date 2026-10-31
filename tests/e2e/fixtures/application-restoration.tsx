import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationUpdateCenter } from '../../../src/components/ApplicationUpdateCenter'
// Browser transport fixture only. Never swaps bundles or changes app data.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'application-restoration-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const sourceId = '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
    requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd',
    now = Date.now()
  const source = {
    id: sourceId,
    requestId,
    createdAt: now - 1000,
    finishedAt: now - 500,
    draft: {
      id: sourceId,
      taskId: id,
      index: 0,
      name: 'Owned App',
      path: '/Applications/Owned.app',
      replacement: '/Users/demo/Downloads/New.app',
      bundleId: 'dev.moomole.owned',
      oldVersion: '1.0',
      newVersion: '2.0',
      publisher: 'adHoc',
      bytes: 12345,
      createdAt: now - 2000,
      expiresAt: now + 118000,
    },
    state: 'updated',
    phase: 'finished',
    reason: 'owned update completed',
    backup: `/Applications/.moomole-app-update-${sourceId}/Owned.app`,
    reviewedAt: null,
    recoveryAvailable: mode !== 'legacy',
  }
  const createdAt = now - (mode === 'expired' ? 120001 : 0)
  const draft = {
    id,
    sourceId,
    name: 'Owned App',
    path: source.draft.path,
    retained: source.backup,
    fromVersion: '2.0',
    toVersion: '1.0',
    publisher: 'adHoc',
    bytes: 12345,
    createdAt,
    expiresAt: createdAt + 120000,
  }
  let record: any = {
    id,
    requestId,
    draft,
    createdAt: now,
    finishedAt: now,
    state: 'restored',
    phase: 'finished',
    reason: 'owned recovery result',
    receipt: {
      installedDevice: '1',
      installedInode: '20',
      retainedDevice: '1',
      retainedInode: '21',
      installedHash: 'a'.repeat(64),
      retainedHash: 'b'.repeat(64),
    },
    reviewedAt: null,
  }
  let records: any[] =
    mode === 'unknown'
      ? [{ ...record, state: 'unknown', phase: 'intent', receipt: null }]
      : mode === 'resume'
        ? [{ ...record, state: 'running', phase: 'intent', finishedAt: null, receipt: null }]
        : []
  let active = mode === 'resume' ? [requestId] : [],
    release: ((v?: unknown) => void) | null = null
  const calls: { command: string; args: Record<string, unknown> }[] = []
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        host.dataset.executions = String(
          calls.filter((c) => c.command === 'execute_application_restoration').length,
        )
        if (command === 'get_application_updates') return [source]
        if (command === 'get_application_restorations') return { records, activeRequests: active }
        if (command === 'prepare_application_restoration') {
          if (mode === 'checkingCancel')
            return new Promise((_, reject) => {
              release = () => reject(Error('owned preparation cancelled'))
            })
          return { ...draft, ...(mode === 'mismatch' ? { sourceId: id } : {}) }
        }
        if (command === 'execute_application_restoration') {
          record = { ...record, requestId: String(args.requestId) }
          if (mode === 'cancel')
            return new Promise((resolve) => {
              release = () => {
                records = [{ ...record, state: 'cancelled', receipt: null }]
                resolve(records[0])
              }
            })
          records = [record]
          if (mode === 'badResult') return { ...record, draft: { ...draft, toVersion: '99' } }
          return record
        }
        if (command === 'cancel_task') {
          release?.()
          if (mode === 'resume') {
            records = [{ ...records[0], state: 'unknown', finishedAt: Date.now() }]
            active = []
          }
          return null
        }
        if (command === 'acknowledge_application_restoration') {
          records = records.map((r) => (r.id === args.id ? { ...r, reviewedAt: Date.now() } : r))
          return null
        }
        if (command === 'discard_application_restoration' || command === 'reveal_path') return null
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [history, setHistory] = useState(false),
      [busy, setBusy] = useState(false),
      [outcome, setOutcome] = useState(false)
    return (
      <>
        <button onClick={() => setHistory(true)}>应用更新记录</button>
        <p role="status">
          {busy ? 'fixture busy' : 'fixture idle'} ·{' '}
          {outcome ? 'fixture changed' : 'fixture unchanged'}
        </p>
        <ApplicationUpdateCenter
          target={null}
          historyOpen={history}
          onClose={() => {}}
          onHistoryClose={() => setHistory(false)}
          onActiveChange={setBusy}
          onOutcome={() => setOutcome(true)}
        />
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
