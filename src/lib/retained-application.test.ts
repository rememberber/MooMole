import { expect, it } from 'vitest'
import { retainedAppDraft, retainedAppRecord, retainedAppView } from './retained-application'
import type { AppUpdateRecord } from './application-update'
const sourceId = '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
  requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd'
const stage = `/Applications/.moomole-app-update-${sourceId}`
const draft = {
  id,
  sourceId,
  name: 'Owned',
  installed: '/Applications/Owned.app',
  retained: `${stage}/Owned.app`,
  stage,
  installedVersion: '2.0',
  retainedVersion: '1.0',
  afterRestore: false,
  publisher: 'adHoc',
  bytes: 12345,
  createdAt: 1000,
  expiresAt: 121000,
}
const source = {
  id: sourceId,
  state: 'updated',
  retainedCopyAvailable: true,
  draft: {
    name: 'Owned',
    path: draft.installed,
    oldVersion: '1.0',
    newVersion: '2.0',
    publisher: 'adHoc',
  },
  backup: draft.retained,
} as AppUpdateRecord
const record = {
  id,
  requestId,
  draft,
  state: 'moved',
  phase: 'finished',
  reason: 'owned',
  createdAt: 1001,
  finishedAt: 1002,
  receipt: { destination: '/Users/demo/.Trash/Owned.app', device: '1', inode: '9007199254740993' },
  stageRemoved: true,
  reviewedAt: null,
}
it('binds installed/retained versions in both reliable orientations to the original update', () => {
  expect(retainedAppDraft(draft, source)).toEqual(draft)
  const restored = { ...draft, afterRestore: true, installedVersion: '1.0', retainedVersion: '2.0' }
  expect(retainedAppDraft(restored, source)).toEqual(restored)
  for (const patch of [
    { sourceId: id },
    { installed: '/Applications/Other.app' },
    { retained: '/Applications/Owned.app' },
    { stage: '/Applications' },
    { afterRestore: true },
    { publisher: 'other' },
    { bytes: 8 * 1024 ** 3 + 1 },
    { expiresAt: 121001 },
    { installed: '/Applications/../Owned.app' },
  ])
    expect(() => retainedAppDraft({ ...draft, ...patch }, source)).toThrow()
  expect(() => retainedAppDraft(draft, { ...source, retainedCopyAvailable: false })).toThrow()
  expect(() => retainedAppDraft(draft, { ...source, retainedCopyAvailable: undefined })).toThrow()
})
it('accepts only actual moved receipts and valid state, phase and exact integer identifiers', () => {
  expect(retainedAppRecord(record)).toEqual(record)
  for (const patch of [
    { receipt: null },
    { phase: 'intent' },
    { finishedAt: null },
    { id: sourceId },
    { state: 'cancelled' },
    { receipt: { ...record.receipt, inode: 9007199254740993 } },
    { receipt: { ...record.receipt, inode: '0' } },
    { receipt: { ...record.receipt, device: '18446744073709551616' } },
  ])
    expect(() => retainedAppRecord({ ...record, ...patch })).toThrow()
  expect(retainedAppRecord({ ...record, state: 'unknown', stageRemoved: false }).state).toBe(
    'unknown',
  )
})
it('reattaches exact active request ownership without restoring permission after manual review', () => {
  const running = {
    ...record,
    state: 'running',
    phase: 'intent',
    finishedAt: null,
    stageRemoved: false,
    receipt: null,
  }
  expect(
    retainedAppView({ records: [running], activeRequests: [requestId] }).activeRequests,
  ).toEqual([requestId])
  for (const value of [
    { records: [record], activeRequests: [requestId] },
    { records: [running], activeRequests: [id] },
    { records: [running, running], activeRequests: [] },
    { records: [running], activeRequests: [requestId, requestId] },
  ])
    expect(() => retainedAppView(value)).toThrow()
  const unknown = { ...record, state: 'unknown', stageRemoved: false, reviewedAt: 1003 }
  expect(retainedAppView({ records: [unknown], activeRequests: [] }).records[0].state).toBe(
    'unknown',
  )
})
