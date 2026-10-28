import type { Page } from '@playwright/test'
import { demoBootstrap } from '../../../src/lib/demo'
import { cancelled } from './execution-history'

declare global {
  interface Window {
    rejectLiveCancel: () => void
  }
}

// Only browser transport fixtures: no native files, SQLite or mutation calls.
export async function installLiveExecutionMock(page: Page) {
  await page.addInitScript(
    ({ initial, terminal }) => {
      let rejectCancel: ((error: Error) => void) | undefined
      const running = {
        ...terminal,
        state: 'running',
        finishedAt: null,
        items: [terminal.items[0], { ...terminal.items[1], state: 'pending', reason: '尚未开始' }],
      }
      const current = () => (sessionStorage.getItem('live-finished') ? terminal : running)
      Object.assign(window, {
        rejectLiveCancel: () => rejectCancel?.(new Error('fixture 迟到的取消失败')),
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (command: string, args: { runId?: string; itemId?: string } = {}) => {
            const calls = JSON.parse(sessionStorage.getItem('live-calls') || '[]')
            sessionStorage.setItem('live-calls', JSON.stringify([...calls, { command, ...args }]))
            if (command === 'get_ai_scan') return null
            if (command === 'bootstrap') return { ...initial, executions: [current()] }
            if (command === 'list_executions') {
              if (sessionStorage.getItem('live-list-error')) throw new Error('fixture 列表读取失败')
              return [current()]
            }
            if (command === 'get_execution') {
              if (args.runId !== terminal.id) throw new Error('unknown ID')
              if (sessionStorage.getItem('live-detail-error'))
                throw new Error('fixture 详情读取失败')
              return current()
            }
            if (command === 'reveal_execution_item') {
              if (args.runId !== terminal.id || args.itemId !== terminal.items[0].id)
                throw new Error('wrong receipt identity')
              if (sessionStorage.getItem('live-reveal-error'))
                throw new Error('记录位置已不存在或无法访问（fixture）；原记录保持不变。')
              return
            }
            if (command === 'cancel_log_cleanup') {
              if (args.runId !== terminal.id) throw new Error('wrong batch')
              if (sessionStorage.getItem('live-cancel-pending'))
                await new Promise<void>((_, reject) => {
                  rejectCancel = reject
                })
              if (sessionStorage.getItem('live-cancel-error'))
                throw new Error('fixture 取消请求失败')
              sessionStorage.setItem('live-cancelled', 'yes')
              return
            }
            if (
              [
                'get_history',
                'list_review_plans',
                'list_application_uninstalls',
                'list_quicklook_refreshes',
              ].includes(command)
            )
              return []
            throw new Error(`Unexpected fixture command: ${command}`)
          },
        },
      })
    },
    { initial: demoBootstrap, terminal: cancelled },
  )
}
