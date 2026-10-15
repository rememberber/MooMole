import { createRoot } from 'react-dom/client'
import { ExecutionHistory } from '../../../src/components/ExecutionHistory'
import type { ExecutionDetail } from '../../../src/types'

// Browser-only journal data. Never inserted into the user's SQLite database.
const at = 1792080000000
const interrupted: ExecutionDetail = {
  id: '6d048c7d-d222-410c-875d-a6e3cef18001',
  planId: '6d048c7d-d222-410c-875d-a6e3cef18002',
  method: 'trash',
  state: 'interrupted',
  createdAt: at,
  finishedAt: at + 1000,
  itemCount: 64,
  movedCount: 1,
  uncertainCount: 1,
  retainedForReview: true,
  revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
  protocol: 1,
  snapshotSha256: 'abcd'.repeat(16),
  items: Array.from({ length: 64 }, (_, i) => ({
    id: `item-${i}`,
    path: i === 0 ? '/fixture/中文\ncache *' : `/fixture/cache-${i}`,
    state: i === 0 ? 'moved' : i === 1 ? 'uncertain' : 'notStarted',
    reason:
      i === 0
        ? 'fixture 身份核对完成'
        : i === 1
          ? '只有意图，完成结果无法确认'
          : '批次中断，未开始此项',
    receipt:
      i === 0
        ? {
            destination: '/fixture/recovery/中文 cache *',
            device: '18446744073709551615',
            inode: '9007199254740993',
          }
        : null,
  })),
  events: [
    { itemId: 'item-0', state: 'intent', createdAt: at, reason: '先记录意图', receipt: null },
    { itemId: 'item-0', state: 'moved', createdAt: at + 1, reason: '随后记录完成', receipt: null },
    { itemId: 'item-1', state: 'intent', createdAt: at + 2, reason: '先记录意图', receipt: null },
    {
      itemId: 'item-1',
      state: 'uncertain',
      createdAt: at + 3,
      reason: '重启后保留待核实',
      receipt: null,
    },
  ],
}
const cancelled: ExecutionDetail = {
  ...interrupted,
  id: '6d048c7d-d222-410c-875d-a6e3cef18003',
  state: 'cancelled',
  itemCount: 2,
  uncertainCount: 0,
  items: [
    interrupted.items[0],
    { ...interrupted.items[1], state: 'cancelled', reason: '开始前取消，未尝试操作' },
  ],
}

export function mount() {
  let failDetail = false
  let failList = false
  const host = document.createElement('div')
  host.id = 'execution-test-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  createRoot(host).render(
    <>
      <p>执行历史 fixture · 模拟数据</p>
      <button
        onClick={() => {
          failDetail = true
        }}
      >
        模拟详情读取失败
      </button>
      <button
        onClick={() => {
          failList = true
        }}
      >
        模拟列表读取失败
      </button>
      <ExecutionHistory
        initialRecords={[interrupted, cancelled]}
        loadRecords={async () => {
          if (failList) {
            failList = false
            throw new Error('fixture 列表暂时不可读取')
          }
          return [interrupted, cancelled]
        }}
        loadDetail={async (id) => {
          await new Promise((resolve) => setTimeout(resolve, 150))
          if (failDetail) {
            failDetail = false
            throw new Error('fixture 详情暂时不可读取')
          }
          return id === interrupted.id ? interrupted : cancelled
        }}
      />
    </>,
  )
}
