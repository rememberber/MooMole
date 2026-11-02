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

it('upgrade binds old/new numeric versions, URL/hash/publisher and requires distinct completion proof', () => {
  const upgrade = {
      version: '2.0',
      url: 'https://example.org/owned.zip',
      sha256: 'b'.repeat(64),
      publisher: 'adHoc',
    },
    d = { ...draft(), upgrade }
  expect(readManagedDraft(d, target, true).upgrade?.version).toBe('2.0')
  expect(() => readManagedDraft(d, target, false)).toThrow()
  expect(() => readManagedDraft(draft(), target, true)).toThrow()
  for (const patch of [
    { version: 'latest' },
    { version: '0.1' },
    { sha256: 'no_check' },
    { url: 'javascript:alert(1)' },
    { url: 'https://user:pass@example.org/file' },
    { publisher: '' },
  ])
    expect(() => readManagedDraft({ ...d, upgrade: { ...upgrade, ...patch } })).toThrow()
  const r = {
    id,
    requestId: id,
    draft: d,
    state: 'upgraded',
    phase: 'finished',
    createdAt: now,
    finishedAt: now + 1,
    reason: 'verified manager outcome',
    reviewedAt: null,
    upgradeResult: {
      version: '2.0',
      publisher: 'adHoc',
      bundleSha256: 'c'.repeat(64),
      receiptSha256: 'd'.repeat(64),
    },
  }
  expect(readManagedRecord(r).state).toBe('upgraded')
  for (const patch of [
    { state: 'removed' },
    { upgradeResult: undefined },
    { draft: draft() },
    { upgradeResult: { ...r.upgradeResult, version: '3.0' } },
    { upgradeResult: { ...r.upgradeResult, bundleSha256: 'bad' } },
  ])
    expect(() => readManagedRecord({ ...r, ...patch })).toThrow()
})
