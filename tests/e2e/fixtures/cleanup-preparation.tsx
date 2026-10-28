import { useState, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import { ReviewPlanDialog } from '../../../src/components/ReviewPlanDialog'
import type { ReviewPlan } from '../../../src/types'
import { clearPermissions } from './permission-observation'

export function mount(mode = 'normal') {
  const now = Date.now()
  const initial: ReviewPlan = {
    id: 'log-plan',
    scanId: 'log-scan',
    createdAt: now,
    checkedAt: now,
    expiresAt: now + 900_000,
    itemCount: 2,
    readOnly: true,
    requiresMoleRecheck: true,
    items: ['session.log', 'shared-cache'].map((name, index) => ({
      id: `item-${index}`,
      path: `/Users/demo/Library/${index ? 'Caches' : 'Logs'}/${name}`,
      section: 'fixture',
      size: 512,
      state: 'unchanged',
      reason: '本次文件信息未见变化。',
      permissions: clearPermissions,
    })),
  }
  function Fixture() {
    const [closed, setClosed] = useState(false)
    const attempts = useRef(0)
    if (closed) return null
    return (
      <ReviewPlanDialog
        initialPlan={initial}
        onExecute={
          mode === 'all-retained'
            ? async () => {
                throw new Error('retained scope must never execute')
              }
            : undefined
        }
        onClose={() => setClosed(true)}
        onRefresh={async () => {
          await new Promise((resolve) => setTimeout(resolve, 250))
          if (mode === 'refresh-error') throw new Error('本地复核读取失败')
          return { ...initial, checkedAt: Date.now() }
        }}
        onCheckRules={async (_, signal) => {
          attempts.current++
          if (mode === 'cancel' && attempts.current > 1)
            await new Promise((_, reject) =>
              signal.addEventListener('abort', () => reject(new Error('规则复查已取消')), {
                once: true,
              }),
            )
          const checked = Date.now()
          return {
            ...initial,
            checkedAt: checked,
            ruleCheckedAt: checked,
            items: initial.items.map((item) => ({
              ...item,
              moleRule: {
                code: 'passed',
                source: 'fixture Mole',
                reason: '本次路径检查未要求保留。',
                module: {
                  version: 2,
                  binding: 'userLogs',
                  scope: 'userLogs',
                  code: 'checked',
                  source: 'fixture',
                  reason: '日志模块已复查。',
                },
                activity: {
                  version: 1,
                  code: 'noOpenHandles',
                  source: 'fixture',
                  reason: '本次未发现打开句柄。',
                },
              },
            })),
            cleanupPreparation: {
              version: mode === 'invalid' ? 9 : 2,
              planId: initial.id,
              scope: 'userLogFiles',
              method: 'trash',
              readOnly: true,
              checkedAt: checked,
              expiresAt: checked + 30_000,
              observedCount: mode === 'all-retained' ? 0 : 1,
              authorizationCount: 0,
              items: [
                {
                  id: 'item-0',
                  code: mode === 'all-retained' ? 'retained' : 'observed',
                  reason: '本轮日志规则、权限及原生对象准备已核对；执行前仍需重新验证。',
                },
                {
                  id: 'item-1',
                  code: 'retained',
                  reason: '当前只准备来源明确的用户日志普通文件；此缓存保留。',
                },
              ],
            },
          }
        }}
      />
    )
  }
  createRoot(document.getElementById('root')!).render(<Fixture />)
}
