import { describe, it, expect } from 'vitest'
import { parseUpdateDraft, parseAppUpdateRecord } from './application-update'
const draft = {
  id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  taskId: '853bf826-4ae1-4695-bc66-85d49e1957b0',
  index: 0,
  name: 'Example',
  path: '/Applications/Example.app',
  replacement: '/Users/demo/Downloads/Example.app',
  bundleId: 'dev.example.fixture',
  oldVersion: '1.0',
  newVersion: '2.0',
  publisher: 'adHoc',
  bytes: 12345,
  createdAt: 1000,
  expiresAt: 121000,
}
const target = {
  taskId: draft.taskId,
  index: 0,
  name: draft.name,
  path: draft.path,
  bundleId: draft.bundleId,
}
const record = {
  id: draft.id,
  requestId: '7205853c-c7fd-47a9-8e83-fcebe84336dd',
  createdAt: 1000,
  finishedAt: 1500,
  draft,
  state: 'updated',
  phase: 'finished',
  reason: 'owned fixture',
  backup: `/Applications/.moomole-app-update-${draft.id}/Example.app`,
  reviewedAt: null,
}
describe('manual app update boundary', () => {
  it('binds the retained row and replacement without treating a parsed draft as permission', () => {
    expect(parseUpdateDraft(draft, target, draft.replacement)).toEqual(draft)
    for (const field of ['taskId', 'index', 'name', 'path', 'bundleId'] as const) {
      const changed = { ...target, [field]: field === 'index' ? 1 : 'different' }
      expect(() => parseUpdateDraft(draft, changed)).toThrow()
    }
    expect(() => parseUpdateDraft(draft, target, '/Users/demo/Other.app')).toThrow()
  })
  it('rejects invalid confirmation lifetimes, ranges and paths', () => {
    for (const patch of [
      { expiresAt: 121001 },
      { createdAt: -1 },
      { bytes: 8 * 1024 ** 3 + 1 },
      { index: 0.1 },
      { id: 'other' },
      { path: '/Applications/../Example.app' },
      { replacement: draft.path },
      { replacement: '/tmp/example.zip' },
    ])
      expect(() => parseUpdateDraft({ ...draft, ...patch })).toThrow()
  })
  it('accepts a bound terminal receipt and rejects invented backup or success evidence', () => {
    expect(parseAppUpdateRecord(record)).toEqual(record)
    for (const patch of [
      { id: draft.taskId },
      { backup: '/Applications/Other.app' },
      { backup: record.backup.replace(draft.id, draft.taskId) },
      { phase: 'checking' },
      { finishedAt: null },
      { finishedAt: 999 },
      { reviewedAt: 1 },
      { state: 'success' },
    ])
      expect(() => parseAppUpdateRecord({ ...record, ...patch })).toThrow()
  })
})
