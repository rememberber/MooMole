import { describe, it, expect } from 'vitest'
import {
  parseUninstallPlan,
  parseUninstallReview,
  parseUninstallDetail,
  parseUninstallSnapshot,
  parseUninstallList,
  preserveUninstall,
} from './app-uninstall'
import { target, planRaw, reviewRaw, detailRaw, requestId } from '../../tests/fixtures/uninstall'

describe('whole-App uninstall native boundary', () => {
  it('binds the original Mole row and keeps the plan read-only', () => {
    const raw = planRaw()
    expect(parseUninstallPlan(raw, target).path).toBe(target.path)
    for (const change of [
      { sourceIndex: 4 },
      { path: '/Applications/other.app' },
      { canExecute: true },
      { scope: 'all' },
      { relatedData: 'remove' },
      { revision: 'future' },
      { expiresAt: raw.createdAt + 300001 },
      { readOnly: false },
    ])
      expect(() => parseUninstallPlan({ ...raw, ...change }, target)).toThrow()
  })
  it('accepts only a v2 fixed-scope confirmation with both original observations', () => {
    const raw = planRaw(),
      plan = parseUninstallPlan(raw, target),
      review = reviewRaw(raw)
    expect(parseUninstallReview(review, plan).confirmation?.path).toBe(target.path)
    for (const change of [
      { version: 1 },
      { planId: requestId },
      { planSnapshotSha256: 'e'.repeat(64) },
      { canExecute: false },
      { requiresOrdinaryAppConfirmation: false },
      { method: 'permanent' },
      { path: '/Applications/other.app' },
      { expiresAt: review.confirmation.createdAt + 30001 },
    ])
      expect(() =>
        parseUninstallReview(
          { ...review, confirmation: { ...review.confirmation, ...change } },
          plan,
        ),
      ).toThrow()
    expect(() =>
      parseUninstallReview(
        {
          ...review,
          runtime: { ...review.runtime, code: 'unknown', canPrepareConfirmation: false },
        },
        plan,
      ),
    ).toThrow()
    expect(() =>
      parseUninstallReview(
        {
          ...review,
          support: {
            ...review.support,
            management: { ...review.support.management, code: 'detected' },
          },
        },
        plan,
      ),
    ).toThrow()
  })
  it('unknown and busy results retain the App without manufacturing a confirmation', () => {
    const plan = parseUninstallPlan(planRaw(), target),
      review = reviewRaw()
    expect(
      parseUninstallReview(
        {
          ...review,
          runtime: {
            ...review.runtime,
            code: 'observed',
            pids: [123],
            canPrepareConfirmation: false,
          },
          confirmation: null,
        },
        plan,
      ).message,
    ).toContain('自行退出')
    expect(
      parseUninstallReview(
        {
          ...review,
          support: {
            ...review.support,
            management: { ...review.support.management, code: 'detected' },
          },
          confirmation: null,
        },
        plan,
      ).confirmation,
    ).toBeNull()
  })
  it('preserves exact receipt integers and valid cancellation/interruption event chains', () => {
    for (const state of [
      'pending',
      'intent',
      'moved',
      'skipped',
      'cancelled',
      'failed',
      'notStarted',
      'uncertain',
    ])
      expect(parseUninstallDetail(detailRaw(requestId, state), requestId).itemState).toBe(state)
    expect(parseUninstallDetail(detailRaw(), requestId).receipt?.actual.inode).toBe(
      '9007199254740993',
    )
    const cancelled = detailRaw(requestId, 'cancelled')
    cancelled.events.splice(1, 0, {
      state: 'intent',
      createdAt: cancelled.createdAt + 1,
      reason: '意图',
      receipt: null,
    })
    expect(parseUninstallDetail(cancelled, requestId).itemState).toBe('cancelled')
  })
  it('rejects missing receipts, changed IDs, malformed states and inconsistent facts', () => {
    const raw = detailRaw()
    for (const change of [
      { id: 'bad' },
      { state: 'running' },
      { receipt: null },
      { method: 'trash' },
      { snapshotSha256: 'bad' },
      { events: raw.events.slice(1) },
      { events: [...raw.events, raw.events[2]] },
    ])
      expect(() => parseUninstallDetail({ ...raw, ...change }, requestId)).toThrow()
    expect(() =>
      parseUninstallDetail(
        {
          ...raw,
          receipt: { ...raw.receipt, actual: { ...raw.receipt!.actual, inode: 9007199254740993 } },
        },
        requestId,
      ),
    ).toThrow()
    expect(() =>
      parseUninstallSnapshot(
        { version: 1, requestId: 'other', active: false, record: raw },
        requestId,
      ),
    ).toThrow()
    expect(() => parseUninstallList([raw, raw])).toThrow()
  })
  it('does not regress completed evidence to an older running read or erase it on absence', () => {
    const done = parseUninstallSnapshot(
      { version: 1, requestId, active: false, record: detailRaw() },
      requestId,
    )
    const old = parseUninstallSnapshot(
      { version: 1, requestId, active: true, record: detailRaw(requestId, 'intent') },
      requestId,
    )
    expect(preserveUninstall(old, done).record?.itemState).toBe('moved')
    expect(preserveUninstall(old, done).active).toBe(false)
    expect(preserveUninstall({ ...old, active: false, record: null }, done).record?.itemState).toBe(
      'moved',
    )
    expect(parseUninstallSnapshot({ ...old, active: false }, requestId).active).toBe(false)
  })
  it('review timestamps apply only to unknown outcomes and stale reads cannot erase them', () => {
    const raw = detailRaw(requestId, 'uncertain')
    const record = parseUninstallDetail({ ...raw, reviewedAt: raw.createdAt + 20 }, requestId)
    expect(record.itemState).toBe('uncertain')
    const known = { version: 1 as const, requestId, active: false, record }
    expect(
      preserveUninstall({ ...known, record: parseUninstallDetail(raw, requestId) }, known).record,
    ).toEqual(record)
    for (const invalid of [
      { ...detailRaw(), reviewedAt: 1 },
      { ...raw, reviewedAt: -1 },
      { ...raw, reviewedAt: undefined },
    ])
      expect(() => parseUninstallDetail(invalid, requestId)).toThrow()
  })
})
