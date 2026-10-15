import { useState } from 'react'
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

function Fixture({ mode }: { mode: string }) {
  const [protectedPaths, setProtectedPaths] = useState<string[]>([])
  const [plan, setPlan] = useState<ReviewPlan | null>(null)
  return (
    <>
      <CleanReview
        preview={preview}
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
            items: preview.items
              .filter((item) => ids.includes(item.id))
              .map((item) => ({
                id: item.id,
                path: item.path,
                size: item.size,
                section: item.section,
                state: 'unchanged',
                reason: '本次元数据未发现变化。',
              })),
          })
        }}
      />
      {plan && (
        <ReviewPlanDialog
          initialPlan={plan}
          onClose={() => setPlan(null)}
          onRefresh={async () => ({
            ...plan,
            checkedAt: Date.now(),
            items: plan.items.map((item) => ({
              ...item,
              state: 'changed',
              reason: '文件身份已变化，需要重新扫描。',
            })),
          })}
          onCheckRules={async (_, signal) => {
            if (mode === 'cancel')
              await new Promise((_, reject) =>
                signal.addEventListener('abort', () => reject(new Error('规则复查已取消。')), {
                  once: true,
                }),
              )
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
