import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { SearchRepairCenter } from '../../../src/components/SearchRepairCenter'
import { ProblemRepairs } from '../../../src/components/ProblemRepairs'
import { searchRepairTransport, type SearchRepairReport } from '../../../src/lib/search-repair'

// All indexing responses are browser fixtures, never native actions.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'search-repair-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const calls: { command: string; args: Record<string, unknown> }[] = []
  const path = '/Users/demo/Documents/Example'
  let finish: ((v: SearchRepairReport) => void) | undefined
  let report: SearchRepairReport
  host.dataset.calls = '[]'
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'prepare_search_repair') {
          const createdAt = Date.now() - 1000
          return {
            id: 'f0d4482c-f575-4861-980b-8db64b314803',
            path,
            createdAt,
            expiresAt: createdAt + 120_000,
          }
        }
        if (command === 'execute_search_repair') {
          report = {
            requestId: String(args.requestId),
            path: mode === 'mismatch' ? '/other' : path,
            attempt: {
              step: 'importFolder',
              outcome: 'commandSucceeded',
              exitCode: 0,
              stop: null,
              reason: 'commandSucceeded',
            },
          }
          return mode === 'pending'
            ? new Promise<SearchRepairReport>((resolve) => {
                finish = resolve
              })
            : report
        }
        if (command === 'cancel_task')
          finish?.({
            ...report,
            attempt: { ...report.attempt, outcome: 'uncertain', exitCode: 143, stop: 'cancelled' },
          })
        else if (command !== 'discard_search_repair') throw Error(command)
      },
    },
  })
  function Fixture() {
    const [signal, setSignal] = useState(0),
      [active, setActive] = useState(false)
    return (
      <>
        <ProblemRepairs
          enabled={!active}
          onPreview={() => {}}
          onSearch={() => setSignal((n) => n + 1)}
          onRegistration={() => {
            host.dataset.route = 'applications'
          }}
        />
        <SearchRepairCenter
          openSignal={signal}
          transport={{ ...searchRepairTransport, choose: async () => path }}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
