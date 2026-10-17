import type { Page } from '@playwright/test'
import { demoBootstrap, demoSnapshot } from '../../../src/lib/demo'
import type { Analysis, AppEntry, MoleHistory } from '../../../src/types'

// Playwright-only transport mock: it never calls a native command or touches files.
export async function installTaskMock(
  page: Page,
  mode: 'resume' | 'new' | 'history' | 'apps' | 'analyze' | 'navigation',
  moleHistory?: MoleHistory,
  applications?: AppEntry[],
  analysis?: Analysis,
) {
  await page.addInitScript(
    ({ initial, snapshot, mode, moleHistory, applications, analysis }) => {
      const readTask = () => JSON.parse(sessionStorage.getItem('mock-task') ?? 'null')
      const writeTask = (task: unknown) => sessionStorage.setItem('mock-task', JSON.stringify(task))
      if (mode === 'resume' && !readTask())
        writeTask({
          id: '6d048c7d-d222-410c-875d-a6e3cef18003',
          kind: 'analyze',
          path: '/Users/demo/Projects',
          state: 'running',
          createdAt: Date.now(),
          finishedAt: null,
          message: null,
        })
      const finished = () => sessionStorage.getItem('mock-done') === 'yes'
      const finalState = () =>
        sessionStorage.getItem('mock-history-cancelled') === 'yes'
          ? 'cancelled'
          : sessionStorage.getItem('mock-history-error')
            ? 'failed'
            : 'succeeded'
      const history = () => {
        const task = readTask()
        return task
          ? [
              {
                ...task,
                state: finished() ? finalState() : 'running',
                message: finished() ? '检查完成' : null,
              },
            ]
          : []
      }
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (
            command: string,
            args: { taskId?: string; kind?: string; path?: string } = {},
          ) => {
            if (command === 'bootstrap')
              return {
                ...initial,
                history: history(),
                engine: { ...initial.engine, available: true, version: '1.59.1' },
              }
            if (command === 'get_history') return history()
            if (command === 'list_review_plans') return []
            if (command === 'plugin:event|listen')
              throw new Error('Simulated unavailable event channel')
            if (command === 'start_task') {
              const starts = JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]')
              starts.push(args)
              sessionStorage.setItem('mock-starts', JSON.stringify(starts))
              if (mode === 'resume') throw new Error('Existing task must not be restarted')
              if (mode === 'navigation' && args.kind === 'analyze') {
                sessionStorage.removeItem('mock-done')
                sessionStorage.removeItem('mock-history-cancelled')
              }
              if (mode === 'analyze' && args.kind === 'analyze')
                sessionStorage.removeItem('mock-done')
              if (mode === 'apps' && args.kind === 'apps') sessionStorage.removeItem('mock-done')
              if (mode === 'history' && args.kind === 'history') {
                sessionStorage.removeItem('mock-done')
                sessionStorage.removeItem('mock-history-cancelled')
              }
              writeTask({
                id: args.taskId,
                kind: args.kind,
                path: mode === 'navigation' ? (args.path ?? null) : null,
                state: 'running',
                createdAt: Date.now(),
                finishedAt: null,
                message: null,
              })
              return
            }
            if (command === 'get_task_snapshot') {
              const task = readTask()
              if (!task || task.id !== args.taskId) throw new Error('Unknown task')
              return {
                id: task.id,
                kind: task.kind,
                state: finished() ? finalState() : 'running',
                error:
                  finished() && finalState() !== 'succeeded'
                    ? (sessionStorage.getItem('mock-history-error') ?? '任务已取消。')
                    : null,
                result:
                  finished() && finalState() === 'succeeded'
                    ? task.kind === 'status'
                      ? snapshot
                      : task.kind === 'apps'
                        ? applications
                        : task.kind === 'history'
                          ? moleHistory
                          : mode === 'navigation'
                            ? {
                                path:
                                  task.path === '/fixture/alias' ? '/fixture/canonical' : task.path,
                                scan_status: 'complete',
                                total_size: 1024,
                                entries: [
                                  {
                                    name: 'child',
                                    path: `${task.path}/child`,
                                    size: 1024,
                                    scan_status: 'complete',
                                    is_dir: true,
                                  },
                                ],
                              }
                            : (analysis ?? {
                                path: task.path,
                                scan_status: 'complete',
                                entries: [],
                                total_size: 0,
                              })
                    : null,
              }
            }
            if (command === 'cancel_task') {
              sessionStorage.setItem('mock-history-cancelled', 'yes')
              sessionStorage.setItem('mock-done', 'yes')
              return
            }
            throw new Error(`Unexpected test command: ${command}`)
          },
        },
      })
    },
    { initial: demoBootstrap, snapshot: demoSnapshot, mode, moleHistory, applications, analysis },
  )
}
