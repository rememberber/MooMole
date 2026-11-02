import { describe, expect, it } from 'vitest'
import {
  readAppUpdateDiscovery,
  readAppUpdateProbe,
  readAppUpdateDelivery,
} from './app-update-discovery'
const target = {
  taskId: '853bf826-4ae1-4695-bc66-85d49e1957b0',
  index: 0,
  name: 'Example',
  path: '/Applications/Example.app',
  bundleId: 'dev.example.fixture',
}
const probe = {
  ...target,
  id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  currentVersion: '1.0',
  currentBuild: '10',
  feedUrl: 'https://updates.example.org/feed.xml',
  state: 'ready' as const,
  reason: 'App declaration only',
  createdAt: 1000,
  expiresAt: 301000,
}
const requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd'
const release = {
  id: '821324f0-d589-4873-b42a-8061782d839e',
  version: '2.0',
  build: '11',
  url: 'https://updates.example.org/release.zip',
  kind: 'package' as const,
  bytes: 1234,
  requiresReview: false,
  downloadable: false,
}
const report = {
  probeId: probe.id,
  requestId,
  checkedAt: 1500,
  expiresAt: probe.expiresAt,
  state: 'newerReported',
  complete: true,
  inspected: 2,
  skipped: 1,
  releases: [release],
}
it('accepts only an exact signed delivery and rejects path, candidate, size and verification mismatches', () => {
  const p = { ...probe, verifiedDownload: true },
    r = { ...release, downloadable: true },
    directory = '/Users/demo/Downloads'
  const d = {
    probeId: probe.id,
    candidateId: release.id,
    requestId,
    path: directory + '/MooMole-update-46573b46-9956-492e-b38c-101114edbeec.zip',
    bytes: release.bytes,
    sha256: 'a'.repeat(64),
    verification: 'installedKeyEd25519',
  }
  expect(readAppUpdateDelivery(d, p, r, requestId, directory)).toEqual(d)
  for (const patch of [
    { candidateId: probe.id },
    { requestId: release.id },
    { bytes: 12 },
    { verification: 'https' },
    { sha256: 'bad' },
    { path: directory + '/../Other.zip' },
    { path: directory + '/sub/MooMole-update-46573b46-9956-492e-b38c-101114edbeec.zip' },
  ])
    expect(() => readAppUpdateDelivery({ ...d, ...patch }, p, r, requestId, directory)).toThrow()
  expect(() =>
    readAppUpdateDelivery(d, { ...p, verifiedDownload: false }, r, requestId, directory),
  ).toThrow()
})
describe('application update discovery boundary', () => {
  it('binds local source to its retained row without a network or install action', () => {
    expect(readAppUpdateProbe(probe, target).feedUrl).toBe(probe.feedUrl)
    for (const field of ['taskId', 'index', 'path', 'bundleId'] as const)
      expect(() =>
        readAppUpdateProbe(probe, { ...target, [field]: field === 'index' ? 1 : 'other' }),
      ).toThrow()
    for (const patch of [
      { expiresAt: 301001 },
      { currentBuild: '11-rc' },
      { feedUrl: 'http://updates.example.org/feed' },
    ])
      expect(() => readAppUpdateProbe({ ...probe, ...patch }, target)).toThrow()
    expect(
      readAppUpdateProbe(
        { ...probe, state: 'unsupported', currentBuild: 'rc', feedUrl: null },
        target,
      ).state,
    ).toBe('unsupported')
  })
  it('requires the exact probe, request, lifetime, count and state relationships', () => {
    expect(readAppUpdateDiscovery(report, probe, requestId).releases).toEqual([release])
    for (const patch of [
      { probeId: requestId },
      { requestId: probe.id },
      { expiresAt: 301001 },
      { checkedAt: 301000 },
      { checkedAt: 999 },
      { inspected: 129 },
      { skipped: 2 },
      { complete: 'true' },
      { state: 'noNewerInFeed' },
      { releases: [release, release] },
    ])
      expect(() => readAppUpdateDiscovery({ ...report, ...patch }, probe, requestId)).toThrow()
  })
  it('keeps incomplete empty feeds unknown, and limits a complete empty result to this source', () => {
    const empty = {
      ...report,
      state: 'unknown',
      complete: false,
      inspected: 0,
      skipped: 0,
      releases: [],
    }
    expect(readAppUpdateDiscovery(empty, probe, requestId).state).toBe('unknown')
    expect(() =>
      readAppUpdateDiscovery(
        { ...empty, complete: true, state: 'noNewerInFeed' },
        probe,
        requestId,
      ),
    ).toThrow()
    expect(
      readAppUpdateDiscovery(
        { ...empty, inspected: 2, skipped: 2, complete: true, state: 'noNewerInFeed' },
        probe,
        requestId,
      ).state,
    ).toBe('noNewerInFeed')
    expect(() =>
      readAppUpdateDiscovery(empty, { ...probe, state: 'unsupported' }, requestId),
    ).toThrow()
  })
  it('rejects local or credentialed URLs and unsafe labels and compares machine builds without JS rounding', () => {
    for (const patch of [
      ...[
        'http://example.org/a',
        'https://127.0.0.1/a',
        'https://[::1]/a',
        'https://host.local/a',
        'https://user:pass@example.org/a',
        'https://example.org:444/a',
        'https://example.org/a#x',
      ].map((url) => ({ url })),
      { version: '\u202eevil' },
      { build: '10.0' },
      { build: '11-rc' },
      { build: '18446744073709551616' },
      { bytes: Number.MAX_SAFE_INTEGER },
      { bytes: 0 },
      { requiresReview: 'false' },
      { kind: 'installer' },
    ])
      expect(() =>
        readAppUpdateDiscovery(
          { ...report, releases: [{ ...release, ...patch }] },
          probe,
          requestId,
        ),
      ).toThrow()
    expect(
      readAppUpdateDiscovery(
        { ...report, releases: [{ ...release, build: '18446744073709551615' }] },
        { ...probe, currentBuild: '18446744073709551614' },
        requestId,
      ).releases,
    ).toHaveLength(1)
  })
})
