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
    mode === 'resume'
      ? detailRaw(requestId, 'intent')
      : [
            'history-unknown',
            'review-error',
            'review-mismatch',
            'home-unknown',
            'home-reviewed',
            'home-moved',
          ].includes(mode)
        ? detailRaw(requestId, 'uncertain')
        : null
  if (record && mode === 'home-reviewed') record.reviewedAt = Date.now()
  if (mode === 'home-moved') record = detailRaw(requestId, 'moved')
  let live = mode === 'resume'
  let id = requestId
  const calls: { command: string; args: unknown }[] = []
  let changed: () => void = () => {}
  let resolve: ((value: unknown) => void) | null = null
  let lastPlan = planRaw()
  let releasePrepare: ((v: unknown) => void) | null = null
  const cancelReplies = new Map<string, (e: Error) => void>()
  let readsBlocked = false
  let reviewFails = mode === 'review-error'
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
      if (mode === 'normal' || mode.startsWith('usage')) {
        finish()
        return record
      }
      if (mode === 'lost') throw new Error('响应通道已中断')
      return new Promise((r) => (resolve = r))
    },
    read: async (request) => {
      call('read', { requestId: request })
      if (readsBlocked) return new Promise(() => {})
      if (mode === 'readError') throw new Error('读取暂不可用')
      return { version: 1, requestId: request, active: live, record }
    },
    list: async () => {
      call('list')
      return record ? [record] : []
    },
    acknowledge: async (value) => {
      call('acknowledge', { requestId: value.id, snapshotSha256: value.snapshotSha256 })
      if (reviewFails) throw new Error('fixture 核查保存暂时失败')
      record = { ...record!, reviewedAt: Date.now() }
      changed()
      return mode === 'review-mismatch' ? { ...record, name: 'invalid rewritten fact' } : record
    },
    cancel: async (request) => {
      call('cancel', { requestId: request })
      if (mode === 'cancelError') throw new Error('取消响应暂不可用')
      if (mode === 'lateCancel')
        return new Promise<void>((_, reject) => cancelReplies.set(request, reject))
      finish(mode === 'lost' ? 'moved' : 'cancelled')
    },
  }
  const libraryApps = [
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
  ]

  if (mode.startsWith('usage'))
    libraryApps.push(
      {
        ...libraryApps[0],
        name: '另一旧副本',
        path: '/Users/demo/Applications/Other.app',
        size: '3MB',
      },
      { ...libraryApps[0], name: '记录未知', path: '/Applications/NoHistory.app' },
      { ...libraryApps[0], name: '受保护副本', path: '/Applications/Protected.app' },
    )
  let releaseUsage: (() => void) | null = null
  const native = (window as any).__TAURI_INTERNALS__ || {}
  const originalInvoke = native.invoke
  native.invoke = async (command: string, args: any = {}) => {
    if (command === 'inspect_application_usage') {
      call('usage', args)
      const now = Date.now(),
        checkedAt = now - (mode === 'usage-expired' ? 300001 : 0)
      const rows = args.indices.map((index: number) => ({
        index,
        path: libraryApps[index].path,
        bundleId: libraryApps[index].bundle_id,
        state:
          index === 1
            ? 'running'
            : index === 3
              ? 'unknown'
              : index === 4
                ? 'protected'
                : 'observed',
        lastOpenedAt: index === 3 || index === 4 ? null : checkedAt - 200 * 86400000,
        issue: index === 3 ? 'notReported' : index === 4 ? 'protected' : null,
      }))
      const report = {
        version: 1,
        taskId: mode === 'usage-mismatch' ? requestId : args.taskId,
        requestId: args.requestId,
        checkedAt,
        expiresAt: checkedAt + 300000,
        requestedIndices: args.indices,
        rows,
        complete: true,
        cancelled: false,
      }
      if (mode === 'usage-cancel' || mode === 'usage-late')
        return new Promise((resolve) => {
          releaseUsage = () =>
            resolve({
              ...report,
              ...(mode === 'usage-cancel'
                ? { complete: false, cancelled: true, rows: rows.slice(0, 1) }
                : {}),
            })
        })
      return report
    }
    if (command === 'cancel_task' && mode.startsWith('usage')) {
      call('usageCancel', args)
      if (mode === 'usage-cancel') releaseUsage?.()
      return null
    }
    return originalInvoke ? originalInvoke(command, args) : null
  }
  Object.assign(window, { __TAURI_INTERNALS__: native })
  Object.assign(window, {
    uninstallFixture: {
      calls,
      allowReview: () => {
        reviewFails = false
      },
      finish,
      holdReads: () => {
        readsBlocked = true
      },
      rejectCancel: (request: string) => {
        cancelReplies.get(request)?.(new Error('旧取消响应已中断'))
        cancelReplies.delete(request)
      },
      releasePrepare: () => {
        releasePrepare?.(lastPlan)
      },
      getRecord: () => record,
      releaseUsage: () => releaseUsage?.(),
    },
  })
  if (mode === 'resume') sessionStorage.setItem(uninstallSessionKey, requestId)
  function Fixture() {
    const [listed, setListed] = useState(libraryApps)
    const [selected, setSelected] = useState<UninstallTarget | null>(null)
    const [active, setActive] = useState(false)
    const [historyVisible, setHistoryVisible] = useState(!mode.startsWith('home-'))
    const [epoch, setEpoch] = useState(0)
    const [, render] = useState(0)
    changed = () => render((v) => v + 1)
    return (
      <main tabIndex={-1} className="main-content">
        {mode.startsWith('usage') && (
          <button onClick={() => setListed(libraryApps.map((a) => ({ ...a })))}>
            模拟应用清单刷新
          </button>
        )}
        <button
          onClick={() => {
            setSelected(null)
            setEpoch((e) => e + 1)
          }}
        >
          重建观察界面
        </button>
        <button onClick={() => setHistoryVisible((v) => !v)}>
          {historyVisible ? '返回概览' : '操作历史'}
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
          historyVisible={historyVisible}
          transport={transport}
          onTargetClose={() => setSelected(null)}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
        <ApplicationLibrary
          taskId={target.taskId}
          apps={listed}
          home="/Users/demo"
          canReveal={mode.startsWith('usage')}
          onReveal={() => {}}
          canUninstall={!active}
          onUninstall={setSelected}
        />
      </main>
    )
  }
  createRoot(host).render(<Fixture />)
}
