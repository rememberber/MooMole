import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { AnalysisExplorer } from '../../../src/components/AnalysisExplorer'
import type { Analysis } from '../../../src/types'

export const analysis: Analysis = {
  path: '/fixture/analysis',
  scan_status: 'partial',
  total_size: 90000,
  entries: [
    {
      name: 'Projects',
      path: '/fixture/analysis/Projects',
      size: 8000,
      is_dir: true,
      scan_status: 'partial',
    },
    {
      name: 'video.mp4',
      path: '/fixture/analysis/video.mp4',
      size: 4000,
      is_dir: false,
      scan_status: 'complete',
    },
    {
      name: '无法读取',
      path: '/fixture/analysis/无法读取',
      size: 99999999,
      is_dir: true,
      scan_status: 'unavailable',
    },
    {
      name: 'empty.txt',
      path: '/fixture/analysis/empty.txt',
      size: 0,
      is_dir: false,
      scan_status: 'complete',
    },
    {
      name: 'partial-zero',
      path: '/fixture/analysis/partial-zero',
      size: 0,
      is_dir: true,
      scan_status: 'partial',
    },
    {
      name: '中文\n[*] $(literal)',
      path: '/fixture/analysis/中文\n[*] $(literal)',
      size: 1024,
      is_dir: false,
      scan_status: 'complete',
    },
  ],
}

export function mount(many = false, observation?: Analysis['mooObservation']) {
  const host = document.createElement('div')
  host.id = 'analysis-test-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  function Fixture() {
    const [value, setValue] = useState<Analysis>(
      many
        ? {
            ...analysis,
            entries: Array.from({ length: 205 }, (_, i) => ({
              name: `item-${String(i).padStart(3, '0')}`,
              path: `/fixture/analysis/item-${String(i).padStart(3, '0')}`,
              size: i === 204 ? 10000000 : 100 + i,
              scan_status: 'complete',
              is_dir: false,
            })),
          }
        : { ...analysis, mooObservation: observation },
    )
    const [request, setRequest] = useState('')
    return (
      <>
        <button onClick={() => setValue({ ...value })}>模拟重新读取成功</button>
        <button onClick={() => setValue({ ...value, entries: value.entries.slice(0, 2) })}>
          模拟较短结果
        </button>
        <button
          onClick={() => setValue({ ...value, path: '/', entries: [], scan_status: 'unavailable' })}
        >
          模拟根目录不可读
        </button>
        <p aria-label="fixture 路径请求">{request}</p>
        <AnalysisExplorer
          analysis={value}
          home="/fixture"
          canAnalyze
          canReveal
          refreshing={false}
          onAnalyze={(path) => setRequest(`analyze:${path}`)}
          onReveal={(path) => setRequest(`reveal:${path}`)}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
