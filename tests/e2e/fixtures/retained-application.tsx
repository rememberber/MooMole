import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationUpdateCenter } from '../../../src/components/ApplicationUpdateCenter'
// Browser transport only; real native operations have separate owned-fixture acceptance.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'retained-application-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const sourceId = '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
    requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd',
    now = Date.now()
  const stage = `/Applications/.moomole-app-update-${sourceId}`
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
    backup: `${stage}/Owned.app`,
    reviewedAt: null,
    recoveryAvailable: !['unknown', 'resume'].includes(mode),
    retainedCopyAvailable: !['unknown', 'resume', 'legacy'].includes(mode),
  }
  const createdAt = now - (mode === 'expired' ? 120001 : 0)
  const draft = {
    id,
    sourceId,
    name: 'Owned App',
    installed: source.draft.path,
    retained: source.backup,
    stage,
    installedVersion: mode === 'restored' ? '1.0' : '2.0',
    retainedVersion: mode === 'restored' ? '2.0' : '1.0',
    afterRestore: mode === 'restored',
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
    state: 'moved',
    phase: 'finished',
    reason: 'owned retained package moved; installed package kept',
    receipt: {
      destination: '/Users/demo/.Trash/Owned.app',
      device: '1',
      inode: '9007199254740993',
    },
    stageRemoved: true,
    reviewedAt: null,
  }
  let records: any[] =
    mode === 'unknown'
      ? [{ ...record, state: 'unknown', stageRemoved: false }]
      : mode === 'resume'
        ? [
            {
              ...record,
              state: 'running',
              phase: 'intent',
              finishedAt: null,
              stageRemoved: false,
              receipt: null,
            },
          ]
        : []
  let active = mode === 'resume' ? [requestId] : [],
    release: (() => void) | null = null
  const calls: { command: string; args: Record<string, unknown> }[] = []
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        host.dataset.executions = String(
          calls.filter((c) => c.command === 'execute_retained_application').length,
        )
        if (command === 'get_application_updates') return [source]
        if (command === 'get_application_restorations') return { records: [], activeRequests: [] }
        if (command === 'get_retained_application_history')
          return { records, activeRequests: active }
        if (command === 'prepare_retained_application') {
          if (mode === 'checkingCancel')
            return new Promise((_, reject) => {
              release = () => reject(Error('owned preparation cancelled'))
            })
          return { ...draft, ...(mode === 'mismatch' ? { sourceId: id } : {}) }
        }
        if (command === 'execute_retained_application') {
          record = { ...record, requestId: String(args.requestId) }
          if (mode === 'cancel')
            return new Promise((resolve) => {
              release = () => {
                records = [{ ...record, state: 'cancelled', stageRemoved: false, receipt: null }]
                resolve(records[0])
              }
            })
          records = [record]
          source.retainedCopyAvailable = false
          source.recoveryAvailable = false
          if (mode === 'badResult') return { ...record, draft: { ...draft, retainedVersion: '99' } }
          return record
        }
        if (command === 'cancel_task') {
          release?.()
          if (mode === 'resume') {
            records = [
              { ...records[0], state: 'unknown', phase: 'finished', finishedAt: Date.now() },
            ]
            active = []
          }
          return null
        }
        if (command === 'acknowledge_retained_application') {
          records = records.map((r) => (r.id === args.id ? { ...r, reviewedAt: Date.now() } : r))
          return null
        }
        if (['discard_retained_application', 'reveal_path'].includes(command)) return null
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [history, setHistory] = useState(false),
      [busy, setBusy] = useState(false)
    return (
      <>
        <button onClick={() => setHistory(true)}>应用更新记录</button>
        <p role="status">{busy ? 'fixture busy' : 'fixture idle'}</p>
        <ApplicationUpdateCenter
          target={null}
          historyOpen={history}
          onClose={() => {}}
          onHistoryClose={() => setHistory(false)}
          onActiveChange={setBusy}
          onOutcome={() => {}}
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
