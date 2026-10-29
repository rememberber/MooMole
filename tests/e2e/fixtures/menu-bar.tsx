import { createRoot } from 'react-dom/client'
import { MenuBarSettings } from '../../../src/components/MenuBarSettings'
import type { MenuBarView } from '../../../src/lib/menu-bar'
import '../../../src/styles.css'
export function mount(mode: string) {
  document.body.innerHTML = '<div id="menu-bar-fixture"></div>'
  const el = document.getElementById('menu-bar-fixture')!
  let saved = JSON.parse(sessionStorage.getItem('owned-menu-bar') ?? 'false') as boolean
  const calls: unknown[] = []
  const report = () => el.setAttribute('data-calls', JSON.stringify(calls))
  const view = (): MenuBarView => ({
    config: { enabled: saved },
    visible: mode === 'native-failure' ? false : saved,
    error: mode === 'native-failure' ? 'owned native unavailable' : null,
  })
  createRoot(el).render(
    <MenuBarSettings
      enabled
      transport={{
        read: async () => {
          calls.push('read')
          report()
          return mode === 'invalid' ? { config: { enabled: true } } : view()
        },
        configure: async (enabled) => {
          calls.push({ enabled })
          report()
          saved = enabled
          sessionStorage.setItem('owned-menu-bar', JSON.stringify(saved))
          return view()
        },
      }}
    />,
  )
}
