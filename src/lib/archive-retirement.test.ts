import { it, expect } from 'vitest'
import { retirementDraft, retirementRecord, retirementSnapshot } from './archive-retirement'
const header = {
  format: 'MooMole execution evidence',
  version: 3,
  id: 'a1000000-0000-4000-8000-000000000001',
  createdAt: 1000,
  counts: { cleanup: 1, uninstall: 0, maintenance: 0 },
  payloadSha256: 'a'.repeat(64),
  payloadBytes: 1000,
}
const draft = {
  id: 'b1000000-0000-4000-8000-000000000001',
  header,
  path: `/Users/demo/Library/Application Support/MooMole/execution-archives/${header.id}.moomole-history`,
  exported: '/Volumes/Owned/backup.moomole-history',
  bytes: 1300,
  sha256: 'b'.repeat(64),
  createdAt: 2000,
  expiresAt: 122000,
}
const record = {
  id: draft.id,
  requestId: 'c1000000-0000-4000-8000-000000000001',
  draft,
  state: 'moved',
  phase: 'finished',
  reason: 'owned result',
  createdAt: 2001,
  finishedAt: 2010,
  receipt: {
    destination: `/Users/demo/.Trash/${header.id}.moomole-history`,
    device: '16777240',
    inode: '18446744073709551614',
  },
  reviewedAt: null,
}
it('binds the exact archive, independent export, complete-file hash, lifetime and native receipt without rounding', () => {
  const parsed = retirementDraft(draft)
  expect(retirementRecord(record, parsed, record.requestId).receipt?.inode).toBe(
    record.receipt.inode,
  )
  for (const patch of [
    { exported: draft.path },
    { bytes: 1000 },
    { bytes: 3049 },
    { expiresAt: 122001 },
    { sha256: 'x'.repeat(64) },
    { path: '/elsewhere/not-the-id.moomole-history' },
    { exported: '/Volumes/Owned/a/../backup.moomole-history' },
  ])
    expect(() => retirementDraft({ ...draft, ...patch })).toThrow()
  expect(() => retirementRecord(record, parsed, draft.id)).toThrow()
  expect(() =>
    retirementRecord(
      { ...record, draft: { ...draft, exported: '/Volumes/Owned/other.moomole-history' } },
      parsed,
      record.requestId,
    ),
  ).toThrow()
})
it('rejects invented success and active-request mismatches while retaining unknown states for manual review', () => {
  for (const patch of [
    { receipt: null },
    { phase: 'intent' },
    { finishedAt: null },
    { finishedAt: 1000 },
    { receipt: { ...record.receipt, inode: 1234 } },
  ])
    expect(() => retirementRecord({ ...record, ...patch })).toThrow()
  const unknown = { ...record, state: 'unknown', receipt: null }
  expect(retirementSnapshot({ records: [unknown], activeRequests: [] }).records[0].state).toBe(
    'unknown',
  )
  const running = { ...record, state: 'running', phase: 'intent', finishedAt: null, receipt: null }
  expect(
    retirementSnapshot({ records: [running], activeRequests: [record.requestId] }).records,
  ).toHaveLength(1)
  for (const raw of [
    { records: [running], activeRequests: [] },
    { records: [unknown], activeRequests: [record.requestId] },
    { records: [unknown, unknown], activeRequests: [] },
  ])
    expect(() => retirementSnapshot(raw)).toThrow()
})

it('binds all six v4 journal counts when retiring a verified local archive copy', () => {
  const h = {
      ...header,
      version: 4 as const,
      counts: { ...header.counts, moleClean: 1, extraction: 1, managedApp: 1 },
    },
    expected = { ...h },
    raw = { ...draft, header: h }
  const parsed = retirementDraft(raw, expected)
  expect(parsed.header.counts).toEqual(h.counts)
  for (const key of ['moleClean', 'extraction', 'managedApp'] as const) {
    const changed = { ...raw, header: { ...h, counts: { ...h.counts, [key]: 2 } } }
    expect(() => retirementDraft(changed, expected)).toThrow()
    expect(() =>
      retirementRecord({ ...record, draft: changed }, parsed, record.requestId),
    ).toThrow()
  }
})
