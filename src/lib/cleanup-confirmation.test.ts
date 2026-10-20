import { describe, expect, it } from 'vitest'
import { confirmationPlan } from '../../tests/fixtures/log-confirmation'
import { cleanupConfirmation } from './cleanup-confirmation'
import type { ReviewPlan } from '../types'

describe('server-owned log confirmation display', () => {
  it('requires current preparation and exact ordered scope', () => {
    const plan = confirmationPlan()
    expect(cleanupConfirmation(plan, 1001)).toBe(plan.cleanupConfirmation)
    for (const now of [999, 31000, 900100]) expect(cleanupConfirmation(plan, now)).toBeNull()
    plan.cleanupPreparation = null
    expect(cleanupConfirmation(plan, 1001)).toBeNull()
  })
  it('withdraws malformed or expanded offers', () => {
    const changes: ((p: ReviewPlan) => void)[] = [
      (p) => {
        p.cleanupConfirmation!.id = 'arbitrary'
      },
      (p) => {
        p.cleanupConfirmation!.version = 9
      },
      (p) => {
        p.cleanupConfirmation!.method = 'delete'
      },
      (p) => {
        p.cleanupConfirmation!.snapshotSha256 = ''
      },
      (p) => {
        p.cleanupConfirmation!.planId = 'other'
      },
      (p) => {
        p.cleanupConfirmation!.createdAt = 999
      },
      (p) => {
        p.cleanupConfirmation!.expiresAt++
      },
      (p) => {
        p.cleanupConfirmation!.items[0].path = '/another'
      },
      (p) => {
        p.cleanupConfirmation!.items[0].sizeBytes = Number.MAX_SAFE_INTEGER + 1
      },
      (p) => {
        p.cleanupConfirmation!.items.push(p.cleanupConfirmation!.items[0])
      },
      (p) => {
        p.cleanupConfirmation!.items = []
      },
      (p) => {
        p.cleanupConfirmation!.retainedCount = 0
      },
      (p) => {
        p.items[0].state = 'protected'
      },
    ]
    for (const change of changes) {
      const plan = confirmationPlan()
      change(plan)
      expect(cleanupConfirmation(plan, 1001)).toBeNull()
    }
  })
})
