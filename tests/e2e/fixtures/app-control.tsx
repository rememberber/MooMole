import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationLibrary } from '../../../src/components/ApplicationLibrary'
import { ApplicationControlCenter } from '../../../src/components/ApplicationControlCenter'
import type { AppControlTarget } from '../../../src/lib/app-control'
import { applications } from './application-library'

// Browser-only transport: no process or filesystem actions.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  let calls = 0
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown>) => {
        if (command !== 'control_application') throw Error(command)
        calls++
        host.dataset.calls = String(calls)
        host.dataset.args = JSON.stringify(args)
        return {
          ...args,
          path: mode === 'mismatch' ? '/Applications/Other.app' : applications[0].path,
          bundleId: applications[0].bundle_id,
          requested: true,
          pids: [12345],
          outcome: args.action === 'quit' ? 'quit' : 'opened',
          reason: 'fixture result',
        }
      },
    },
  })
  function Fixture() {
    const [target, setTarget] = useState<AppControlTarget | null>(null)
    const [active, setActive] = useState(false)
    const [stale, setStale] = useState(false)
    return (
      <>
        <p role="status">{stale ? 'fixture stale' : 'fixture fresh'}</p>
        <ApplicationLibrary
          apps={applications.slice(0, 2)}
          home="/Users/demo"
          taskId="fixture-app-task"
          onControl={setTarget}
          canControl={!active && !stale}
        />
        <ApplicationControlCenter
          target={target}
          onClose={() => setTarget(null)}
          onActiveChange={setActive}
          onOutcome={() => setStale(true)}
        />
      </>
    )
  }
  host.dataset.calls = '0'
  host.id = 'app-control-fixture'
  createRoot(host).render(<Fixture />)
}
