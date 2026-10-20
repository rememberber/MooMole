import type { ReviewPlan } from '../types'
import { cleanupPreparation } from './cleanup-preparation'

export function cleanupConfirmation(plan: ReviewPlan, now: number) {
  const offer = plan.cleanupConfirmation
  const report = plan.cleanupPreparation
  if (!offer || !report || !cleanupPreparation(plan, now)?.valid) return null
  const ids = report.items.filter((item) => item.code === 'observed').map((item) => item.id)
  if (
    offer.version !== 1 ||
    offer.method !== 'trash' ||
    offer.planId !== plan.id ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(offer.id) ||
    !/^[0-9a-f]{64}$/.test(offer.snapshotSha256) ||
    !Number.isSafeInteger(offer.createdAt) ||
    !Number.isSafeInteger(offer.expiresAt) ||
    offer.createdAt < report.checkedAt ||
    now < offer.createdAt ||
    now >= offer.expiresAt ||
    offer.expiresAt > report.expiresAt ||
    offer.expiresAt <= offer.createdAt ||
    !Array.isArray(offer.items) ||
    !ids.length ||
    ids.length > 20 ||
    offer.items.length !== ids.length ||
    offer.retainedCount !== plan.items.length - ids.length ||
    offer.items.some(
      (item, index) =>
        !item ||
        item.id !== ids[index] ||
        item.path !== plan.items.find((source) => source.id === item.id)?.path ||
        !Number.isSafeInteger(item.sizeBytes) ||
        item.sizeBytes < 0,
    )
  )
    return null
  return offer
}
