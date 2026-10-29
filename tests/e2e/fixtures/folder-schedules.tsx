import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { FolderSchedules } from '../../../src/components/FolderSchedules'
import { useFolderSchedules } from '../../../src/lib/use-folder-schedules'
import type {
  FolderSchedulesView,
  FolderWatch,
  FolderScanRecord,
  FolderScheduleTransport,
} from '../../../src/lib/folder-schedules'
import '../../../src/styles.css'
const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
const root = '/Users/owned/Downloads'
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'folder-schedules-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const calls: unknown[] = []
  const call = (command: string, args?: unknown) => {
    calls.push({ command, args })
    host.dataset.calls = JSON.stringify(calls)
  }
  let view: FolderSchedulesView = JSON.parse(
    sessionStorage.getItem('owned-folder-scans') ?? 'null',
  ) ?? { watches: [], records: [], activeRequests: [], error: null }
  const save = () => sessionStorage.setItem('owned-folder-scans', JSON.stringify(view))
  if (mode === 'changes') {
    view.watches = [
      {
        id: uuid(1),
        root,
        enabled: true,
        intervalMinutes: 15,
        updatedAt: Date.now(),
        lastAttemptAt: Date.now(),
      },
    ]
    view.records = [
      {
        version: 1,
        id: uuid(2),
        watchId: uuid(1),
        generation: uuid(3),
        root,
        trigger: 'scheduled',
        startedAt: Date.now() - 1000,
        finishedAt: Date.now(),
        state: 'completed',
        summary: {
          complete: true,
          visited: 4,
          skipped: 0,
          files: 3,
          logicalBytes: 1000,
          comparison: 'compared',
          added: 1,
          changed: 1,
          notSeen: 1,
          logicalDeltaBytes: 100,
          changes: [
            { path: root + '/new.txt', kind: 'added' },
            { path: root + '/changed.txt', kind: 'changed' },
            { path: root + '/old.txt', kind: 'notSeen' },
          ],
          changesTruncated: false,
        },
        error: null,
      },
    ]
  }
  const transport: FolderScheduleTransport = {
    read: async () => {
      call('read')
      return mode === 'invalid'
        ? { watches: [], records: [{ version: 99 }] }
        : structuredClone(view)
    },
    configure: async (edit) => {
      call('configure', edit)
      const old = view.watches.find((w) => w.id === edit.id)
      const w: FolderWatch = {
        id: edit.id ?? uuid(1),
        root: edit.root,
        enabled: edit.enabled,
        intervalMinutes: edit.intervalMinutes,
        updatedAt: Date.now(),
        lastAttemptAt: old?.lastAttemptAt ?? null,
      }
      view.watches = [...view.watches.filter((w) => w.id !== edit.id), w]
      save()
      return structuredClone(w)
    },
    remove: async (id) => {
      call('remove', { id })
      view.watches = view.watches.filter((w) => w.id !== id)
      save()
    },
    scan: async (watchId, requestId) => {
      call('scan', { watchId, requestId })
      const w = view.watches.find((w) => w.id === watchId)!
      const r: FolderScanRecord = {
        version: 1,
        id: requestId,
        watchId,
        generation: uuid(3),
        root: w.root,
        trigger: 'manual',
        startedAt: Date.now(),
        finishedAt: null,
        state: 'running',
        summary: null,
        error: null,
      }
      view.records = [r, ...view.records]
      view.activeRequests = [requestId]
      w.lastAttemptAt = r.startedAt
      save()
    },
    cancel: async (id) => {
      call('cancel', { id })
      const r = view.records.find((r) => r.id === id)!
      r.state = 'cancelled'
      r.finishedAt = Date.now()
      r.summary = {
        complete: false,
        visited: 1,
        skipped: 0,
        files: 1,
        logicalBytes: 5,
        comparison: 'unavailable',
        added: null,
        changed: null,
        notSeen: null,
        logicalDeltaBytes: null,
        changes: [],
        changesTruncated: false,
      }
      view.activeRequests = []
      save()
    },
  }
  function Fixture() {
    const c = useFolderSchedules(true, transport),
      [route, setRoute] = useState('')
    return (
      <>
        <p aria-label="fixture organize root">{route}</p>
        <FolderSchedules
          enabled
          controller={c}
          chooseDirectory={async () => root}
          onOrganize={(root) => {
            call('organize', { root })
            setRoute(root)
          }}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
