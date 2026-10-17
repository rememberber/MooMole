import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TaskHistory } from '../../../src/components/TaskHistory'
import type { TaskRecord } from '../../../src/types'

const at = 1792216800000
export const records: TaskRecord[] = [
  {
    id: 'failed-fixture',
    kind: 'analyze',
    state: 'failed',
    createdAt: at,
    finishedAt: at + 1250,
    path: '/Users/demo/中文 空格/[*] $(literal)/' + 'very-long-'.repeat(24) + '\n换行',
    message: 'fixture 权限拒绝\n完整诊断保留：' + '原因说明'.repeat(30),
  },
  {
    id: 'running-fixture',
    kind: 'analyze',
    state: 'running',
    createdAt: at - 1000,
    finishedAt: null,
    path: '/fixture/current',
    message: null,
  },
  {
    id: 'cancelled-fixture',
    kind: 'clean',
    state: 'cancelled',
    createdAt: at - 2000,
    finishedAt: at,
    path: '/fixture/cancel',
    message: 'fixture 已取消',
  },
  {
    id: 'status-fixture',
    kind: 'status',
    state: 'succeeded',
    createdAt: at - 3000,
    finishedAt: at - 3000,
    path: null,
    message: 'fixture 检查完成',
  },
  {
    id: 'future-fixture',
    kind: 'future-check' as TaskRecord['kind'],
    state: 'future-phase',
    createdAt: NaN,
    finishedAt: -1,
    path: null,
    message: null,
  },
]
let replace: (mode: 'changed' | 'missing') => void
export function update(mode: 'changed' | 'missing') {
  replace(mode)
}
export function mount(many = false) {
  const host = document.createElement('div')
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  function Fixture() {
    const [list, setList] = useState(
      many
        ? Array.from({ length: 115 }, (_, i) => ({
            ...records[0],
            id: `bulk-${i}`,
            path: `/fixture/task-${i}`,
          }))
        : records,
    )
    const [refreshFails, setRefreshFails] = useState(false)
    const [tracking, setTracking] = useState('')
    replace = (mode) =>
      setList((list) =>
        mode === 'missing'
          ? []
          : list.map((record) =>
              record.id === 'running-fixture'
                ? {
                    ...record,
                    state: 'failed',
                    finishedAt: at + 1500,
                    message: 'fixture 目录读取失败',
                  }
                : record,
            ),
      )
    return (
      <>
        <button onClick={() => setRefreshFails(!refreshFails)}>切换刷新失败</button>
        <p aria-label="fixture 跟踪任务">{tracking}</p>
        <TaskHistory
          records={list}
          home="/Users/demo"
          canRefresh
          canTrack
          onRefresh={async () => {
            if (refreshFails) throw new Error('fixture history unavailable')
            replace('changed')
          }}
          onTrack={(task) => setTracking(task.id)}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
