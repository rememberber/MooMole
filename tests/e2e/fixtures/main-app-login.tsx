import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { MainAppLogin } from '../../../src/components/MainAppLogin'
import { mainAppLoginTransport } from '../../../src/lib/main-app-login'
import { setLanguagePreference } from '../../../src/i18n/locale'
export function mount(mode = 'normal') {
  setLanguagePreference(mode === 'en' ? 'en' : mode === 'ja' ? 'ja' : 'zh')
  const host = document.createElement('div')
  host.id = 'main-login-fixture'
  host.style.cssText =
    'position:fixed;inset:0;background:var(--background);padding:20px;overflow:auto;z-index:100'
  document.body.append(host)
  const system = {
    bundleId: 'io.github.rememberber.moomole',
    bundlePath: '/Applications/MooMole.app',
    status: mode === 'notFound' ? 'notFound' : mode === 'enabled' ? 'enabled' : 'notRegistered',
  }
  const previous = 'a1000000-0000-4000-8000-000000000001'
  let records: Record<string, unknown>[] =
    mode === 'unknown'
      ? [
          {
            version: 1,
            id: previous,
            enabled: true,
            before: { ...system },
            after: null,
            createdAt: 1000,
            finishedAt: 1001,
            state: 'unknown',
            error: 'loginInterrupted',
            nativeCode: null,
            reviewedAt: null,
          },
        ]
      : []
  const calls: { command: string; args: Record<string, unknown> }[] = []
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'get_main_app_login')
          return { system, records, activeRequests: [], error: null }
        if (command === 'set_main_app_login') {
          const before = { ...system },
            now = Date.now()
          if (mode !== 'failed')
            system.status = args.enabled
              ? mode === 'approval'
                ? 'requiresApproval'
                : 'enabled'
              : 'notRegistered'
          const r = {
            version: 1,
            id: args.requestId,
            enabled: args.enabled,
            before,
            after: mode === 'failed' ? null : { ...system },
            createdAt: now,
            finishedAt: now,
            state: mode === 'failed' ? 'unknown' : 'accepted',
            error: mode === 'failed' ? 'loginRequestRejected' : null,
            nativeCode: mode === 'failed' ? 3 : null,
            reviewedAt: null,
          }
          records = [r, ...records]
          if (mode === 'mismatch') return { ...r, id: previous }
          return r
        }
        if (command === 'acknowledge_main_app_login') {
          records = records.map((r) => (r.id === args.id ? { ...r, reviewedAt: Date.now() } : r))
          return null
        }
        if (command === 'open_login_item_settings') return null
        throw Error(command)
      },
    },
  })
  createRoot(host).render(
    <StrictMode>
      <MainAppLogin enabled transport={mainAppLoginTransport} />
    </StrictMode>,
  )
}
