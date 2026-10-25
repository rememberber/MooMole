import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { QuickLookRefreshCenter } from '../../../src/components/QuickLookRefreshCenter'
import { refreshSessionKey, type RefreshTransport } from '../../../src/lib/quicklook-refresh'
import { draftRaw, refreshDetailRaw, refreshRequest } from '../../fixtures/quicklook'
export function mount(mode = 'normal') {
  sessionStorage.removeItem(refreshSessionKey)
  if (mode === 'resume') sessionStorage.setItem(refreshSessionKey, refreshRequest)
  const host = document.createElement('main')
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  let record = ['resume', 'bootstrap'].includes(mode)
      ? refreshDetailRaw(refreshRequest, 'interrupted')
      : null,
    id = refreshRequest,
    active = false,
    resolve: ((v: unknown) => void) | null = null,
    release: ((v: unknown) => void) | null = null,
    rejectCancel: ((e: Error) => void) | null = null
  const calls: { command: string; id?: string }[] = []
  let changed = () => {}
  function finish(state = 'completed') {
    record = refreshDetailRaw(id, state, record?.createdAt)
    active = false
    resolve?.(record)
    resolve = null
    changed()
  }
  const transport: RefreshTransport = {
    prepare: async (selected) => {
      calls.push({ command: 'prepare' })
      changed()
      if (mode === 'slow') return new Promise((r) => (release = r))
      return { ...draftRaw(selected), ...(mode === 'invalid' ? { scope: 'wholeFinder' } : {}) }
    },
    discard: async (id) => {
      calls.push({ command: 'discard', id })
      changed()
    },
    execute: async (_draft, request) => {
      id = request
      calls.push({ command: 'execute', id })
      active = mode !== 'admission'
      record = active ? refreshDetailRaw(id, 'running') : null
      changed()
      if (mode === 'normal') {
        finish()
        return record
      }
      if (mode === 'lost') throw new Error('响应通道中断')
      return new Promise((r) => (resolve = r))
    },
    read: async (request) => ({ version: 1, requestId: request, active, record }),
    list: async () => (record ? [record] : []),
    cancel: async (request) => {
      calls.push({ command: 'cancel', id: request })
      if (mode === 'cancel-error') throw new Error('取消响应暂时不可用')
      if (mode === 'admission' && !active) throw new Error('此维护任务已结束，请读取实际结果。')
      if (mode === 'late-cancel-error')
        return new Promise<void>((_, reject) => {
          rejectCancel = reject
        })
      finish('cancelled')
    },
    acknowledge: async (value) => {
      calls.push({ command: 'acknowledge', id: value.id })
      record = { ...record!, reviewedAt: Date.now(), retainedForReview: false }
      changed()
      return record
    },
  }
  Object.assign(window, {
    refreshFixture: {
      calls,
      finish,
      admit: () => {
        active = true
        record = refreshDetailRaw(id, 'running')
        changed()
      },
      rejectCancel: () => {
        rejectCancel?.(new Error('此维护任务已结束，请读取实际结果。'))
        rejectCancel = null
      },
      release: () => {
        release?.(draftRaw())
        release = null
      },
    },
  })
  function View() {
    const [signal, setSignal] = useState(0),
      [, refresh] = useState(0)
    changed = () => refresh((x) => x + 1)
    return (
      <>
        <button onClick={() => setSignal((n) => n + 1)}>刷新预览…</button>
        <output aria-label="维护提交次数">
          {calls.filter((c) => c.command === 'execute').length}
        </output>
        <output aria-label="维护撤销次数">
          {calls.filter((c) => c.command === 'discard').length}
        </output>
        <QuickLookRefreshCenter
          openSignal={signal}
          enabled
          historyVisible={mode !== 'bootstrap'}
          initialRecords={mode === 'bootstrap' && record ? [record] : []}
          transport={transport}
          onActiveChange={() => {}}
        />
      </>
    )
  }
  createRoot(host).render(<View />)
}
declare global {
  interface Window {
    refreshFixture: {
      calls: { command: string; id?: string }[]
      finish: (state?: string) => void
      admit: () => void
      rejectCancel: () => void
      release: () => void
    }
  }
}
