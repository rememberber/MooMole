import { useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { CleanReview } from '../../../src/components/CleanReview'
import { demoCleanPreview } from '../../../src/lib/demo'
import { ReviewPlanDialog } from '../../../src/components/ReviewPlanDialog'
import type { ReviewPlan } from '../../../src/types'

const preview = {
  ...demoCleanPreview,
  scanId: 'fixture-scan',
  scannedAt: Date.now(),
  expiresAt: Date.now() + 900_000,
}
const legacyPreview = {
  ...preview,
  items: preview.items.map((item) => ({ ...item, impact: undefined })),
}

function Fixture({ mode }: { mode: string }) {
  const fixturePreview = mode === 'legacy' ? legacyPreview : preview
  const [protectedPaths, setProtectedPaths] = useState<string[]>([])
  const attempts = useRef(0)
  const [plan, setPlan] = useState<ReviewPlan | null>(null)
  return (
    <>
      <CleanReview
        preview={fixturePreview}
        protectedPaths={protectedPaths}
        home="/Users/demo"
        desktop
        onProtect={async (path) => {
          setProtectedPaths((current) => [...current, path])
        }}
        onReveal={() => {}}
        onCreatePlan={async (scanId, ids) => {
          setPlan({
            id: 'fixture-plan',
            scanId,
            itemCount: ids.length,
            createdAt: Date.now(),
            expiresAt: preview.expiresAt,
            checkedAt: Date.now(),
            readOnly: true,
            requiresMoleRecheck: true,
            items: fixturePreview.items
              .filter((item) => ids.includes(item.id))
              .map((item) => ({
                id: item.id,
                path: item.path,
                size: item.size,
                section: item.section,
                state: 'unchanged',
                reason: '本次元数据未发现变化。',
                impact: item.impact,
                inventoryEntries: item.isDirectory ? 3 : null,
              })),
          })
        }}
      />
      {plan && (
        <ReviewPlanDialog
          initialPlan={plan}
          onClose={() => setPlan(null)}
          onRefresh={async () => {
            if (mode === 'modules') throw new Error('fixture 本地复核读取失败')
            return {
              ...plan,
              checkedAt: Date.now(),
              items: plan.items.map((item) => ({
                ...item,
                state: 'changed',
                reason: '文件身份已变化，需要重新扫描。',
              })),
            }
          }}
          onCheckRules={async (_, signal) => {
            attempts.current++
            if (mode === 'cancel' || (mode === 'module-cancel' && attempts.current > 1))
              await new Promise((_, reject) =>
                signal.addEventListener('abort', () => reject(new Error('规则复查已取消。')), {
                  once: true,
                }),
              )
            if (mode === 'modules' || mode === 'module-cancel')
              return {
                ...plan,
                ruleCheckedAt: Date.now(),
                items: plan.items.map((item, index) => ({
                  ...item,
                  state:
                    index === 0
                      ? 'unchanged'
                      : index === 1
                        ? 'moduleUnsupported'
                        : 'moduleRetained',
                  reason:
                    index === 0
                      ? '本次元数据复核未发现变化。'
                      : index === 1
                        ? '模块规则尚未适配。'
                        : '模块要求保留或无法确认。',
                  moleRule: {
                    code: 'passed',
                    source: 'Mole 路径级保护检查',
                    reason: '路径级检查未要求保留。',
                    module: {
                      version: 1,
                      scope: index === 0 ? 'userCache' : index === 1 ? 'unsupported' : 'userLogs',
                      code:
                        index === 0 ? 'checked' : index === 1 ? 'unsupported' : 'retainOrUnknown',
                      source:
                        index === 0
                          ? 'Mole _user_cache_deno_delete_guard'
                          : index === 1
                            ? '尚未适配'
                            : 'Mole _user_log_delete_guard',
                      reason:
                        index === 0
                          ? '仅 Deno 约束已复查，其他模块仍需单独核对。'
                          : index === 1
                            ? '此路径的模块规则尚未适配。'
                            : '模块要求保留或无法确认当前条件。',
                    },
                  },
                })),
              }
            return {
              ...plan,
              ruleCheckedAt: Date.now(),
              items: plan.items.map((item) => ({
                ...item,
                state: 'moleProtected',
                reason: '当前 Mole 白名单要求保留。',
                moleRule: {
                  code: 'whitelist',
                  source: 'Mole 有效白名单',
                  reason: '路径与当前有效规则匹配，应保留。',
                },
              })),
            }
          }}
        />
      )}
    </>
  )
}

// Imported only by Playwright through Vite; not an application route or IPC hook.
export function mount(mode = 'rules') {
  const container = document.createElement('div')
  container.id = 'review-test-fixture'
  container.style.cssText = 'max-width:900px;margin:20px'
  document.body.append(container)
  document.getElementById('root')!.style.display = 'none'
  createRoot(container).render(<Fixture mode={mode} />)
}
