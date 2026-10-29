import type { Page } from '@playwright/test'
import { demoBootstrap, demoCleanPreview, demoSnapshot } from '../../../src/lib/demo'
import { confirmationPlan } from '../../fixtures/log-confirmation'

// App-level IPC transport only. Never invokes native code or touches real files.
export async function installSoftwareCleanupMock(page: Page) {
  const candidate = structuredClone(demoCleanPreview.items[0])
  candidate.id = 'chrome-cache'
  candidate.path = '/Users/demo/Library/Caches/com.google.Chrome'
  candidate.resolvedPath = candidate.path
  candidate.provenance!.observations[0].path = candidate.path
  const plan = confirmationPlan()
  plan.items = [
    {
      ...plan.items[0],
      id: candidate.id,
      path: candidate.path,
      size: candidate.size,
      impact: candidate.impact,
      inventoryEntries: 1,
      descendantRules: { version: 1, code: 'checked', checkedEntries: 1, reason: 'fixture' },
    },
  ]
  plan.items[0].moleRule!.module!.binding = 'userCache'
  plan.items[0].moleRule!.module!.scope = 'userCache'
  plan.itemCount = 1
  plan.cleanupPreparation!.scope = 'userCachesAndLogs'
  plan.cleanupPreparation!.items = [{ id: candidate.id, code: 'observed', reason: 'fixture' }]
  plan.cleanupConfirmation!.retainedCount = 0
  plan.cleanupConfirmation!.items = [
    { id: candidate.id, path: candidate.path, sizeBytes: candidate.size! },
  ]
  await page.addInitScript(
    ({ initial, preview, snapshot, candidate, plan }) => {
      let task: { id: string; kind: string } | null = null
      let completed: unknown = null
      const calls: { command: string; args: Record<string, unknown> }[] = []
      const fresh = () => {
        const now = Date.now()
        return {
          ...plan,
          createdAt: now - 100,
          checkedAt: now,
          ruleCheckedAt: now,
          expiresAt: now + 900000,
          cleanupPreparation: {
            ...plan.cleanupPreparation,
            checkedAt: now,
            expiresAt: now + 30000,
          },
          cleanupConfirmation: {
            ...plan.cleanupConfirmation,
            createdAt: now,
            expiresAt: now + 30000,
          },
        }
      }
      const scan = () => ({
        ...preview,
        items: [candidate],
        scanId: 'scan',
        scannedAt: Date.now() - 100,
        expiresAt: Date.now() + 900000,
      })
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (command: string, args: Record<string, unknown> = {}) => {
            calls.push({ command, args })
            sessionStorage.setItem('software-ipc', JSON.stringify(calls))
            if (command === 'bootstrap')
              return {
                ...initial,
                latestCleanPreview: scan(),
                engine: { ...initial.engine, available: true, version: '1.65.0' },
              }
            if (command === 'get_ai_scan') return null
            if (command === 'plugin:event|listen') throw new Error('fixture polling transport')
            if (
              command === 'get_history' ||
              command === 'list_application_uninstalls' ||
              command === 'list_quicklook_refreshes'
            )
              return []
            if (command === 'list_review_plans') return []
            if (command === 'list_executions') return completed ? [completed] : []
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
                result:
                  task.kind === 'status'
                    ? snapshot
                    : task.kind === 'apps'
                      ? []
                      : task.kind === 'clean'
                        ? scan()
                        : null,
              }
            if (command === 'create_review_plan') {
              if (
                args.scanId !== 'scan' ||
                JSON.stringify(args.selectedIds) !== JSON.stringify([candidate.id])
              )
                throw new Error('wrong selection')
              return { ...fresh(), cleanupConfirmation: null }
            }
            if (command === 'check_review_rules' && args.planId === plan.id) return fresh()
            if (command === 'discard_cleanup_confirmation') return
            if (
              command === 'execute_log_cleanup' &&
              args.confirmationId === plan.cleanupConfirmation!.id &&
              !completed
            ) {
              completed = {
                id: '6d048c7d-d222-410c-875d-a6e3cef18003',
                planId: plan.id,
                method: 'trash',
                state: 'completed',
                createdAt: Date.now(),
                finishedAt: Date.now(),
                itemCount: 1,
                movedCount: 1,
                uncertainCount: 0,
                retainedForReview: true,
                reviewedAt: null,
                revision: preview.revision,
                protocol: 2,
                snapshotSha256: 'a'.repeat(64),
                items: [
                  {
                    id: candidate.id,
                    path: candidate.path,
                    state: 'moved',
                    reason: 'fixture moved',
                    receipt: {
                      destination: '/Users/demo/.Trash/com.google.Chrome',
                      device: '1',
                      inode: '2',
                    },
                  },
                ],
                events: [],
              }
              return completed
            }
            throw new Error(`Unexpected software fixture command: ${command}`)
          },
        },
      })
    },
    { initial: demoBootstrap, preview: demoCleanPreview, snapshot: demoSnapshot, candidate, plan },
  )
}
