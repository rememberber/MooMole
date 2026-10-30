import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { DeviceBackups } from '../../../src/components/DeviceBackups'
import type { BackupView, BackupRecord, DeviceBackup } from '../../../src/lib/device-backups'
// UI/IPC fixture only. Never touches files or runs backup operations.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'device-backups-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const uuid = () => crypto.randomUUID()
  const root = '/Users/demo/BackupCopies'
  const item: DeviceBackup = {
    id: uuid(),
    path: `${root}/owned-backup`,
    name: 'owned-backup',
    deviceName: 'Owned Phone',
    product: 'iPhone16,2',
    encrypted: true,
    markedFinished: true,
    modifiedAt: Date.now() - 100000,
    size: 8192,
    entries: 24,
    eligible: true,
    reason: '状态文件标记完成；不证明备份可恢复或当前未被使用。',
  }
  const view: BackupView = {
    phase: '',
    requestId: null,
    activeRequests: [],
    error: null,
    report: null,
    records: [],
  }
  const calls: { command: string; args: any }[] = []
  let draft: any
  host.dataset.calls = '[]'
  const state = (name: string, phase: BackupRecord['phase'] = 'finished'): BackupRecord => ({
    revision: 'MooMole.device-backups.v1',
    id: draft?.id || uuid(),
    requestId: view.requestId || uuid(),
    createdAt: Date.now(),
    finishedAt: Date.now(),
    reviewedAt: null,
    state: name as BackupRecord['state'],
    phase,
    reason: name === 'moved' ? '整份备份已移入废纸篓，未清空。' : '结果待核对，不会重试。',
    plan: { id: draft?.id || '', root, item: { row: item } },
    receipt:
      name === 'moved'
        ? { destination: '/Users/demo/.Trash/owned-backup', device: '1', inode: '9007199254740993' }
        : null,
  })
  if (mode === 'existing') {
    draft = { id: uuid() }
    const record = state('unknown', 'intent')
    view.records = [record]
  }
  if (mode === 'resumed') {
    draft = { id: uuid() }
    view.phase = 'processing'
    view.requestId = uuid()
    view.activeRequests = [view.requestId]
    view.records = [state('running', 'checking')]
  }
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: any = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'get_device_backups') return structuredClone(view)
        if (command === 'scan_device_backups') {
          view.phase = 'ready'
          view.requestId = args.requestId
          view.report = {
            id: uuid(),
            root,
            createdAt: Date.now(),
            expiresAt: Date.now() + 900000,
            complete: false,
            cancelled: false,
            reason: '另有未完成项目。',
            items: [
              item,
              {
                ...item,
                id: uuid(),
                path: `${root}/incomplete`,
                name: 'incomplete',
                deviceName: 'Incomplete Phone',
                markedFinished: false,
                eligible: false,
                reason: '状态未确认完成，保留。',
              },
            ],
          }
          return
        }
        if (command === 'prepare_device_backup') {
          if (args.itemId !== item.id || args.scanId !== view.report?.id)
            throw Error('wrong selection')
          draft = {
            id: uuid(),
            expiresAt: Date.now() + 30000,
            item: { ...item, path: mode === 'mismatch' ? '/other' : item.path },
          }
          return draft
        }
        if (command === 'execute_device_backup') {
          if (args.id !== draft.id || !args.confirmed) throw Error('wrong confirmation')
          view.requestId = args.requestId
          if (mode === 'pending') {
            view.phase = 'processing'
            view.activeRequests = [args.requestId]
            view.records = [state('running', 'checking')]
          } else {
            view.phase = 'ready'
            view.records = [state('moved')]
            view.report = null
          }
          return
        }
        if (command === 'cancel_task') {
          view.activeRequests = []
          view.phase = 'ready'
          view.records = [{ ...state('cancelled'), requestId: args.taskId }]
          return
        }
        if (command === 'acknowledge_device_backup') {
          view.records[0].reviewedAt = Date.now()
          return
        }
        if (command === 'discard_device_backup') return
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [signal, setSignal] = useState(0),
      [active, setActive] = useState(false)
    return (
      <>
        <button onClick={() => setSignal((n) => n + 1)}>设备备份…</button>
        <DeviceBackups openSignal={signal} enabled={!active} onActiveChange={setActive} />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
