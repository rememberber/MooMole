import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ExecutionHistory } from '../../../src/components/ExecutionHistory'
import { FileRestorationCenter } from '../../../src/components/FileRestorationCenter'
import { restorationTransport, type RestorationTarget } from '../../../src/lib/file-restoration'
import { interrupted } from './execution-history'
import type { ExecutionDetail } from '../../../src/types'
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'restoration-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const record: ExecutionDetail = {
    ...interrupted,
    origin: 'personalFiles',
    state: 'completed',
    method: 'move',
    itemCount: 1,
    movedCount: 1,
    uncertainCount: 0,
    items: [
      {
        ...interrupted.items[0],
        path: '/Users/demo/Downloads/chosen.txt',
        targetPath: '/Users/demo/Organized/chosen.txt',
        receipt: {
          ...interrupted.items[0].receipt!,
          destination: '/Users/demo/Organized/chosen.txt',
        },
      },
    ],
    events: [],
  }
  const id = '9f3123c1-95a5-4af0-aec8-0b5c82c6a2dd'
  if (mode.startsWith('software')) {
    record.origin = 'softwareContent'
    record.method = 'trash'
    record.items[0].targetPath = null
    record.items[0].receipt!.destination = '/Users/demo/.Trash/chosen.txt'
    if (mode === 'softwareLibrary')
      record.items[0].path = '/Users/demo/Library/Mail Downloads/chosen.txt'
    if (mode === 'softwareUnknown') {
      record.items[0].state = 'uncertain'
      record.uncertainCount = 1
      record.movedCount = 0
    }
  }
  const calls: { command: string; args: Record<string, unknown> }[] = []
  let finish: ((value: ExecutionDetail) => void) | undefined
  let result: ExecutionDetail
  host.dataset.calls = '[]'
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'prepare_file_restoration')
          return {
            id,
            sourceRunId: record.id,
            sourceItemId: record.items[0].id,
            currentPath: record.items[0].receipt!.destination,
            originalPath: mode === 'mismatch' ? '/other' : record.items[0].path,
            expiresAt: Date.now() + 29_000,
          }
        if (command === 'execute_file_restoration') {
          result = {
            ...record,
            id: 'ca9784dc-57d4-481f-9707-0b7a8076e2ab',
            planId: id,
            origin: 'fileRestoration',
            method: 'move',
            items: [
              {
                ...record.items[0],
                path: record.items[0].receipt!.destination,
                targetPath: record.items[0].path,
                receipt: { ...record.items[0].receipt!, destination: record.items[0].path },
              },
            ],
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
            movedCount: 0,
            items: [{ ...result.items[0], state: 'cancelled', receipt: null }],
          })
        else if (command !== 'discard_file_restoration') throw Error(command)
      },
    },
  })
  function Fixture() {
    const [target, setTarget] = useState<RestorationTarget | null>(null)
    const [active, setActive] = useState(false)
    return (
      <>
        <ExecutionHistory
          initialRecords={[record]}
          loadRecords={async () => [record]}
          loadDetail={async () => record}
          onRestore={active ? undefined : setTarget}
        />
        <FileRestorationCenter
          target={target}
          transport={restorationTransport}
          onClose={() => setTarget(null)}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
