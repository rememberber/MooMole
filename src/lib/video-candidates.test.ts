import { describe, expect, it } from 'vitest'
import { readPersonalSnapshot } from './personal-files'
const id = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
const thumbnail = 'data:image/png;base64,iVBORw0KGgoAAAA='
function snapshot() {
  const now = Date.now()
  const items = [1, 2].map((n) => ({
    id: id(n),
    path: `/Users/demo/Movies/clip-${n}.mp4`,
    name: `clip-${n}.mp4`,
    size: n * 1024,
    modifiedAt: now - 1000,
  }))
  return {
    phase: 'ready',
    requestId: id(9),
    runId: null,
    error: null,
    report: {
      id: id(8),
      root: '/Users/demo/Movies',
      createdAt: now,
      expiresAt: now + 900000,
      complete: true,
      cancelled: false,
      skipped: 0,
      visited: 2,
      items,
      similarPhotos: {
        method: 'videoFramesVisionV2',
        groups: [{ id: id(7), itemIds: items.map((i) => i.id) }],
        images: items.map((i, n) => ({
          id: i.id,
          width: 160 * (n + 1),
          height: 96 * (n + 1),
          thumbnail,
        })),
        videos: items.map((i) => ({
          id: i.id,
          durationMs: 3000,
          frames: [300, 1500, 2700].map((atMs) => ({ atMs, thumbnail })),
        })),
        checkedFiles: 2,
        bytesRead: 3072,
      },
    },
  }
}
describe('visual video candidate protocol', () => {
  it('retains distinct files, exact group IDs and three ordered sample positions', () => {
    const report = readPersonalSnapshot(snapshot()).report!
    expect(report.similarPhotos?.method).toBe('videoFramesVisionV2')
    expect(report.items.map((i) => i.size)).toEqual([1024, 2048])
    expect(report.similarPhotos?.videos?.[1].frames.map((f) => f.atMs)).toEqual([300, 1500, 2700])
  })
  it('refuses incomplete, reordered, out-of-scope or mismatched video evidence', () => {
    const mutations = [
      (r: any) => {
        r.similarPhotos.videos[0].frames.pop()
      },
      (r: any) => {
        r.similarPhotos.videos[0].frames.reverse()
      },
      (r: any) => {
        r.similarPhotos.videos[0].frames[0].atMs = 0
      },
      (r: any) => {
        r.similarPhotos.videos[1].durationMs = 8000
      },
      (r: any) => {
        r.similarPhotos.videos[1].id = id(1)
      },
      (r: any) => {
        r.similarPhotos.images[1].width = 300
      },
      (r: any) => {
        r.similarPhotos.images[1].height = 384
      },
      (r: any) => {
        r.similarPhotos.method = 'visionFeaturePrintV2'
      },
      (r: any) => {
        r.similarPhotos.sequence = { missingMetadata: 0 }
      },
      (r: any) => {
        r.items[0].path = '/Users/demo/Movies/clip-1.png'
        r.items[0].name = 'clip-1.png'
      },
      (r: any) => {
        r.items[0].size = 512 * 1024 ** 2 + 1
      },
      (r: any) => {
        r.similarPhotos.bytesRead = 2 * 1024 ** 3 + 1
      },
    ]
    for (const mutate of mutations) {
      const s = snapshot()
      mutate(s.report)
      expect(() => readPersonalSnapshot(s)).toThrow()
    }
  })
})
