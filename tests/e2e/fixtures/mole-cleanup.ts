import type { Page } from '@playwright/test'
import { installTaskMock } from './native-task'
import { moleCleanScopes } from '../../../src/lib/mole-cleanup'
export async function installMoleCleanMock(page: Page, mode = 'normal') {
  await installTaskMock(page, 'new')
  await page.addInitScript(
    ({ scope, mode }) => {
      sessionStorage.setItem('mock-done', 'yes')
      const native = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
          }
        }
      ).__TAURI_INTERNALS__
      const original = native.invoke
      const id = '61d8c490-1245-414b-8d62-0c6aab303555'
      const plan = (system = false) => {
        const createdAt = Date.now()
        return {
          version: 1,
          id,
          revision: '308bf9911179ddfa40e5df9fdc42095a18ba5cfc',
          engineVersion: '1.65.0',
          home: '/Users/demo',
          system,
          method: 'molePermanent',
          protectedPaths: ['/Users/demo/Documents'],
          createdAt,
          expiresAt: createdAt + 120_000,
          scope,
        }
      }
      const events = () => [
        { seq: 1, kind: 'section', path: '', detail: 'Browsers', token: '' },
        { seq: 2, kind: 'intent', path: '/owned/cache', detail: 'safe_remove', token: id },
      ]
      const saved = () => JSON.parse(sessionStorage.getItem('mole-record') ?? 'null')
      if (mode === 'uncertain' && !saved())
        sessionStorage.setItem(
          'mole-record',
          JSON.stringify({
            id,
            plan: plan(),
            state: 'uncertain',
            createdAt: Date.now(),
            finishedAt: Date.now(),
            beforeBytes: 1024,
            afterBytes: null,
            error: '样例结果需核查',
            diagnostic: 'Mole fixture diagnostic',
            reviewedAt: null,
            events: events(),
            eventsTruncated: false,
          }),
        )
      native.invoke = async (command, args = {}) => {
        if (
          ![
            'get_mole_cleanup',
            'prepare_mole_cleanup',
            'discard_mole_cleanup',
            'execute_mole_cleanup',
            'cancel_mole_cleanup',
            'authorize_mole_cleanup',
            'acknowledge_mole_cleanup',
            'save_mole_cleanup_export',
            'clear_mole_cleanup_record',
          ].includes(command)
        )
          return original(command, args)
        const calls = JSON.parse(sessionStorage.getItem('mole-calls') ?? '[]')
        const { password, ...recorded } = args
        sessionStorage.setItem(
          'mole-calls',
          JSON.stringify([...calls, { command, args: recorded }]),
        )
        if (command === 'prepare_mole_cleanup') {
          const p = plan(Boolean(args.system))
          sessionStorage.setItem('mole-plan', JSON.stringify(p))
          return p
        }
        if (command === 'discard_mole_cleanup') return
        if (command === 'execute_mole_cleanup') {
          if (sessionStorage.getItem('mole-reject')) throw Error('保护规则已变化，请重新确认。')
          const p = JSON.parse(sessionStorage.getItem('mole-plan')!)
          if (
            args.planId !== p.id ||
            args.permanentConfirmed !== true ||
            args.trashConfirmed !== true
          )
            throw Error('wrong confirmation')
          sessionStorage.setItem(
            'mole-record',
            JSON.stringify({
              id: args.requestId,
              plan: p,
              state: 'running',
              createdAt: Date.now(),
              finishedAt: null,
              beforeBytes: 1024,
              afterBytes: null,
              error: null,
              diagnostic: '',
              reviewedAt: null,
              events: events(),
              eventsTruncated: false,
            }),
          )
          return
        }
        if (command === 'get_mole_cleanup') {
          const r = saved()
          if (sessionStorage.getItem('mole-read-error')) throw Error('样例进度暂时无法读取')
          const active = r?.state === 'running'
          return {
            version: 1,
            activeRequest: active ? r.id : null,
            authorizationRequired: !!(
              active &&
              r.plan.system &&
              mode === 'authorization' &&
              !sessionStorage.getItem('mole-authorized')
            ),
            records: r
              ? [
                  {
                    id: r.id,
                    state: r.state,
                    createdAt: r.createdAt,
                    finishedAt: r.finishedAt,
                    eventCount: r.events.length,
                    reviewedAt: r.reviewedAt,
                  },
                ]
              : [],
            current: r,
          }
        }
        if (command === 'authorize_mole_cleanup') {
          if (!password || args.requestId !== saved()?.id) throw Error('wrong authorization')
          sessionStorage.setItem('mole-authorized', 'yes')
          return
        }
        if (command === 'cancel_mole_cleanup') {
          const r = saved()
          if (args.requestId !== r.id) throw Error('wrong cancel request')
          sessionStorage.setItem(
            'mole-record',
            JSON.stringify({
              ...r,
              state: 'cancelled',
              finishedAt: Date.now(),
              events: [
                ...r.events,
                {
                  seq: 3,
                  kind: 'retained',
                  path: '/owned/cache',
                  detail: 'cancelled before mutation',
                  token: id,
                },
              ],
            }),
          )
          return
        }
        if (command === 'save_mole_cleanup_export')
          return sessionStorage.getItem('mole-save-cancelled') !== 'yes'
        if (command === 'acknowledge_mole_cleanup') {
          const r = saved()
          if (args.lastSeq !== r.events.at(-1).seq) throw Error('wrong history binding')
          sessionStorage.setItem('mole-record', JSON.stringify({ ...r, reviewedAt: Date.now() }))
          return
        }
        if (command === 'clear_mole_cleanup_record') {
          const r = saved()
          if (args.clearConfirmed !== true || !r.reviewedAt) throw Error('history not reviewed')
          sessionStorage.removeItem('mole-record')
          return
        }
      }
    },
    { scope: moleCleanScopes.map(([id]) => id), mode },
  )
}
