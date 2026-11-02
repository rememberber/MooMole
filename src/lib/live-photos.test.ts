import { expect, it } from 'vitest'
import {
  readPersonalSnapshot,
  readPersonalConfirmation,
  type PersonalReport,
} from './personal-files'
const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
const now = Date.now()
function report(): PersonalReport {
  return {
    id: uuid(1),
    root: '/Users/demo/Exports',
    createdAt: now,
    expiresAt: now + 900000,
    complete: true,
    cancelled: false,
    skipped: 0,
    visited: 2,
    items: [
      {
        id: uuid(2),
        name: 'Photo.jpg',
        path: '/Users/demo/Exports/Photo.jpg',
        size: 12000,
        modifiedAt: now,
      },
      {
        id: uuid(3),
        name: 'Photo.mov',
        path: '/Users/demo/Exports/Photo.mov',
        size: 36000,
        modifiedAt: now,
      },
    ],
    livePhotos: {
      pairs: [{ id: uuid(4), photoId: uuid(2), movieId: uuid(3) }],
      checkedCandidates: 1,
      rejectedCandidates: 0,
      unpairedFiles: 0,
      bytesRead: 48000,
    },
  }
}
it('pair reports reject mixed sources, reused components, mismatched paths and counts', () => {
  const r = report()
  const state = { phase: 'ready', requestId: uuid(5), runId: null, error: null, report: r }
  expect(readPersonalSnapshot(state).report?.livePhotos?.pairs).toHaveLength(1)
  for (const mutate of [
    (r: PersonalReport) => {
      r.livePhotos!.pairs[0].movieId = uuid(2)
    },
    (r: PersonalReport) => {
      r.items[1].path = '/Users/demo/Exports/Other.mov'
      r.items[1].name = 'Other.mov'
    },
    (r: PersonalReport) => {
      r.livePhotos!.rejectedCandidates = 1
    },
    (r: PersonalReport) => {
      r.livePhotos!.pairs.push(r.livePhotos!.pairs[0])
    },
    (r: PersonalReport) => {
      r.duplicates = { groups: [], bytesRead: 0, checkedFiles: 0, extraBytes: 0 }
    },
  ]) {
    const changed = report()
    mutate(changed)
    expect(() => readPersonalSnapshot({ ...state, report: changed })).toThrow()
  }
})
it('confirmation binds complete selected pairs and rejects missing or substituted pair evidence', () => {
  const r = report(),
    ids = r.items.map((i) => i.id),
    draft = {
      id: uuid(6),
      root: r.root,
      createdAt: now,
      expiresAt: now + 30000,
      items: r.items,
      livePhotoPairs: r.livePhotos!.pairs,
    }
  expect(readPersonalConfirmation(draft, r, ids, now).livePhotoPairs).toHaveLength(1)
  for (const changed of [
    { ...draft, livePhotoPairs: undefined },
    { ...draft, items: [r.items[0]] },
    { ...draft, livePhotoPairs: [{ ...draft.livePhotoPairs[0], id: uuid(7) }] },
  ])
    expect(() => readPersonalConfirmation(changed, r, ids, now)).toThrow()
  expect(() =>
    readPersonalConfirmation(draft, r, ids, now, {
      directory: '/Users/demo/Elsewhere',
      conflict: 'skip',
    }),
  ).toThrow()
})
