import { describe, it, expect } from 'vitest'
import { videoTargets, readVideoComparison, type VideoTarget } from './video-comparison'
const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
const targets: VideoTarget[] = [1, 2].map((n) => ({
  domain: 'personalFiles',
  scanId: uuid(10),
  itemId: uuid(n),
  path: `/Users/owned/${n}.mp4`,
  name: `${n}.mp4`,
  size: 1234,
}))
const payload = () => ({
  comparisonId: uuid(11),
  scanId: uuid(10),
  items: targets.map((t, index) => ({
    scanId: t.scanId,
    itemId: t.itemId,
    path: t.path,
    content: {
      kind: 'media',
      url: `moomole-media://localhost/${uuid(index + 20)}/media`,
      leaseId: uuid(index + 20),
      mediaKind: 'video',
      mime: 'video/mp4',
      expiresAt: Date.now() + 60000,
    },
  })),
})
describe('video comparison binding', () => {
  it('accepts only bounded distinct members of the same retained domain and scan', () => {
    expect(() => videoTargets(targets)).not.toThrow()
    for (const list of [
      targets.slice(0, 1),
      [...targets, ...targets],
      [targets[0], { ...targets[1], scanId: uuid(99) }],
      [targets[0], { ...targets[1], domain: 'softwareContent' as const }],
      [targets[0], { ...targets[1], path: '/Users/owned/audio.wav' }],
    ])
      expect(() => videoTargets(list)).toThrow()
  })
  it('binds session, scan, order, paths and independent media capabilities', () => {
    expect(readVideoComparison(payload(), uuid(11), targets)).toHaveLength(2)
    for (const mode of [
      'session',
      'scan',
      'order',
      'path',
      'audio',
      'duplicateLease',
      'external',
      'expired',
    ]) {
      const v = payload()
      if (mode === 'session') v.comparisonId = uuid(99)
      if (mode === 'scan') v.scanId = uuid(99)
      if (mode === 'order') v.items.reverse()
      if (mode === 'path') v.items[1].path = '/Users/owned/other.mp4'
      if (mode === 'audio') {
        v.items[1].content.mediaKind = 'audio'
        v.items[1].content.mime = 'audio/mp4'
      }
      if (mode === 'duplicateLease') {
        v.items[1].content.leaseId = v.items[0].content.leaseId
        v.items[1].content.url = v.items[0].content.url
      }
      if (mode === 'external') v.items[1].content.url = 'https://example.invalid/video'
      if (mode === 'expired') v.items[1].content.expiresAt = Date.now() - 1
      expect(() => readVideoComparison(v, uuid(11), targets), mode).toThrow()
    }
  })
})
