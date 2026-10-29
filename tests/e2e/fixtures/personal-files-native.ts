import type { Page } from '@playwright/test'
import { demoBootstrap, demoSnapshot } from '../../../src/lib/demo'

// UI transport fixture only: no native file reads, moves, rules or SQLite.
export async function installPersonalMock(page: Page, pendingScan = false, duplicates = false) {
  await page.addInitScript(
    ({ initial, status, pendingScan, duplicates }) => {
      const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
      const now = Date.now(),
        root = '/Users/demo/Downloads'
      const original: any = {
        id: uuid(1),
        root,
        createdAt: now - 1000,
        expiresAt: now + 899000,
        complete: false,
        cancelled: false,
        skipped: 2,
        visited: 6,
        items: [
          {
            id: uuid(2),
            path: `${root}/old.zip`,
            name: 'old.zip',
            size: 200 * 1024 ** 2,
            modifiedAt: now - 100 * 86400000,
          },
          {
            id: uuid(3),
            path: `${root}/recent.pdf`,
            name: 'recent.pdf',
            size: 400 * 1024 ** 2,
            modifiedAt: now,
          },
          {
            id: uuid(4),
            path: `${root}/notes.txt`,
            name: 'notes.txt',
            size: 1024,
            modifiedAt: now - 400 * 86400000,
          },
          {
            id: uuid(5),
            path: `${root}/protected.mov`,
            name: 'protected.mov',
            size: 2 * 1024 ** 3,
            modifiedAt: now - 500 * 86400000,
          },
        ],
      }
      if (duplicates) {
        original.items = [2, 3, 4].map((n, index) => ({
          id: uuid(n),
          path: `${root}/${['one.txt', 'two.txt', 'keep.txt'][index]}`,
          name: ['one.txt', 'two.txt', 'keep.txt'][index],
          size: 65536,
          modifiedAt: now - 86400000,
        }))
        original.duplicates = {
          groups: [
            {
              id: 'a'.repeat(64),
              size: 65536,
              itemIds: original.items.map((i: { id: string }) => i.id),
            },
          ],
          bytesRead: 262144,
          checkedFiles: 4,
          extraBytes: 131072,
        }
      }
      const read = () =>
        JSON.parse(sessionStorage.getItem('personal-state') || 'null') || {
          phase: 'idle',
          requestId: null,
          runId: null,
          report: null,
          error: null,
        }
      const save = (v: unknown) => sessionStorage.setItem('personal-state', JSON.stringify(v))
      const scan = () => JSON.parse(sessionStorage.getItem('personal-report') || 'null') || original
      let task: { id: string; kind: string } | null = null
      const result = () => {
        const state = read(),
          items = JSON.parse(sessionStorage.getItem('personal-selected') || '[]')
        const movement = JSON.parse(sessionStorage.getItem('personal-movement') || 'null')
        const interrupted = !!sessionStorage.getItem('personal-interrupted')
        const terminal = state.phase === 'completed' || interrupted
        return {
          id: uuid(7),
          planId: uuid(6),
          method: movement ? 'move' : 'trash',
          state: interrupted
            ? 'interrupted'
            : terminal
              ? sessionStorage.getItem('personal-cancelled')
                ? 'cancelled'
                : 'completed'
              : 'running',
          createdAt: now,
          finishedAt: terminal ? now + 1 : null,
          itemCount: items.length,
          movedCount:
            terminal && !interrupted && !sessionStorage.getItem('personal-cancelled')
              ? items.length
              : 1,
          uncertainCount: interrupted ? 1 : 0,
          retainedForReview: true,
          reviewedAt: null,
          revision: 'MooMole.personal-files.v1',
          protocol: 1,
          origin: duplicates ? 'duplicateFiles' : 'personalFiles',
          snapshotSha256: 'a'.repeat(64),
          spaceObservation: null,
          events: [],
          items: items.map((item: { id: string; path: string; size: number }, index: number) => ({
            id: item.id,
            path: item.path,
            scanSizeBytes: item.size,
            targetPath:
              movement?.targets.find((t: { id: string }) => t.id === item.id)?.destination ?? null,
            state:
              interrupted && index === 1
                ? 'uncertain'
                : index === 0 || (terminal && !sessionStorage.getItem('personal-cancelled'))
                  ? 'moved'
                  : terminal
                    ? 'cancelled'
                    : 'pending',
            reason: 'fixture result',
            receipt:
              interrupted && index === 1
                ? null
                : index === 0 || (terminal && !sessionStorage.getItem('personal-cancelled'))
                  ? {
                      destination: movement
                        ? movement.targets.find((t: { id: string }) => t.id === item.id).destination
                        : `/Users/demo/.Trash/${item.path.split('/').at(-1)}`,
                      device: '1',
                      inode: String(index + 2),
                    }
                  : null,
          })),
        }
      }
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (command: string, args: Record<string, unknown> = {}) => {
            const calls = JSON.parse(sessionStorage.getItem('personal-calls') || '[]')
            sessionStorage.setItem('personal-calls', JSON.stringify([...calls, { command, args }]))
            if (command === 'bootstrap')
              return {
                ...initial,
                settings: { ...initial.settings, protectedPaths: [`${root}/protected.mov`] },
                engine: { ...initial.engine, available: true, version: '1.65.0' },
                executions: read().runId ? [result()] : [],
              }
            if (command === 'get_ai_scan') return null
            if (command === 'personal_move_strategy') return 'sameVolume'
            if (command === 'get_personal_transfers') return { records: [], activeRequests: [] }
            if (command === 'get_folder_schedules')
              return { watches: [], records: [], activeRequests: [], error: null }
            if (
              [
                'get_history',
                'list_review_plans',
                'list_application_uninstalls',
                'list_quicklook_refreshes',
              ].includes(command)
            )
              return []
            if (command === 'list_executions') return read().runId ? [result()] : []
            if (command === 'plugin:event|listen') throw Error('fixture polling channel')
            if (command === 'start_task') {
              task = { id: String(args.taskId), kind: String(args.kind) }
              return
            }
            if (command === 'get_task_snapshot' && task?.id === args.taskId)
              return {
                id: task.id,
                kind: task.kind,
                state: 'succeeded',
                error: null,
                result: status,
              }
            if (command === 'get_personal_files' || command === 'get_duplicate_files') {
              const state = read()
              if (duplicates && state.phase === 'processing') {
                state.phase = 'completed'
                save(state)
              }
              if (state.phase === 'scanning' && sessionStorage.getItem('personal-finish-scan')) {
                state.phase = 'ready'
                state.report = scan()
                save(state)
              }
              return state
            }
            if (command === 'start_personal_scan' || command === 'start_duplicate_scan') {
              if (args.root !== root) throw Error('unexpected root')
              sessionStorage.removeItem('personal-movement')
              sessionStorage.removeItem('personal-executed')
              sessionStorage.removeItem('personal-cancelled')
              sessionStorage.setItem('personal-report', JSON.stringify(original))
              save({
                phase: pendingScan ? 'scanning' : 'ready',
                requestId: args.requestId,
                runId: null,
                report: pendingScan ? null : original,
                error: null,
              })
              return
            }
            if (
              command === 'prepare_personal_trash' ||
              command === 'prepare_personal_move' ||
              command === 'prepare_duplicate_trash'
            ) {
              const source = scan()
              if (args.scanId !== source.id) throw Error('wrong source scan')
              const ids = args.selectedIds as string[]
              const items = source.items.filter((item: { id: string }) => ids.includes(item.id))
              if (items.length !== ids.length || !items.length) throw Error('wrong exact IDs')
              if (duplicates && ids.length >= source.items.length) throw Error('must retain a copy')
              sessionStorage.setItem('personal-selected', JSON.stringify(items))
              const movement =
                command === 'prepare_personal_move'
                  ? {
                      directory: args.destination,
                      conflict: args.conflict,
                      targets: items.map((item: { id: string; name: string }) => ({
                        id: item.id,
                        destination:
                          item.name === 'old.zip' && args.conflict === 'skip'
                            ? null
                            : `${args.destination}/${item.name === 'old.zip' ? 'old (1).zip' : item.name}`,
                      })),
                    }
                  : undefined
              if (movement) sessionStorage.setItem('personal-movement', JSON.stringify(movement))
              else sessionStorage.removeItem('personal-movement')
              return {
                id: uuid(6),
                root,
                createdAt: Date.now(),
                expiresAt: Date.now() + 30000,
                items,
                ...(movement ? { movement } : {}),
                ...(duplicates
                  ? {
                      keptFiles: source.items
                        .filter((i: { id: string }) => !ids.includes(i.id))
                        .slice(0, 1),
                    }
                  : {}),
              }
            }
            if (command === 'discard_personal_trash' || command === 'discard_duplicate_trash')
              return
            if (
              command === 'execute_personal_trash' ||
              command === 'execute_personal_move' ||
              command === 'execute_duplicate_trash'
            ) {
              if (
                args.confirmationId !== uuid(6) ||
                args.personalFilesConfirmed !== true ||
                sessionStorage.getItem('personal-executed')
              )
                throw Error('duplicate or unconfirmed operation')
              sessionStorage.setItem('personal-executed', 'yes')
              save({
                phase: 'processing',
                requestId: args.requestId,
                runId: uuid(7),
                report: null,
                error: null,
              })
              return
            }
            if (command === 'get_execution' && args.runId === uuid(7)) return result()
            if (command === 'cancel_task') {
              const state = read()
              if (state.requestId !== args.taskId) throw Error('wrong task cancellation')
              if (state.phase === 'scanning') {
                const report = scan()
                report.complete = false
                report.cancelled = true
                state.report = report
                state.phase = 'ready'
              } else {
                sessionStorage.setItem('personal-cancelled', 'yes')
                state.phase = 'completed'
              }
              save(state)
              return
            }
            if (command === 'reveal_execution_item' || command === 'reveal_path') return
            throw Error(`Unexpected personal fixture command: ${command}`)
          },
        },
      })
    },
    { initial: demoBootstrap, status: demoSnapshot, pendingScan, duplicates },
  )
}
