import { describe, expect, it } from 'vitest'
import { maintenanceReport } from '../../tests/e2e/fixtures/maintenance'
import { maintenanceReview, reviewSelection } from '../../tests/e2e/fixtures/maintenance-review'
import { maintenanceReviewOutcome, readMaintenanceReview } from './maintenance-review'

describe('native maintenance review binding', () => {
  it('retains exact selected keys without extending observation lifetime', () => {
    const report = maintenanceReport()
    const review = maintenanceReview(report, reviewSelection)
    expect(
      readMaintenanceReview(review, report, reviewSelection, report.checkedAt, report.checkedAt),
    ).toEqual(review)
    expect(review.items[0].keys.every((key) => key.outcome === 'unchanged')).toBe(true)
    expect(review.items[1].keys.every((key) => key.outcome === 'preserveProtected')).toBe(true)
    expect(
      readMaintenanceReview(
        review,
        report,
        reviewSelection,
        report.checkedAt,
        review.expiresAt - 1,
      ),
    ).toEqual(review)
    for (const now of [report.checkedAt - 1, review.expiresAt, review.expiresAt + 1]) {
      expect(() =>
        readMaintenanceReview(review, report, reviewSelection, report.checkedAt, now),
      ).toThrow()
    }
  })
  it('rejects changed authority, execution claims, expanded scope and contradictory results', () => {
    const report = maintenanceReport()
    const original = maintenanceReview(report, reviewSelection)
    const mutations = [
      (r: Record<string, any>) => {
        r.protocol = 2
      },
      (r: Record<string, any>) => {
        r.observationId = crypto.randomUUID()
      },
      (r: Record<string, any>) => {
        r.revision = 'other'
      },
      (r: Record<string, any>) => {
        r.canExecute = true
      },
      (r: Record<string, any>) => {
        delete r.originalsCaptured
      },
      (r: Record<string, any>) => {
        r.concurrencyResolved = true
      },
      (r: Record<string, any>) => {
        r.mode = 'execute'
      },
      (r: Record<string, any>) => {
        r.checkedAt++
      },
      (r: Record<string, any>) => {
        r.expiresAt++
      },
      (r: Record<string, any>) => {
        r.preparedAt += 6000
      },
      (r: Record<string, any>) => {
        r.items.reverse()
      },
      (r: Record<string, any>) => {
        r.items[0].keys.pop()
      },
      (r: Record<string, any>) => {
        r.items[0].keys[0].current = 'disabled'
      },
      (r: Record<string, any>) => {
        r.items[0].keys[0].outcome = 'needsEvidence'
      },
      (r: Record<string, any>) => {
        r.items[0].keys[0].intent = 'removeOverride'
      },
    ]
    for (const mutate of mutations) {
      const review = structuredClone(original)
      mutate(review)
      expect(() =>
        readMaintenanceReview(review, report, reviewSelection, report.checkedAt, report.checkedAt),
      ).toThrow()
    }
    for (const selected of [[], [reviewSelection[0], reviewSelection[0]], [reviewSelection[0]]]) {
      expect(() =>
        readMaintenanceReview(original, report, selected, report.checkedAt, report.checkedAt),
      ).toThrow()
    }
  })
  it('keeps protection, limited access, unknown conditions and missing evidence distinct', () => {
    const report = maintenanceReport()
    const item = report.items[0]
    const key = item.findings[0]
    key.value = 'disabled'
    expect(maintenanceReviewOutcome(item, key)).toBe('needsEvidence')
    item.paths[0].access = 'unknown'
    expect(maintenanceReviewOutcome(item, key)).toBe('preserveUnknown')
    item.code = 'restricted'
    expect(maintenanceReviewOutcome(item, key)).toBe('preserveRestricted')
    item.code = 'protected'
    expect(maintenanceReviewOutcome(item, key)).toBe('preserveProtected')
  })
})
