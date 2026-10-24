import { describe, it, expect } from 'vitest'
import { draftRaw, refreshDetailRaw, refreshRequest } from '../../tests/fixtures/quicklook'
import {
  readRefreshDraft,
  readRefreshDetail,
  readRefreshSnapshot,
  readRefreshList,
  preserveRefresh,
  refreshSteps,
} from './quicklook-refresh'
describe('fixed Quick Look protocol', () => {
  it('binds the original exact selection, scope, acknowledgement and time', () => {
    const raw = draftRaw()
    expect(readRefreshDraft(raw, [...refreshSteps], Date.now()).selected).toEqual(refreshSteps)
    for (const patch of [
      { scope: 'wholeFinder' },
      { selected: ['generators', 'thumbnailCache'] },
      { requiresRebuildAcknowledgement: false },
      { expiresAt: raw.createdAt + 30_001 },
      { revision: 'b'.repeat(40) },
    ])
      expect(() => readRefreshDraft({ ...raw, ...patch }, [...refreshSteps], Date.now())).toThrow()
  })
  it('keeps completed facts and late cancellation or list responses cannot erase them', () => {
    const done = readRefreshDetail(refreshDetailRaw()),
      old = readRefreshDetail(refreshDetailRaw(refreshRequest, 'running', done.createdAt))
    const before = { version: 1 as const, requestId: refreshRequest, active: false, record: done }
    expect(preserveRefresh(before, { ...before, active: true, record: old }).record).toEqual(done)
    expect(preserveRefresh(before, { ...before, active: true, record: old }).active).toBe(false)
    expect(() =>
      preserveRefresh(before, {
        ...before,
        record: { ...done, planId: '9d54bcdd-a9a4-4444-a1ab-282a11e33641' },
      }),
    ).toThrow()
    expect(preserveRefresh(before, { ...before, record: null }).record).toEqual(done)
    expect(readRefreshList([done])[0].succeededCount).toBe(2)
    expect(() => readRefreshList([done, done])).toThrow()
  })
  it('rejects missing intents, inconsistent counts and state/result contradictions', () => {
    const raw = refreshDetailRaw()
    for (const changed of [
      { ...raw, succeededCount: 1 },
      { ...raw, events: raw.events.filter((e) => e.state !== 'intent') },
      { ...raw, items: raw.items.map((i, p) => (p ? i : { ...i, exitCode: 7 })) },
      { ...raw, reviewedAt: Date.now() },
    ])
      expect(() => readRefreshDetail(changed)).toThrow()
  })
  it('a reviewed unknown result stays unknown and cannot regress on a stale observation', () => {
    const raw = refreshDetailRaw(refreshRequest, 'interrupted')
    const before = readRefreshDetail({ ...raw, reviewedAt: Date.now(), retainedForReview: false })
    expect(before.items[0].state).toBe('uncertain')
    const snapshot = {
      version: 1 as const,
      requestId: refreshRequest,
      active: false,
      record: before,
    }
    expect(
      preserveRefresh(snapshot, { ...snapshot, record: readRefreshDetail(raw) }).record?.reviewedAt,
    ).toBe(before.reviewedAt)
    expect(() => readRefreshSnapshot({ ...snapshot, requestId: 'other' }, refreshRequest)).toThrow()
  })
  it('wall-clock rollback does not discard committed results or order them by timestamps', () => {
    const raw = refreshDetailRaw()
    raw.finishedAt = raw.createdAt - 1000
    expect(readRefreshDetail(raw).succeededCount).toBe(2)
    const reversed = {
      ...raw,
      events: [
        raw.events[0],
        raw.events[1],
        raw.events[2],
        raw.events[4],
        raw.events[3],
        raw.events[5],
      ],
    }
    expect(() => readRefreshDetail(reversed)).toThrow()
  })
})
