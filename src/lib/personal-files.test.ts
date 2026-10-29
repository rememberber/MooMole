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
})
