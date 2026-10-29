import { describe, expect, it } from 'vitest'
import {
  filterPersonalFiles,
  readPersonalConfirmation,
  readPersonalSnapshot,
  type PersonalReport,
} from './personal-files'
const at = 1_790_000_000_000
const first = '12345678-1234-4123-8123-123456789abc',
  second = '22345678-1234-4123-8123-123456789abc'
function report(): PersonalReport {
  return {
    id: first,
    root: '/Users/demo/Downloads',
    createdAt: at,
    expiresAt: at + 900000,
    complete: true,
    cancelled: false,
    skipped: 0,
    visited: 2,
    items: [
      {
        id: first,
        path: '/Users/demo/Downloads/old.zip',
        name: 'old.zip',
        size: 200 * 1024 ** 2,
        modifiedAt: at - 100 * 86400000,
      },
      {
        id: second,
        path: '/Users/demo/Downloads/recent.pdf',
        name: 'recent.pdf',
        size: 400 * 1024 ** 2,
        modifiedAt: at,
      },
    ],
  }
}
describe('personal file boundaries', () => {
  it('offline media reports require a playlist in the reported account scope and cannot relabel another source', () => {
    const source = report()
    source.softwareContent = {
      source: 'vlcPlaylist',
      minimumAgeDays: 0,
      minimumDirectoryBytes: 0,
      minimumSizeMet: true,
      executionSupported: true,
      playlistPath: `${source.root}/offline.m3u8`,
    }
    const snapshot = { phase: 'ready', requestId: first, runId: null, report: source, error: null }
    expect(readPersonalSnapshot(snapshot).report?.softwareContent?.playlistPath).toBe(
      `${source.root}/offline.m3u8`,
    )
    for (const mutate of [
      (s: any) => {
        delete s.report.softwareContent.playlistPath
      },
      (s: any) => {
        s.report.softwareContent.playlistPath = '/Users/other/collection.m3u8'
      },
      (s: any) => {
        s.report.softwareContent.source = 'chromeDownloads'
      },
    ]) {
      const changed = structuredClone(snapshot)
      mutate(changed)
      expect(() => readPersonalSnapshot(changed)).toThrow()
    }
  })
  it('similar groups allow different content sizes but bind thumbnails and the retained original', () => {
    const source = report()
    source.items = source.items.map((i, index) => ({
      ...i,
      path: `${source.root}/${index}.png`,
      name: `${index}.png`,
      size: 10000 + index * 10000,
    }))
    source.similarPhotos = {
      method: 'visionFeaturePrintV2',
      groups: [{ id: first, itemIds: [first, second] }],
      images: source.items.map((i) => ({
        id: i.id,
        width: 640,
        height: 480,
        thumbnail: 'data:image/png;base64,iVBORw0KGgoAAAA=',
      })),
      checkedFiles: 2,
      bytesRead: 30000,
    }
    const snapshot = { phase: 'ready', requestId: first, runId: null, report: source, error: null }
    expect(readPersonalSnapshot(snapshot).report?.similarPhotos?.groups).toHaveLength(1)
    const raw = {
      id: second,
      root: source.root,
      createdAt: at + 1,
      expiresAt: at + 30000,
      items: [source.items[0]],
      keptFiles: [source.items[1]],
    }
    expect(readPersonalConfirmation(raw, source, [first], at + 2).keptFiles?.[0].size).toBe(20000)
    expect(() =>
      readPersonalConfirmation({ ...raw, keptFiles: [source.items[0]] }, source, [first], at + 2),
    ).toThrow()
    for (const mutate of [
      (s: any) => {
        s.report.similarPhotos.method = 'guessed'
      },
      (s: any) => {
        s.report.similarPhotos.images[0].thumbnail = 'data:image/svg+xml,<svg/>'
      },
      (s: any) => {
        s.report.similarPhotos.groups[0].itemIds = [first, first]
      },
    ]) {
      const changed = structuredClone(snapshot)
      mutate(changed)
      expect(() => readPersonalSnapshot(changed)).toThrow()
    }
  })
  it('requires duplicate groups and confirmation to retain a distinct known copy', () => {
    const source = report()
    source.items[1].size = source.items[0].size
    source.duplicates = {
      groups: [{ id: 'a'.repeat(64), size: source.items[0].size, itemIds: [first, second] }],
      extraBytes: source.items[0].size,
      bytesRead: 2 * source.items[0].size,
      checkedFiles: 2,
    }
    const snapshot = { phase: 'ready', requestId: first, runId: null, report: source, error: null }
    expect(readPersonalSnapshot(snapshot).report?.duplicates?.groups).toHaveLength(1)
    const raw = {
      id: second,
      root: source.root,
      createdAt: at + 1,
      expiresAt: at + 30000,
      items: [source.items[0]],
      keptFiles: [source.items[1]],
    }
    expect(readPersonalConfirmation(raw, source, [first], at + 2).keptFiles?.[0].id).toBe(second)
    for (const keptFiles of [
      undefined,
      [],
      [source.items[0]],
      [{ ...source.items[1], path: '/Users/demo/Downloads/other.pdf', name: 'other.pdf' }],
    ])
      expect(() =>
        readPersonalConfirmation({ ...raw, keptFiles }, source, [first], at + 2),
      ).toThrow()
    const malformed = structuredClone(snapshot)
    malformed.report.duplicates!.groups[0].itemIds = [first, first]
    expect(() => readPersonalSnapshot(malformed)).toThrow()
  })
  it('rejects outside-root, malformed and duplicate source rows instead of dropping evidence', () => {
    const valid = { phase: 'ready', requestId: first, runId: null, report: report(), error: null }
    expect(readPersonalSnapshot(valid).report?.items).toHaveLength(2)
    for (const patch of [
      { path: '/Users/demo/Downloads2/old.zip' },
      { path: '/Users/demo/Downloads/../old.zip' },
      { size: -1 },
      { size: Number.MAX_SAFE_INTEGER + 1 },
      { modifiedAt: NaN },
      { name: 'different.zip' },
    ]) {
      const changed = structuredClone(valid)
      Object.assign(changed.report.items[0], patch)
      expect(() => readPersonalSnapshot(changed)).toThrow()
    }
    const duplicate = structuredClone(valid)
    duplicate.report.items.push(duplicate.report.items[0])
    expect(() => readPersonalSnapshot(duplicate)).toThrow()
    expect(() => readPersonalSnapshot({ ...valid, phase: 'success-guessed' })).toThrow()
  })
  it('confirmation retains exact selected identities, paths, sizes and lifetime', () => {
    const source = report(),
      raw = {
        id: second,
        root: source.root,
        createdAt: at + 1,
        expiresAt: at + 30000,
        items: [source.items[0]],
      }
    expect(readPersonalConfirmation(raw, source, [first], at + 2).items[0]).toEqual(source.items[0])
    for (const patch of [
      { root: '/other' },
      { items: [source.items[1]] },
      { expiresAt: at + 31000 },
      { items: [{ ...source.items[0], size: 0 }] },
    ])
      expect(() =>
        readPersonalConfirmation({ ...raw, ...patch }, source, [first], at + 2),
      ).toThrow()
    expect(() => readPersonalConfirmation(raw, source, [first], raw.expiresAt)).toThrow()
  })
  it('combines type, size, date and search filters using modification time without changing source order', () => {
    const source = report(),
      original = structuredClone(source)
    expect(
      filterPersonalFiles(source.items, 'old', 'archive', 100 * 1024 ** 2, 90, at, 'size').map(
        (i) => i.id,
      ),
    ).toEqual([first])
    expect(filterPersonalFiles(source.items, '', 'document', 0, 30, at, 'modified')).toEqual([])
    expect(filterPersonalFiles(source.items, '', 'all', 0, 0, at, 'size').map((i) => i.id)).toEqual(
      [second, first],
    )
    expect(source).toEqual(original)
  })
  it('binds movement mode, exact destinations, conflict choice and skipped targets', () => {
    const source = report(),
      expected = { directory: '/Users/demo/Organized', conflict: 'keepBoth' as const }
    const raw = {
      id: second,
      root: source.root,
      createdAt: at + 1,
      expiresAt: at + 30000,
      items: source.items,
      movement: {
        ...expected,
        targets: [
          { id: first, destination: '/Users/demo/Organized/old (1).zip' },
          { id: second, destination: null },
        ],
      },
    }
    expect(
      readPersonalConfirmation(raw, source, [first, second], at + 2, expected).movement?.targets[1]
        .destination,
    ).toBeNull()
    expect(() => readPersonalConfirmation(raw, source, [first, second], at + 2)).toThrow()
    for (const movement of [
      undefined,
      { ...raw.movement, conflict: 'skip' },
      { ...raw.movement, directory: '/other' },
      {
        ...raw.movement,
        targets: [
          { id: first, destination: '/Users/demo/Other/old.zip' },
          { id: second, destination: null },
        ],
      },
      {
        ...raw.movement,
        targets: [
          { id: first, destination: '/Users/demo/Organized/OLD.zip' },
          { id: second, destination: '/Users/demo/Organized/old.zip' },
        ],
      },
      {
        ...raw.movement,
        targets: [
          { id: second, destination: '/Users/demo/Organized/old.zip' },
          { id: first, destination: null },
        ],
      },
    ])
      expect(() =>
        readPersonalConfirmation({ ...raw, movement }, source, [first, second], at + 2, expected),
      ).toThrow()
  })
})
