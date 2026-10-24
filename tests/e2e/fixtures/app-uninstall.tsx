import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationUninstallCenter } from '../../../src/components/ApplicationUninstallCenter'
import { ApplicationLibrary } from '../../../src/components/ApplicationLibrary'
import type { UninstallTarget, UninstallTransport } from '../../../src/lib/app-uninstall'
import { uninstallSessionKey } from '../../../src/lib/app-uninstall'
import { target, planRaw, reviewRaw, detailRaw, requestId } from '../../fixtures/uninstall'

export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'uninstall-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  let record: ReturnType<typeof detailRaw> | null =
    mode === 'resume' ? detailRaw(requestId, 'intent') : null
  let live = mode === 'resume'
  let id = requestId
  const calls: { command: string; args: unknown }[] = []
  let changed: () => void = () => {}
  let resolve: ((value: unknown) => void) | null = null
  let lastPlan = planRaw()
  let releasePrepare: ((v: unknown) => void) | null = null
  const call = (command: string, args: unknown = {}) => {
    calls.push({ command, args })
    changed()
  }
  function finish(state = 'moved') {
    record = detailRaw(id, state)
    live = false
    resolve?.(record)
    resolve = null
    changed()
  }
  const transport: UninstallTransport = {
    prepare: async (taskId, index) => {
      call('prepare', { taskId, index })
      lastPlan = planRaw()
      if (mode === 'slow') return new Promise((r) => (releasePrepare = r))
      return lastPlan
    },
    review: async (planId) => {
      call('review', { planId })
      const v = reviewRaw(lastPlan)
      if (mode === 'unknown')
        return {
          ...v,
          confirmation: null,
          runtime: {
            ...v.runtime,
            code: 'unknown',
            canPrepareConfirmation: false,
            coverage: 'incomplete',
            issue: 'changed',
          },
        }
      if (mode === 'invalid')
        return { ...v, confirmation: { ...v.confirmation, relatedData: 'removeAll' } }
      return v
    },
    discardPlan: async (planId) => {
      call('discardPlan', { planId })
    },
    discardConfirmation: async (confirmationId) => {
      call('discardConfirmation', { confirmationId })
    },
    execute: async (planId, confirmationId, request) => {
      call('execute', { planId, confirmationId, requestId: request, ordinaryAppConfirmed: true })
      id = request
      if (mode === 'reject') throw new Error('本次确认已撤下')
      live = true
      record = detailRaw(id, 'intent')
      changed()
      if (mode === 'normal') {
        finish()
        return record
      }
      if (mode === 'lost') throw new Error('响应通道已中断')
      return new Promise((r) => (resolve = r))
    },
    read: async (request) => {
      call('read', { requestId: request })
      if (mode === 'readError') throw new Error('读取暂不可用')
      return { version: 1, requestId: request, active: live, record }
    },
    list: async () => {
      call('list')
      return record ? [record] : []
    },
    cancel: async (request) => {
      call('cancel', { requestId: request })
      finish(mode === 'lost' ? 'moved' : 'cancelled')
    },
  }
  Object.assign(window, {
    uninstallFixture: {
      calls,
      finish,
      releasePrepare: () => {
        releasePrepare?.(lastPlan)
      },
      getRecord: () => record,
    },
  })
  if (mode === 'resume') sessionStorage.setItem(uninstallSessionKey, requestId)
  function Fixture() {
    const [selected, setSelected] = useState<UninstallTarget | null>(null)
    const [active, setActive] = useState(false)
    const [epoch, setEpoch] = useState(0)
    const [, render] = useState(0)
    changed = () => render((v) => v + 1)
    return (
      <main tabIndex={-1} className="main-content">
        <button
          onClick={() => {
            setSelected(null)
            setEpoch((e) => e + 1)
          }}
        >
          重建观察界面
        </button>
        <output aria-label="卸载提交次数">
          {calls.filter((c) => c.command === 'execute').length}
        </output>
        <output aria-label="计划撤下次数">
          {calls.filter((c) => c.command === 'discardPlan').length}
        </output>
        <ApplicationUninstallCenter
          key={epoch}
          target={selected}
          enabled
          historyVisible
          transport={transport}
          onTargetClose={() => setSelected(null)}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
        <ApplicationLibrary
          taskId={target.taskId}
          apps={[
            {
              name: target.name,
              path: target.path,
              bundle_id: target.bundleId,
              source: 'App',
              size: '1MB',
              uninstall_name: 'unused',
            },
            {
              name: '受管理 App',
              path: '/Applications/Managed.app',
              bundle_id: 'org.example.managed',
              source: 'Homebrew',
              size: '2MB',
              uninstall_name: 'managed',
            },
          ]}
          home="/Users/demo"
          canReveal={false}
          onReveal={() => {}}
          canUninstall={!active}
          onUninstall={setSelected}
        />
      </main>
    )
  }
  createRoot(host).render(<Fixture />)
}
