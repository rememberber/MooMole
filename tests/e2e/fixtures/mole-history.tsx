import { createRoot } from 'react-dom/client'
import { useState } from 'react'
import { MoleHistoryView } from '../../../src/components/MoleHistoryView'
import type { MoleHistory } from '../../../src/types'

// Browser-only data; never written to Mole logs or the user's SQLite database.
export const moleHistory: MoleHistory = {
  version: 1,
  revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
  collectedAt: 1792080000000,
  limit: 200,
  operationsSource: { path: '/fixture/operations.log', state: 'ready', reason: null },
  deletionsSource: { path: '/fixture/deletions.log', state: 'ready', reason: null },
  sessions: Array.from({ length: 25 }, (_, i) => ({
    command: i === 1 ? 'optimize' : 'clean',
    run_id: i < 2 || i > 3 ? `run-${i}` : '',
    attribution: i === 2 ? 'ambiguous' : i === 3 ? 'command' : 'run',
    started_at: `2026-10-16 10:00:${String(i).padStart(2, '0')}`,
    ended_at: i === 0 ? '' : '2026-10-16 11:00:00',
    items: 1,
    size: '4KB',
    operation_count: 2,
    failed_tasks: 0,
    actions: { removed: 0, trashed: 1, skipped: 1, failed: 0, rebuilt: 0, other: 0 },
  })),
  deletions: Array.from({ length: 25 }, (_, i) => ({
    timestamp: '2026-10-16T10:00:00+0800',
    mode: i === 1 ? 'future-mode' : 'trash',
    status: i === 0 ? 'dry-run' : i === 1 ? 'future-status' : 'ok',
    size_kb: i === 0 ? null : 4,
    path: i === 0 ? '/fixture/中文\\n[*] $(literal)' : `/fixture/cache-${i}`,
  })),
}

function Fixture() {
  const [report, setReport] = useState<MoleHistory | null>(moleHistory)
  return (
    <>
      <button
        onClick={() =>
          setReport({
            ...moleHistory,
            operationsSource: {
              ...moleHistory.operationsSource,
              state: 'unavailable',
              reason: 'fixture 权限拒绝',
            },
            sessions: [],
          })
        }
      >
        模拟部分受限
      </button>
      <button
        onClick={() =>
          setReport({
            ...moleHistory,
            operationsSource: { ...moleHistory.operationsSource, state: 'missing' },
            deletionsSource: { ...moleHistory.deletionsSource, state: 'missing' },
            sessions: [],
            deletions: [],
          })
        }
      >
        模拟日志缺失
      </button>
      <button onClick={() => setReport({ ...moleHistory, sessions: [], deletions: [] })}>
        模拟空日志
      </button>
      <MoleHistoryView
        report={report}
        disabled={false}
        reading={false}
        onRead={() => setReport(moleHistory)}
      />
    </>
  )
}

export function mount() {
  const host = document.createElement('div')
  host.id = 'mole-history-test-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  createRoot(host).render(<Fixture />)
}
