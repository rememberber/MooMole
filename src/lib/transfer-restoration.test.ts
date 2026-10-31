import { describe, it, expect } from 'vitest'
import {
  readTransferRestoreDraft,
  readTransferRestoreRecord,
  readTransferRestoreView,
} from './transfer-restoration'
const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
const draft = {
  id: uuid(1),
  runId: uuid(2),
  itemId: uuid(3),
  copyPath: '/Volumes/Owned/copy.txt',
  originalPath: '/Users/owned/Downloads/original.txt',
  bytes: 123,
  createdAt: 1000,
  expiresAt: 121000,
}
const record = {
  id: draft.id,
  requestId: uuid(4),
  draft,
  createdAt: 1000,
  finishedAt: 2000,
  state: 'restored',
  phase: 'finished',
  temporaryPath: `/Users/owned/Downloads/.moomole-restore-${draft.id}.tmp`,
  receipt: { destination: draft.originalPath, device: '1', inode: '99' },
  reason: 'owned fixture',
  reviewedAt: null,
}
describe('cross-volume copy recovery binding', () => {
  it('binds the original record, item and both paths before exposing confirmation', () => {
    expect(readTransferRestoreDraft(draft, draft)).toEqual(draft)
    for (const patch of [
      { runId: uuid(99) },
      { itemId: uuid(99) },
      { copyPath: '/Volumes/Owned/other.txt' },
      { originalPath: '/Users/owned/Downloads/other.txt' },
    ])
      expect(() => readTransferRestoreDraft(draft, { ...draft, ...patch })).toThrow()
    for (const patch of [
      { expiresAt: 121001 },
      { bytes: 16 * 1024 ** 3 + 1 },
      { id: 'wrong' },
      { originalPath: draft.copyPath },
      { copyPath: '/Volumes/Owned/../copy.txt' },
    ])
      expect(() => readTransferRestoreDraft({ ...draft, ...patch })).toThrow()
  })
  it('requires actual original-path receipt for success and preserves unknown outcomes', () => {
    expect(readTransferRestoreRecord(record)).toEqual(record)
    for (const patch of [
      { receipt: null },
      { phase: 'intent' },
      { finishedAt: null },
      { finishedAt: 999 },
      { temporaryPath: '/tmp/other' },
      { receipt: { ...record.receipt, inode: '0' } },
      { receipt: { ...record.receipt, destination: draft.copyPath } },
    ])
      expect(() => readTransferRestoreRecord({ ...record, ...patch })).toThrow()
    expect(
      readTransferRestoreRecord({ ...record, state: 'unknown', phase: 'intent', receipt: null })
        .state,
    ).toBe('unknown')
  })
  it('never treats another request or a terminal record as a live restoration', () => {
    const running = {
      ...record,
      state: 'running',
      phase: 'intent',
      finishedAt: null,
      receipt: null,
    }
    expect(
      readTransferRestoreView({ records: [running], activeRequests: [record.requestId] })
        .activeRequests,
    ).toEqual([record.requestId])
    for (const raw of [
      { records: [record], activeRequests: [record.requestId] },
      { records: [running], activeRequests: [uuid(99)] },
      { records: [running, running], activeRequests: [] },
    ])
      expect(() => readTransferRestoreView(raw)).toThrow()
  })
})
