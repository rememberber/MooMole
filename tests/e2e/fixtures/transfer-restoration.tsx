import { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { FileTransferHistory } from '../../../src/components/FileTransferCenter'
import {
  TransferRestorationCenter,
  TransferRestorationHistory,
} from '../../../src/components/TransferRestorationCenter'
import type { TransferRestoreTarget } from '../../../src/lib/transfer-restoration'
// Browser-only history and native IPC fixture. No actual files are touched.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'transfer-restoration-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`,
    now = Date.now() - (mode === 'expired' ? 120001 : 0)
  const native = (window as any).__TAURI_INTERNALS__,
    original = native.invoke
  const entry = {
    id: uuid(3),
    path: '/Users/demo/Downloads/one.txt',
    target: '/Volumes/OwnedRestore/one (1).txt',
    temporaryPath: null,
    state: 'moved',
    reason: 'owned transfer fixture',
    copyReceipt: { destination: '/Volumes/OwnedRestore/one (1).txt', device: '2', inode: '22' },
    sourceTrashReceipt: { destination: '/Users/demo/.Trash/one.txt', device: '1', inode: '11' },
    sha256: 'a'.repeat(64),
    metadataSha256: 'b'.repeat(64),
  }
  const source = {
    protocol: 1,
    revision: 'MooMole.cross-volume-move.v1',
    id: uuid(2),
    requestId: uuid(9),
    createdAt: now - 1000,
    finishedAt: now - 500,
    state: 'completed',
    items: [entry],
    reviewedAt: null,
    reason: 'owned original transfer',
  }
  const draft = {
    id: uuid(1),
    runId: source.id,
    itemId: entry.id,
    copyPath: entry.target,
    originalPath: entry.path,
    bytes: 123,
    createdAt: now,
    expiresAt: now + 120000,
  }
  let record: any = {
    id: draft.id,
    requestId: uuid(4),
    draft,
    createdAt: Date.now(),
    finishedAt: Date.now(),
    state: mode === 'unknown' ? 'unknown' : mode === 'resume' ? 'running' : 'restored',
    phase: mode === 'resume' ? 'intent' : 'finished',
    temporaryPath: `/Users/demo/Downloads/.moomole-restore-${draft.id}.tmp`,
    receipt:
      mode === 'unknown' || mode === 'resume'
        ? null
        : { destination: entry.path, device: '1', inode: '99' },
    reason: 'owned restoration fixture',
    reviewedAt: null,
  }
  let records: any[] = mode === 'unknown' || mode === 'resume' ? [record] : [],
    active = mode === 'resume' ? [record.requestId] : [],
    release: ((value: any) => void) | null = null
  const calls: { command: string; args: any }[] = []
  native.invoke = async (command: string, args: any = {}) => {
    calls.push({ command, args })
    host.dataset.calls = JSON.stringify(calls)
    host.dataset.executions = String(
      calls.filter((c) => c.command === 'execute_transfer_restoration').length,
    )
    if (command === 'get_personal_transfers') return { records: [source], activeRequests: [] }
    if (command === 'get_transfer_restorations') return { records, activeRequests: active }
    if (command === 'prepare_transfer_restoration') {
      if (mode === 'checkingCancel')
        return new Promise((resolve, reject) => {
          release = () => reject(Error('owned prepare cancelled'))
        })
      return {
        ...draft,
        ...(mode === 'mismatch' ? { originalPath: '/Users/demo/Downloads/other.txt' } : {}),
      }
    }
    if (command === 'discard_transfer_restoration') return null
    if (command === 'execute_transfer_restoration') {
      record = { ...record, requestId: args.requestId }
      records = [record]
      if (mode === 'cancel') {
        record = { ...record, state: 'running', phase: 'intent', finishedAt: null, receipt: null }
        records = [record]
        active = [args.requestId]
        return new Promise((resolve) => {
          release = resolve
        })
      }
      if (mode === 'badResult')
        return { ...record, draft: { ...draft, copyPath: '/Volumes/OwnedRestore/other.txt' } }
      return record
    }
    if (
      command === 'cancel_task' &&
      (mode === 'cancel' || mode === 'checkingCancel' || mode === 'resume')
    ) {
      if (mode !== 'checkingCancel') {
        record = {
          ...record,
          state: 'cancelled',
          phase: 'finished',
          finishedAt: Date.now(),
          receipt: null,
        }
        records = [record]
        active = []
      }
      release?.(record)
      return null
    }
    if (command === 'acknowledge_transfer_restoration') {
      record = { ...record, reviewedAt: Date.now() }
      records = [record]
      return null
    }
    if (command === 'reveal_path') {
      host.dataset.reveal = String(args.path)
      return null
    }
    return original(command, args)
  }
  function Fixture() {
    const [target, setTarget] = useState<TransferRestoreTarget | null>(null),
      [busy, setBusy] = useState(false),
      [epoch, setEpoch] = useState(0)
    return (
      <>
        <FileTransferHistory key={epoch} canRestore={!busy} onRestore={setTarget} />
        <TransferRestorationCenter
          target={target}
          onClose={() => setTarget(null)}
          onActiveChange={setBusy}
          onOutcome={() => setEpoch((v) => v + 1)}
          onHistory={() => {}}
        />
        <TransferRestorationHistory key={`restored-${epoch}`} />
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
