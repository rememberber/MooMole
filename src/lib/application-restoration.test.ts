import { it, expect } from 'vitest'
import { appRestoreDraft, appRestoreRecord, appRestoreView } from './application-restoration'
const sourceId = '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
  requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd'
const draft = {
  id,
  sourceId,
  name: 'Owned',
  path: '/Applications/Owned.app',
  retained: `/Applications/.moomole-app-update-${sourceId}/Owned.app`,
  fromVersion: '2.0',
  toVersion: '1.0',
  publisher: 'adHoc',
  bytes: 12345,
  createdAt: 1000,
  expiresAt: 121000,
}
const record = {
  id,
  requestId,
  draft,
  createdAt: 1001,
  finishedAt: 1002,
  state: 'restored',
  phase: 'finished',
  reason: 'owned',
  receipt: {
    installedDevice: '1',
    installedInode: '9007199254740993',
    retainedDevice: '1',
    retainedInode: '4',
    installedHash: 'a'.repeat(64),
    retainedHash: 'b'.repeat(64),
  },
  reviewedAt: null,
}
it('old-version recovery binds the source, exact two paths and a fresh bounded draft', () => {
  expect(appRestoreDraft(draft)).toEqual(draft)
  for (const patch of [
    { sourceId: id },
    { expiresAt: 121001 },
    { bytes: 8 * 1024 ** 3 + 1 },
    { path: '/Applications/../Owned.app' },
    { retained: '/Applications/Other.app' },
  ])
    expect(() => appRestoreDraft({ ...draft, ...patch })).toThrow()
})
it('restoration success requires actual two-sided receipts and keeps inode precision', () => {
  expect(appRestoreRecord(record)).toEqual(record)
  for (const patch of [
    { receipt: null },
    { phase: 'intent' },
    { finishedAt: null },
    { id: sourceId },
    { receipt: { ...record.receipt, installedInode: 9007199254740993 } },
    { receipt: { ...record.receipt, retainedHash: 'unknown' } },
  ])
    expect(() => appRestoreRecord({ ...record, ...patch })).toThrow()
})
it('reattaches only exact running request owners while preserving unknown results', () => {
  const running = { ...record, state: 'running', phase: 'intent', finishedAt: null, receipt: null }
  expect(
    appRestoreView({ records: [running], activeRequests: [requestId] }).activeRequests,
  ).toEqual([requestId])
  expect(() => appRestoreView({ records: [record], activeRequests: [requestId] })).toThrow()
  expect(() => appRestoreView({ records: [running], activeRequests: [id] })).toThrow()
  const unknown = { ...running, state: 'unknown', finishedAt: 1003 }
  expect(appRestoreView({ records: [unknown], activeRequests: [] }).records[0].state).toBe(
    'unknown',
  )
})
