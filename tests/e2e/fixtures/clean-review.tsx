import { useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { clearPermissions } from './permission-observation'
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
  const fixturePreview =
    mode === 'old-scan'
      ? { ...legacyPreview, protocol: 1 as const }
      : mode === 'legacy'
        ? legacyPreview
        : { ...preview }
  const [protectedPaths, setProtectedPaths] = useState<string[]>([])
  if (mode === 'origins')
    fixturePreview.items = fixturePreview.items.map((item, index) =>
      index === 0
        ? {
            ...item,
            provenance: {
              version: 1,
              observations: [
                ...item.provenance!.observations,
                {
                  path: item.path,
                  functions: [
                    'record_dry_run_cleanup_target',
                    '_safe_clean_impl',
                    'safe_clean',
                    'fixture_other_module',
                  ],
                  deleteGuard: '',
                  finalGuard: '',
                },
              ],
            },
          }
        : item,
    )
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
                permissions: clearPermissions,
                impact: item.impact,
                provenance: item.provenance,
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
                permissions: {
                  version: 1,
                  code: 'notChecked',
                  reason: 'fixture：目标变化，本次未检查权限。',
                  findings: [],
                },
                reason: '文件身份已变化，需要重新扫描。',
              })),
            }
          }}
          onCheckRules={async (_, signal) => {
            attempts.current++
            if (
              mode === 'cancel' ||
              (['module-cancel', 'activity', 'descendants'].includes(mode) && attempts.current > 1)
            )
              await new Promise((_, reject) =>
                signal.addEventListener('abort', () => reject(new Error('规则复查已取消。')), {
                  once: true,
                }),
              )
            if (mode === 'activity' || mode === 'activity-budget')
              return {
                ...plan,
                ruleCheckedAt: Date.now(),
                items: plan.items.map((item, index) => ({
                  ...item,
                  state:
                    index === 0 ? 'unchanged' : index === 1 ? 'activityBusy' : 'activityUnknown',
                  reason: index === 0 ? '本次元数据复核未发现变化。' : '附加占用观察要求重新确认。',
                  moleRule: {
                    code: 'passed',
                    source: 'Mole 路径级保护检查',
                    reason: '本次路径级检查未要求保留。',
                    module: {
                      version: 2,
                      binding: 'userCache',
                      scope: 'userCache',
                      code: 'checked',
                      source: 'fixture module',
                      reason: '仅适用模块条件已复查。',
                    },
                    activity: {
                      version: 1,
                      code:
                        index === 0
                          ? 'noOpenHandles'
                          : index === 1
                            ? 'busy'
                            : mode === 'activity-budget'
                              ? 'budgetExhausted'
                              : 'unknown',
                      source: '客户端附加观察 · Mole _mole_container_cache_has_open_handle',
                      reason:
                        index === 0
                          ? '本次未发现打开句柄；不是持续空闲保证，也不是执行许可。'
                          : index === 1
                            ? '发现所选目录后代有打开句柄，应保留；不自动关闭任何应用。'
                            : mode === 'activity-budget'
                              ? '本批附加占用检查的 5 秒预算已用完，此项尚未确认；请缩小选择后复查。'
                              : '进程视图不完整，无法可靠确认占用，应保留。',
                    },
                  },
                })),
              }
            if (mode === 'descendants')
              return {
                ...plan,
                ruleCheckedAt: Date.now(),
                items: plan.items.map((item, index) => ({
                  ...item,
                  state:
                    index === 0
                      ? 'descendantProtected'
                      : index === 1
                        ? 'descendantUnavailable'
                        : 'unchanged',
                  reason:
                    index === 0
                      ? '目录后代要求保留。'
                      : index === 1
                        ? '后代检查未完成。'
                        : '本次元数据复核未发现变化。',
                  descendantRules: {
                    version: 1,
                    code: index === 0 ? 'protected' : index === 1 ? 'unavailable' : 'checked',
                    checkedEntries: index === 0 ? 2 : index === 1 ? 0 : 3,
                    path: index === 0 ? item.path + '/deep/保留 中文\n[*].log' : null,
                    source: index === 0 ? 'Mole 有效白名单' : null,
                    reason:
                      index === 0
                        ? '目录后代要求保留或未能确认：路径与当前 Mole 白名单匹配，应保留。'
                        : index === 1
                          ? '后代路径超过本批规则检查的剩余额度；本次未完成后代保护检查。'
                          : '本次已逐项复查后代路径保护；不包含全部后代模块规则，也不是执行许可。',
                  },
                })),
              }
            if (mode === 'origins')
              return {
                ...plan,
                ruleCheckedAt: Date.now(),
                items: plan.items.map((item) => ({
                  ...item,
                  state: 'moduleUnsupported',
                  reason: '同一对象存在混合模块来源。',
                  moleRule: {
                    code: 'passed',
                    source: 'Mole 路径级保护检查',
                    reason: '本次路径级检查未要求保留。',
                    module: {
                      version: 2,
                      binding: 'mixed',
                      scope: 'unsupported',
                      code: 'unsupported',
                      source: '尚未适配的调用来源',
                      reason: '同一对象存在混合模块来源，不能只按路径选择检查。',
                    },
                  },
                })),
              }
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
                      version: 2,
                      binding: index === 0 ? 'userCache' : index === 1 ? 'mixed' : 'userLogs',
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
