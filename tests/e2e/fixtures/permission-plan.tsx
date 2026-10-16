import { createRoot } from 'react-dom/client'
import { ReviewPlanDialog } from '../../../src/components/ReviewPlanDialog'
import type { ReviewPlan } from '../../../src/types'
import {
  clearPermissions,
  restrictedPermissions,
  unknownPermissions,
} from './permission-observation'

// Browser-only reports. This fixture never opens files or a native database.
export function mount(mode: 'normal' | 'error' = 'normal') {
  const host = document.createElement('div')
  host.id = 'permission-plan-fixture'
  document.body.append(host)
  const root = createRoot(host)
  const plan: ReviewPlan = {
    id: '6d048c7d-d222-410c-875d-a6e3cef18901',
    scanId: 'permission-fixture',
    createdAt: Date.now() - 1000,
    checkedAt: Date.now(),
    expiresAt: Date.now() + 900_000,
    itemCount: 6,
    readOnly: true,
    requiresMoleRecheck: true,
    items: [
      ['clear', clearPermissions],
      ['restricted', restrictedPermissions],
      ['acl', unknownPermissions],
      ['legacy', undefined],
      ['future', { ...clearPermissions, version: 9 }],
      ['incomplete', { ...clearPermissions, findings: [] }],
    ].map(([name, permissions], i) => ({
      id: `permission:${i}`,
      path: `/fixture/${name}.log`,
      size: 1024,
      section: 'fixture',
      state: 'unchanged',
      reason: 'fixture：元数据未发现变化。',
      permissions: permissions as ReviewPlan['items'][number]['permissions'],
    })),
  }
  root.render(
    <ReviewPlanDialog
      initialPlan={plan}
      onClose={() => root.unmount()}
      onRefresh={async () => {
        await new Promise((resolve) => setTimeout(resolve, 150))
        if (mode === 'error') throw new Error('fixture 权限复核暂不可用')
        return {
          ...plan,
          checkedAt: Date.now(),
          items: plan.items.map((item) => ({ ...item, permissions: clearPermissions })),
        }
      }}
      onCheckRules={async (_, signal) => {
        await new Promise((_, reject) =>
          signal.addEventListener('abort', () => reject(new Error('fixture 规则复查已取消')), {
            once: true,
          }),
        )
        return plan
      }}
    />,
  )
}
