import { describe, it, expect } from 'vitest'
import { readManagedDraft, readManagedRecord } from './managed-apps'
const id = '12345678-1234-4123-8123-123456789abc',
  now = Date.now()
const target = {
  taskId: id,
  index: 3,
  path: '/Applications/Owned.app',
  name: 'Owned',
  bundleId: 'org.example.owned',
}
function draft() {
  return {
    id,
    ...target,
    token: 'author/tap/owned',
    installedVersion: '1.0',
    managerPath: '/opt/homebrew/bin/brew',
    metadataSha256: 'a'.repeat(64),
    createdAt: now,
    expiresAt: now + 120000,
  }
}
describe('managed application protocol', () => {
  it('binds the exact retained App source and rejects arbitrary manager paths, flags, tokens and expiry shapes', () => {
    expect(readManagedDraft(draft(), target).token).toBe('author/tap/owned')
    for (const change of [
      { taskId: 'bad' },
      { index: 2 },
      { path: '/Applications/Other.app' },
      { bundleId: 'org.other' },
      { name: 'Other' },
      { name: '' },
      { installedVersion: '1\nflag' },
      { token: '--zap' },
      { token: 'a/b/../../c' },
      { managerPath: '/tmp/brew' },
      { expiresAt: String(now + 120000) },
      { metadataSha256: 'bad' },
    ])
      expect(() => readManagedDraft({ ...draft(), ...change }, target)).toThrow()
  })
  it('keeps manager removal distinct from Trash and validates durable unknown and interrupted states', () => {
    const record = {
      id,
      requestId: id,
      draft: draft(),
      state: 'unknown',
      phase: 'intent',
      createdAt: now,
      finishedAt: now + 100,
      reason: 'partial result; review actual state',
      reviewedAt: null,
    }
    expect(readManagedRecord(record).state).toBe('unknown')
    for (const change of [
      { id: 'bad' },
      { state: 'moved' },
      { phase: 'delete' },
      { finishedAt: now - 1 },
      { reviewedAt: now - 1 },
      { state: 'running' },
      { finishedAt: null },
      { createdAt: now - 1 },
    ])
      expect(() => readManagedRecord({ ...record, ...change })).toThrow()
  })
})
