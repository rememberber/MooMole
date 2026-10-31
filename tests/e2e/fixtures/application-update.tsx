import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationLibrary } from '../../../src/components/ApplicationLibrary'
import { ApplicationUpdateCenter } from '../../../src/components/ApplicationUpdateCenter'
import type { UninstallTarget } from '../../../src/lib/app-uninstall'
import { applications } from './application-library'

// Browser-only transport fixtures. They never mutate files or launch apps.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'application-update-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const calls: { command: string; args: Record<string, unknown> }[] = []
  const now = Date.now() - (mode === 'expired' ? 120001 : 0)
  const task = '853bf826-4ae1-4695-bc66-85d49e1957b0'
  const draft = {
    id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    taskId: task,
    index: 0,
    name: applications[0].name,
    path: applications[0].path,
    replacement: '/Users/demo/Downloads/Example.app',
    bundleId: applications[0].bundle_id,
    oldVersion: '1.0',
    newVersion: '2.0',
    publisher: 'adHoc',
    bytes: 12345,
    createdAt: now,
    expiresAt: now + 120000,
  }
  let record = {
    id: draft.id,
    requestId: '7205853c-c7fd-47a9-8e83-fcebe84336dd',
    createdAt: Date.now(),
    finishedAt: Date.now(),
    draft,
    state: mode === 'history' ? 'unknown' : 'updated',
    phase: 'finished',
    reason: 'owned browser fixture result',
    backup: `/Applications/.moomole-app-update-${draft.id}/Example.app`,
    reviewedAt: null as number | null,
  }
  let release: ((value: unknown) => void) | null = null
  const add = (command: string, args: Record<string, unknown>) => {
    calls.push({ command, args })
    host.dataset.calls = JSON.stringify(calls)
    host.dataset.execute = String(
      calls.filter((c) => c.command === 'execute_application_update').length,
    )
  }
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        add(command, args)
        if (command === 'plugin:dialog|open') return draft.replacement
        if (command === 'prepare_application_update') {
          if (mode === 'checkingCancel')
            return new Promise((resolve, reject) => {
              release = () => reject(Error('owned preparation cancelled'))
            })
          return { ...draft, ...(mode === 'mismatch' ? { path: '/Applications/Other.app' } : {}) }
        }
        if (command === 'execute_application_update') {
          record = { ...record, requestId: String(args.requestId) }
          if (mode === 'cancel')
            return new Promise((resolve) => {
              release = resolve
            })
          if (mode === 'badResult') return { ...record, draft: { ...draft, newVersion: '99.0' } }
          return record
        }
        if (command === 'cancel_task') {
          release?.({ ...record, state: 'cancelled', reason: 'owned cancelled before replacement' })
          return null
        }
        if (command === 'get_application_updates') return mode === 'history' ? [record] : []
        if (command === 'acknowledge_application_update') {
          record = { ...record, reviewedAt: Date.now() }
          return null
        }
        if (command === 'discard_application_update' || command === 'reveal_path') return null
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [target, setTarget] = useState<UninstallTarget | null>(null),
      [active, setActive] = useState(false),
      [stale, setStale] = useState(false),
      [history, setHistory] = useState(false)
    return (
      <>
        <button onClick={() => setHistory(true)}>应用更新记录</button>
        <p role="status">{stale ? 'fixture stale' : 'fixture fresh'}</p>
        <ApplicationLibrary
          apps={applications.slice(0, 2)}
          home="/Users/demo"
          taskId={task}
          canControl={!active && !stale}
          onUpdate={setTarget}
        />
        <ApplicationUpdateCenter
          target={target}
          historyOpen={history}
          onClose={() => setTarget(null)}
          onHistoryClose={() => setHistory(false)}
          onActiveChange={setActive}
          onOutcome={() => setStale(true)}
        />
      </>
    )
  }
  host.dataset.calls = '[]'
  host.dataset.execute = '0'
  createRoot(host).render(<Fixture />)
}
