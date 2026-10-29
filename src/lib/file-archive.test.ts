import { expect, it } from 'vitest'
import { readFileArchive } from './file-archive'
it('archive completion needs verified content and unique exact items; unknown publication stays unknown', () => {
  const record = {
    id: '12345678-1234-4123-8123-123456789abc',
    requestId: '22345678-1234-4123-8123-123456789abc',
    createdAt: 100,
    finishedAt: 200,
    state: 'completed',
    phase: 'finished',
    outputPath: '/Users/demo/Documents/owned.zip',
    temporaryPath: '/Users/demo/Documents/.partial',
    reason: 'done',
    reviewedAt: null,
    entries: [
      {
        id: '32345678-1234-4123-8123-123456789abc',
        path: '/Users/demo/Downloads/owned.txt',
        name: 'owned.txt',
        size: 12,
        state: 'archived',
        sha256: 'a'.repeat(64),
      },
    ],
    artifact: { device: 1, inode: 2, size: 100, sha256: 'b'.repeat(64) },
  }
  expect(readFileArchive(record).state).toBe('completed')
  for (const alter of [
    (r: any) => {
      r.artifact = null
    },
    (r: any) => {
      r.entries[0].state = 'verified'
    },
    (r: any) => {
      r.entries.push({ ...r.entries[0] })
    },
    (r: any) => {
      r.entries[0].name = '../owned.txt'
    },
  ]) {
    const copy = structuredClone(record)
    alter(copy)
    expect(() => readFileArchive(copy)).toThrow()
  }
  expect(readFileArchive({ ...record, state: 'interrupted', reviewedAt: 300 }).state).toBe(
    'interrupted',
  )
})
