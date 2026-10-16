import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationLibrary } from '../../../src/components/ApplicationLibrary'
import type { AppEntry } from '../../../src/types'

export const applications: AppEntry[] = [
  {
    name: 'Example',
    bundle_id: 'org.example.same',
    source: 'App',
    uninstall_name: 'Example',
    path: '/Applications/Example.app',
    size: '1.00GB',
  },
  {
    name: 'Example',
    bundle_id: 'org.example.same',
    source: 'Homebrew',
    uninstall_name: 'example',
    path: '/Users/demo/Applications/Example Beta.app',
    size: '999.0MB',
  },
  {
    name: '游戏',
    bundle_id: '',
    source: 'App',
    uninstall_name: 'unused',
    path: '/Applications/Steam.app',
    size: 'N/A (Steam-managed)',
  },
  {
    name: '空包',
    bundle_id: '',
    source: 'App',
    uninstall_name: 'unused',
    path: '/Applications/Empty.app',
    size: '0B',
  },
  {
    name: '新来源',
    bundle_id: 'org.future.app',
    source: 'future-manager',
    uninstall_name: 'unused',
    path: '/Volumes/中文 外接盘/Applications/特殊\n[*] $(literal).app',
    size: 'Unknown',
  },
]

// Only browser component fixtures; no native commands or filesystem mutations.
export function mount(many = false) {
  const host = document.createElement('div')
  host.id = 'applications-test-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  function Fixture() {
    const [apps, setApps] = useState(
      many
        ? Array.from({ length: 115 }, (_, i) => ({
            ...applications[0],
            name: `App ${i}`,
            path: `/fixture/${i}.app`,
          }))
        : applications,
    )
    const [revealed, setRevealed] = useState('')
    return (
      <>
        <button onClick={() => setApps(apps.map((app) => ({ ...app })))}>模拟列表刷新成功</button>
        <button onClick={() => setApps([])}>模拟空清单</button>
        <p aria-label="fixture Finder 请求">{revealed}</p>
        <ApplicationLibrary apps={apps} home="/Users/demo" canReveal onReveal={setRevealed} />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
