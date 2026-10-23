import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ReviewPlanDialog } from '../../../src/components/ReviewPlanDialog'
import { confirmationPlan } from '../../fixtures/log-confirmation'
import { cancelled } from './execution-history'

export function mount(mode = 'normal') {
  const plan = confirmationPlan()
  const now = Date.now()
  plan.createdAt = now
  plan.checkedAt = now
  plan.ruleCheckedAt = now
  plan.expiresAt = now + 900000
  plan.cleanupPreparation!.checkedAt = now
  plan.cleanupPreparation!.expiresAt = now + 30000
  plan.cleanupConfirmation!.createdAt = now
  plan.cleanupConfirmation!.expiresAt = now + 30000
  if (mode === 'invalid') plan.cleanupConfirmation!.items[0].path = '/wrong'
  if (mode === 'authorization' || mode === 'authorization-cancel') {
    plan.cleanupPreparation!.observedCount = 0
    plan.cleanupPreparation!.authorizationCount = 1
    plan.cleanupPreparation!.items[0].code = 'authorizationRequired'
    plan.cleanupPreparation!.items[0].reason = '占用尚未确认，需当次系统授权检查。'
    plan.items[0].state = 'activityUnknown'
    plan.items[0].moleRule!.activity!.code = 'unknown'
    plan.cleanupConfirmation!.requiresAuthorization = true
  }
  function Fixture() {
    const [calls, setCalls] = useState(0)
    const [discards, setDiscards] = useState(0)
    const [revealed, setRevealed] = useState('')
    const [closed, setClosed] = useState(false)
    return (
      <>
        <output aria-label="调用次数">{calls}</output>
        <output aria-label="撤销次数">{discards}</output>
        <output aria-label="定位收据">{revealed}</output>
        {!closed && (
          <ReviewPlanDialog
            initialPlan={plan}
            onClose={() => setClosed(true)}
            onRefresh={async () => plan}
            onRevealReceipt={async (runId, itemId) => {
              setRevealed(`${runId}/${itemId}`)
            }}
            onDiscardConfirmation={async () => {
              setDiscards((n) => n + 1)
            }}
            onExecute={async (id, signal) => {
              if (id !== plan.cleanupConfirmation!.id) throw new Error('unexpected ID')
              setCalls((n) => n + 1)
              if (mode === 'error') throw new Error('fixture 写入失败')
              if (mode === 'cancel' || mode === 'authorization-cancel')
                await new Promise<void>((resolve) =>
                  signal.addEventListener('abort', () => resolve(), { once: true }),
                )
              return cancelled
            }}
          />
        )}
      </>
    )
  }
  createRoot(document.getElementById('root')!).render(<Fixture />)
}
