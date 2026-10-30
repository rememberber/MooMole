import { createRoot } from 'react-dom/client'
import { ScheduledFiles } from '../../../src/components/ScheduledFiles'
import { useScheduledFiles } from '../../../src/lib/use-scheduled-files'
import {
  scheduledFilesTransport,
  type ScheduledFileRule,
  type ScheduledFilePolicy,
  type ScheduledFileRecord,
} from '../../../src/lib/scheduled-files'
// Browser-only transport fixtures. Never schedules a native operation or changes files.
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'scheduled-files-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const policyId = '4a79a140-f83f-49f9-8ef2-f4e04688c4fa',
    draftId = '5a79a140-f83f-49f9-8ef2-f4e04688c4fa',
    request = '6a79a140-f83f-49f9-8ef2-f4e04688c4fa'
  const initial: ScheduledFileRule = {
    root: '/Users/demo/RuleSource',
    action: 'move',
    destination: '/Users/demo/RuleTarget',
    extensions: ['zip'],
    minimumAgeDays: 30,
    maximumFiles: 10,
    maximumBytes: 67108864,
    intervalMinutes: 1440,
  }
  let policies: ScheduledFilePolicy[] =
      mode === 'active' || mode === 'pause-failure'
        ? [
            {
              id: policyId,
              rule: initial,
              enabled: true,
              updatedAt: Date.now(),
              lastAttemptAt: null,
            },
          ]
        : [],
    records: ScheduledFileRecord[] = [],
    activeRequests: string[] = [],
    prepared: ScheduledFileRule = initial
  if (mode === 'active') {
    activeRequests = [request]
    records = [
      {
        id: request,
        policyId,
        rule: initial,
        trigger: 'scheduled',
        startedAt: Date.now(),
        finishedAt: null,
        state: 'running',
        matchedFiles: 1,
        selectedFiles: 1,
        heldByLimits: 0,
        executionId: null,
        reason: 'fixture running',
      },
    ]
  }
  const calls: { command: string; args: Record<string, unknown> }[] = []
  host.dataset.calls = '[]'
  let chooseCount = 0
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'get_scheduled_files')
          return { policies, records, activeRequests, error: null }
        if (command === 'prepare_scheduled_files') {
          prepared = (args.edit as { rule: ScheduledFileRule }).rule
          const createdAt = Date.now() - 1000
          return {
            id: draftId,
            policyId: (args.edit as { id: string | null }).id,
            rule: mode === 'mismatch' ? { ...prepared, root: '/other' } : prepared,
            createdAt,
            expiresAt: createdAt + 120_000,
            matchedFiles: mode === 'zero' ? 0 : 1,
            selected:
              mode === 'zero'
                ? []
                : [
                    {
                      id: '7a79a140-f83f-49f9-8ef2-f4e04688c4fa',
                      path: `${prepared.root}/old.zip`,
                      name: 'old.zip',
                      size: 123,
                      modifiedAt: createdAt - 40 * 86400000,
                    },
                  ],
            heldByLimits: 0,
          }
        }
        if (command === 'enable_scheduled_files') {
          policies = [
            {
              id: policyId,
              rule: prepared,
              enabled: true,
              updatedAt: Date.now(),
              lastAttemptAt: null,
            },
          ]
          return mode === 'saved-mismatch'
            ? { ...policies[0], rule: { ...prepared, root: '/other' } }
            : policies[0]
        }
        if (command === 'pause_scheduled_files') {
          policies = policies.map((p) => ({ ...p, enabled: false }))
          activeRequests = []
          if (mode === 'pause-failure')
            throw Error('本次运行已停止，但暂停设置尚未保存，请核对后重试。')
          return
        }
        if (command === 'remove_scheduled_files') {
          policies = []
          activeRequests = []
          return
        }
        if (command === 'run_scheduled_files') {
          const id = String(args.requestId)
          activeRequests = [id]
          records = [
            {
              id,
              policyId,
              rule: prepared,
              trigger: 'manual',
              startedAt: Date.now(),
              finishedAt: null,
              state: 'running',
              matchedFiles: 1,
              selectedFiles: 1,
              heldByLimits: 0,
              executionId: null,
              reason: 'fixture running',
            },
          ]
          return
        }
        if (command === 'cancel_task') {
          activeRequests = []
          policies = policies.map((p) => ({ ...p, enabled: false }))
          records = records.map((r) => ({
            ...r,
            state: 'paused',
            finishedAt: Date.now(),
            reason: '已停止本次处理，规则已暂停。',
          }))
          return
        }
        if (command === 'discard_scheduled_files') return
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const c = useScheduledFiles(true, scheduledFilesTransport)
    return (
      <ScheduledFiles
        enabled
        controller={c}
        chooseDirectory={async () =>
          ++chooseCount % 2 ? '/Users/demo/RuleSource' : '/Users/demo/RuleTarget'
        }
        onHistory={() => {
          host.dataset.history = 'yes'
        }}
      />
    )
  }
  createRoot(host).render(<Fixture />)
}
