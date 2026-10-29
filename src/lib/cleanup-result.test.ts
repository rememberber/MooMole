import { describe, expect, it } from 'vitest'
import type { ExecutionDetail } from '../types'
import { cleanupResultSizes, cleanupSpaceChange } from './cleanup-result'

function detail(): ExecutionDetail {
  return {
    id: 'run',
    planId: 'plan',
    method: 'trash',
    state: 'cancelled',
    createdAt: 100,
    finishedAt: 200,
    itemCount: 4,
    movedCount: 2,
    uncertainCount: 1,
    reviewedAt: null,
    retainedForReview: true,
    revision: 'fixture',
    protocol: 2,
    snapshotSha256: 'a'.repeat(64),
    events: [],
    items: [
      {
        id: '1',
        path: '/fixture/1',
        state: 'moved',
        reason: 'fixture',
        receipt: { destination: '/trash/1', device: '1', inode: '2' },
        scanSizeBytes: 100,
      },
      {
        id: '2',
        path: '/fixture/2',
        state: 'moved',
        reason: 'fixture',
        receipt: { destination: '/trash/2', device: '1', inode: '3' },
        scanSizeBytes: null,
      },
      {
        id: '3',
        path: '/fixture/3',
        state: 'uncertain',
        reason: 'fixture',
        receipt: null,
        scanSizeBytes: 200,
      },
      {
        id: '4',
        path: '/fixture/4',
        state: 'cancelled',
        reason: 'fixture',
        receipt: null,
        scanSizeBytes: 300,
      },
    ],
    spaceObservation: {
      version: 1,
      before: { volumeId: '1:aabb', measuredAt: 101, totalBytes: 1000, availableBytes: 400 },
      after: { volumeId: '1:aabb', measuredAt: 201, totalBytes: 1000, availableBytes: 400 },
    },
  }
}
describe('cleanup result amounts', () => {
  it('only counts committed moves with receipts; unknown sizes are explicit', () => {
    const record = detail()
    expect(cleanupResultSizes(record)).toEqual({
      selected: { knownBytes: 600, unknownCount: 1 },
      moved: { knownBytes: 100, unknownCount: 1 },
    })
    record.items[0].receipt = null
    record.items[3].scanSizeBytes = Number.MAX_SAFE_INTEGER
    expect(cleanupResultSizes(record).moved).toEqual({ knownBytes: 0, unknownCount: 1 })
    expect(cleanupResultSizes(record).selected.unknownCount).toBe(2)
  })
  it('preserves zero, increasing and decreasing observed free space without attributing the change', () => {
    const record = detail()
    expect(cleanupSpaceChange(record)).toMatchObject({ code: 'observed', delta: 0 })
    record.spaceObservation!.after!.availableBytes = 600
    expect(cleanupSpaceChange(record)).toMatchObject({ code: 'observed', delta: 200 })
    record.spaceObservation!.after!.availableBytes = 300
    expect(cleanupSpaceChange(record)).toMatchObject({ code: 'observed', delta: -100 })
  })
  it('does not invent a delta for legacy, interrupted, invalid or different-volume observations', () => {
    const record = detail()
    record.spaceObservation!.after!.volumeId = '2:aabb'
    expect(cleanupSpaceChange(record).code).toBe('volumeChanged')
    record.spaceObservation!.after!.volumeId = '1:aabb'
    record.spaceObservation!.before!.measuredAt = 201
    record.spaceObservation!.after!.measuredAt = 202
    expect(cleanupSpaceChange(record).code).toBe('timeChanged')
    record.spaceObservation!.before!.measuredAt = 101
    record.spaceObservation!.after!.measuredAt = 99
    expect(cleanupSpaceChange(record).code).toBe('timeChanged')
    record.spaceObservation!.after!.availableBytes = -1
    expect(cleanupSpaceChange(record).code).toBe('unavailable')
    record.spaceObservation!.after = null
    expect(cleanupSpaceChange(record).code).toBe('unavailable')
    delete record.spaceObservation
    for (const item of record.items) delete item.scanSizeBytes
    expect(cleanupResultSizes(record).selected).toEqual({ knownBytes: 0, unknownCount: 4 })
    expect(cleanupSpaceChange(record).code).toBe('unavailable')
  })
})
