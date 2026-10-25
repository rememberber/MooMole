import type { Page } from '@playwright/test'
import { demoBootstrap } from '../../../src/lib/demo'
import type { ExecutionDetail } from '../../../src/types'
import { interrupted, cancelled } from './execution-history'

// Browser-only journal fixtures, never inserted into a native database.
const allResults: ExecutionDetail = {
  ...interrupted,
  id: '6d048c7d-d222-410c-875d-a6e3cef18004',
  itemCount: 2,
  movedCount: 2,
  uncertainCount: 0,
  items: [
    interrupted.items[0],
    {
      ...interrupted.items[0],
      id: 'item-1',
      path: '/fixture/second',
      receipt: { ...interrupted.items[0].receipt!, destination: '/fixture/recovery/second' },
    },
  ],
  events: interrupted.events.slice(0, 2),
}
const completed = { ...allResults, id: '6d048c7d-d222-410c-875d-a6e3cef18005', state: 'completed' }

export async function installRecoveryMock(
  page: Page,
  mode: 'uncertain' | 'interrupted' | 'terminal',
) {
  const records =
    mode === 'uncertain'
      ? [interrupted, allResults, cancelled]
      : mode === 'interrupted'
        ? [allResults, cancelled]
        : [completed, cancelled]
  await page.addInitScript(
    ({ initial, records }) => {
      const current = () =>
        sessionStorage.getItem('recovery-updated')
          ? records.filter((r) => r.state !== 'interrupted')
          : records
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (command: string, args: { runId?: string } = {}) => {
            const calls = JSON.parse(sessionStorage.getItem('recovery-calls') ?? '[]')
            sessionStorage.setItem('recovery-calls', JSON.stringify([...calls, command]))
            if (command === 'bootstrap') return { ...initial, executions: current() }
            if (
              [
                'get_history',
                'list_review_plans',
                'list_application_uninstalls',
                'list_quicklook_refreshes',
              ].includes(command)
            )
              return []
            if (command === 'list_executions') {
              if (sessionStorage.getItem('recovery-list-error'))
                throw new Error('fixture 执行记录暂不可读')
              return current()
            }
            if (command === 'get_execution') {
              const record = current().find((r) => r.id === args.runId)
              if (!record) throw new Error('Unknown fixture execution')
              return record
            }
            throw new Error(`Unexpected test command: ${command}`)
          },
        },
      })
    },
    { initial: demoBootstrap, records },
  )
}
