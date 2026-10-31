import { it, expect } from 'vitest'
import {
  readFolderArchiveDraft,
  readFolderArchiveRecord,
  readFolderArchiveView,
} from './folder-archive'
const id = '853bf826-4ae1-4695-bc66-85d49e1957b0',
  requestId = '7205853c-c7fd-47a9-8e83-fcebe84336dd'
const draft = {
  id,
  root: '/Users/demo/Documents/Trip',
  outputPath: '/Users/demo/Archives/Trip.zip',
  files: 1,
  directories: 1,
  hidden: 1,
  bytes: 12345,
  metadataBytes: 4096,
  sourceSha256: 'a'.repeat(64),
  samples: [
    { name: 'Trip', directory: true },
    { name: 'Trip/.notes', directory: false },
  ],
  createdAt: 1000,
  expiresAt: 121000,
}
const record = {
  id,
  requestId,
  draft,
  temporaryPath: `/Users/demo/Archives/.MooMole-folder-archive-${id}.partial`,
  createdAt: 1001,
  finishedAt: 1002,
  state: 'completed',
  phase: 'finished',
  artifact: { device: '1', inode: '9007199254740993', bytes: 4096, sha256: 'b'.repeat(64) },
  reason: 'owned',
  reviewedAt: null,
}
it('whole-folder review binds scope, destination, bounded counts, metadata and safe sample paths', () => {
  expect(
    readFolderArchiveDraft(draft, {
      root: draft.root,
      directory: '/Users/demo/Archives',
      filename: 'Trip.zip',
    }),
  ).toEqual(draft)
  for (const patch of [
    { root: '/Users/demo/../Trip' },
    { outputPath: draft.root + '/Trip.zip' },
    { expiresAt: 121001 },
    { files: 10000 },
    { hidden: 3 },
    { metadataBytes: 256 * 1024 ** 2 + 1 },
    { samples: [{ name: 'Trip/../Secret', directory: false }] },
    { samples: [{ name: 'Other/file', directory: false }] },
  ])
    expect(() => readFolderArchiveDraft({ ...draft, ...patch })).toThrow()
  expect(() =>
    readFolderArchiveDraft(draft, {
      root: draft.root,
      directory: '/Users/demo/Archives',
      filename: 'Other.zip',
    }),
  ).toThrow()
})
it('completed ZIP evidence requires an actual bound artifact and preserves inode precision', () => {
  expect(readFolderArchiveRecord(record)).toEqual(record)
  for (const patch of [
    { artifact: null },
    { finishedAt: null },
    { phase: 'publishing' },
    { temporaryPath: '/Users/demo/Archives/other.partial' },
    { artifact: { ...record.artifact, inode: 9007199254740993 } },
    { artifact: { ...record.artifact, sha256: 'unknown' } },
  ])
    expect(() => readFolderArchiveRecord({ ...record, ...patch })).toThrow()
  const unknown = { ...record, state: 'unknown', phase: 'publishing' }
  expect(readFolderArchiveRecord(unknown).state).toBe('unknown')
})
it('reconnects only exact live requests without treating expected artifact data as publication success', () => {
  const running = { ...record, state: 'running', phase: 'publishing', finishedAt: null }
  expect(
    readFolderArchiveView({ records: [running], activeRequests: [requestId] }).activeRequests,
  ).toEqual([requestId])
  expect(() => readFolderArchiveView({ records: [record], activeRequests: [requestId] })).toThrow()
  expect(() => readFolderArchiveView({ records: [running], activeRequests: [id] })).toThrow()
  expect(() => readFolderArchiveView({ records: [record, record], activeRequests: [] })).toThrow()
})
