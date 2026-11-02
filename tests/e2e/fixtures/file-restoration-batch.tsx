import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ExecutionHistory } from '../../../src/components/ExecutionHistory'
import { FileRestorationBatchCenter } from '../../../src/components/FileRestorationBatchCenter'
import type { RestorationBatchTarget } from '../../../src/lib/file-restoration-batch'
import { interrupted } from './execution-history'
import type { ExecutionDetail } from '../../../src/types'
// Browser transport only; never performs filesystem or process operations.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'batch-restoration-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const record: ExecutionDetail = {
    ...interrupted,
    origin: 'personalFiles',
    state: 'completed',
    method: 'move',
    itemCount: 3,
    movedCount: 2,
    uncertainCount: 1,
    items: [0, 1, 2].map((n) => ({
      ...interrupted.items[0],
      id: `source-${n}`,
      state: n === 2 ? 'uncertain' : 'moved',
      path: `/Users/demo/Downloads/${n}.txt`,
      targetPath: `/Users/demo/Organized/${n}.txt`,
      receipt: { ...interrupted.items[0].receipt!, destination: `/Users/demo/Organized/${n}.txt` },
    })),
    events: [],
  }
  const planId = crypto.randomUUID(),
    itemIds = [crypto.randomUUID(), crypto.randomUUID()]
  if (mode === 'software') {
    record.origin = 'softwareContent'
    record.method = 'trash'
    for (const [n, item] of record.items.entries()) {
      item.targetPath = null
      item.receipt!.destination = `/Users/demo/.Trash/${n}.txt`
    }
  }
  const calls: { command: string; args: Record<string, unknown> }[] = []
  host.dataset.calls = '[]'
  let finish: ((v: ExecutionDetail) => void) | undefined, result: ExecutionDetail
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'prepare_file_restoration_batch') {
          if (mode === 'fresh') await new Promise((resolve) => setTimeout(resolve, 100))
          const createdAt = Date.now()
          return {
            id: planId,
            sourceRunId: record.id,
            createdAt,
            expiresAt: createdAt + 30_000,
            items: record.items.slice(0, 2).map((i, n) => ({
              itemId: itemIds[n],
              sourceItemId: i.id,
              currentPath: i.receipt!.destination,
              originalPath: mode === 'mismatch' ? '/other' : i.path,
            })),
          }
        }
        if (command === 'execute_file_restoration_batch') {
          result = {
            ...record,
            id: crypto.randomUUID(),
            planId,
            origin: 'fileRestoration',
            method: 'move',
            itemCount: 2,
            movedCount: mode === 'conflict' ? 1 : 2,
            uncertainCount: 0,
            state: mode === 'conflict' ? 'partial' : 'completed',
            items: record.items.slice(0, 2).map((i, n) => ({
              ...i,
              id: itemIds[n],
              path: mode === 'result-mismatch' ? '/other' : i.receipt!.destination,
              targetPath: i.path,
              state: mode === 'conflict' && n === 0 ? 'skipped' : 'moved',
              reason: mode === 'conflict' && n === 0 ? '原位置已有同名项目' : '已移回原位置',
              receipt:
                mode === 'conflict' && n === 0 ? null : { ...i.receipt!, destination: i.path },
            })),
          }
          return mode === 'pending'
            ? new Promise<ExecutionDetail>((resolve) => {
                finish = resolve
              })
            : result
        }
        if (command === 'cancel_task')
          finish?.({
            ...result,
            state: 'cancelled',
            movedCount: 1,
            items: result.items.map((i, n) =>
              n ? { ...i, state: 'cancelled', receipt: null } : i,
            ),
          })
        else if (command !== 'discard_file_restoration') throw Error(command)
      },
    },
  })
  function Fixture() {
    const [target, setTarget] = useState<RestorationBatchTarget | null>(null),
      [active, setActive] = useState(false)
    return (
      <>
        <ExecutionHistory
          initialRecords={[record]}
          loadRecords={async () => [record]}
          loadDetail={async () => record}
          onRestoreBatch={active ? undefined : setTarget}
        />
        <FileRestorationBatchCenter
          target={target}
          onClose={() => setTarget(null)}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
