import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ReviewPlanDialog } from '../../../src/components/ReviewPlanDialog'
import { useMotionVisibility } from '../../../src/components/TaskMotion'
import type { ExecutionDetail } from '../../../src/types'
import { confirmationPlan } from '../../fixtures/log-confirmation'

// UI-only evidence: this fixture never invokes a native operation.
export function mount() {
  const plan = confirmationPlan()
  const now = Date.now()
  Object.assign(plan, {
    createdAt: now,
    checkedAt: now,
    ruleCheckedAt: now,
    expiresAt: now + 900_000,
  })
  Object.assign(plan.cleanupPreparation!, {
    checkedAt: now,
    expiresAt: now + 30_000,
    observedCount: 2,
    items: plan.items.map(({ id }) => ({ id, code: 'observed', reason: 'fixture' })),
  })
  Object.assign(plan.cleanupConfirmation!, {
    createdAt: now,
    expiresAt: now + 30_000,
    retainedCount: 0,
    items: plan.items.map(({ id, path, size }) => ({ id, path, sizeBytes: size! })),
  })
  const initial: ExecutionDetail = {
    id: 'motion-run',
    planId: plan.id,
    method: 'trash',
    state: 'running',
    createdAt: now + 1,
    finishedAt: null,
    itemCount: 2,
    movedCount: 0,
    uncertainCount: 0,
    retainedForReview: false,
    reviewedAt: null,
    revision: 'fixture',
    protocol: 2,
    snapshotSha256: plan.cleanupConfirmation!.snapshotSha256,
    items: plan.items.map(({ id, path }) => ({
      id,
      path,
      state: 'pending',
      reason: 'fixture',
      receipt: null,
    })),
    events: [],
  }
  function Fixture() {
    useMotionVisibility()
    const [execution, setExecution] = useState(initial)
    const finish = useRef<(value: ExecutionDetail) => void>(undefined)
    useEffect(() => {
      const update = (event: Event) => {
        const { movedCount, state = 'running', uncertainCount = 0 } = (event as CustomEvent).detail
        const next = {
          ...initial,
          movedCount,
          state,
          uncertainCount,
          items: initial.items.map((item, index) => ({
            ...item,
            state: index < movedCount ? 'moved' : uncertainCount ? 'uncertain' : 'pending',
          })),
        }
        setExecution(next)
        if (state !== 'running') finish.current?.(next)
      }
      window.addEventListener('motion-progress', update)
      return () => window.removeEventListener('motion-progress', update)
    }, [])
    return (
      <ReviewPlanDialog
        initialPlan={plan}
        executions={[execution]}
        onRefresh={async () => plan}
        onClose={() => {}}
        onExecute={async () =>
          new Promise<ExecutionDetail>((resolve) => {
            finish.current = resolve
          })
        }
      />
    )
  }
  const host = document.createElement('div')
  document.body.append(host)
  createRoot(host).render(<Fixture />)
}
