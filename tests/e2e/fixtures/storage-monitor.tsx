import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { StorageMonitor } from '../../../src/components/StorageMonitor'
import {
  storageMonitorDefaults,
  type StorageMonitorConfig,
  type StorageMonitorView,
} from '../../../src/lib/storage-monitor'
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'storage-monitor-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const now = Date.now(),
    gib = 1024 ** 3
  let view: StorageMonitorView = {
    config: { ...storageMonitorDefaults },
    collecting: false,
    error: null,
    samples: [
      {
        measuredAt: now - 3_600_000,
        volumeId: 'abc:123',
        totalBytes: 100 * gib,
        availableBytes: 20 * gib,
      },
      { measuredAt: now, volumeId: 'abc:123', totalBytes: 100 * gib, availableBytes: 8 * gib },
    ],
  }
  const calls: { command: string; config?: StorageMonitorConfig }[] = []
  host.dataset.calls = '[]'
  function call(command: string, config?: StorageMonitorConfig) {
    calls.push({ command, config })
    host.dataset.calls = JSON.stringify(calls)
  }
  const transport = {
    read: async () => {
      call('read')
      return mode === 'invalid'
        ? { ...view, samples: [{ ...view.samples[0], availableBytes: null }] }
        : structuredClone(view)
    },
    configure: async (config: StorageMonitorConfig) => {
      call('configure', config)
      view = { ...view, config: { ...config } }
      return structuredClone(view)
    },
    sample: async () => {
      call('sample')
      view = {
        ...view,
        samples: [...view.samples, { ...view.samples[1], measuredAt: Date.now() + 1 }],
      }
      return structuredClone(view)
    },
  }
  function Fixture() {
    const [route, setRoute] = useState('')
    return (
      <>
        <p aria-label="fixture destination">{route}</p>
        <StorageMonitor enabled transport={transport} onAnalyze={() => setRoute('analyze')} />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
