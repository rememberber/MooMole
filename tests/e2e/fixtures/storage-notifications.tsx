import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { useLanguage } from '../../../src/i18n/locale'
import { StorageNotifications } from '../../../src/components/StorageNotifications'
import type {
  StorageNotificationsTransport,
  StorageNotificationsView,
} from '../../../src/lib/storage-notifications'
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'storage-notifications-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const saved = sessionStorage.getItem('notification-view')
  let view: StorageNotificationsView = saved
    ? JSON.parse(saved)
    : {
        enabled: false,
        monitoringEnabled: false,
        permission: {
          authorization: mode === 'denied' ? 'denied' : 'notDetermined',
          alerts: false,
          center: true,
        },
        error: null,
        records: [],
      }
  const calls: { command: string; value?: boolean }[] = [],
    call = (command: string, value?: boolean) => {
      calls.push({ command, value })
      host.dataset.calls = JSON.stringify(calls)
    },
    save = () => sessionStorage.setItem('notification-view', JSON.stringify(view))
  let release: (() => void) | undefined
  const transport: StorageNotificationsTransport = {
    read: async () => {
      call('read')
      if (mode === 'future') throw Error('unsupportedSettings')
      return structuredClone(view)
    },
    request: async () => {
      call('request')
      if (mode === 'timeout') throw Error('authorizationTimeout')
      if (mode === 'slow')
        await new Promise<void>((r) => {
          release = r
        })
      view.permission = { authorization: 'authorized', alerts: true, center: true }
      save()
      return structuredClone(view)
    },
    configure: async (enabled) => {
      call('configure', enabled)
      if (mode !== 'mismatch') view.enabled = enabled
      save()
      return structuredClone(view)
    },
    test: async () => {
      call('test')
      const now = Date.now()
      view.records.unshift({
        id: crypto.randomUUID(),
        kind: 'test',
        state: mode === 'unknown' ? 'unknown' : 'submitted',
        createdAt: now,
        finishedAt: now,
        availableBytes: null,
        thresholdBytes: null,
        error: mode === 'unknown' ? 'submissionTimeout' : null,
      })
      save()
      if (mode === 'unknown') throw Error('submissionTimeout')
      return structuredClone(view)
    },
  }
  Object.assign(window, { notificationFixture: { release: () => release?.() } })
  function Fixture() {
    useLanguage()
    return <StorageNotifications enabled transport={transport} />
  }
  createRoot(host).render(
    <StrictMode>
      <Fixture />
    </StrictMode>,
  )
}
declare global {
  interface Window {
    notificationFixture: { release: () => void }
  }
}
