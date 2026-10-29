import { describe, it, expect } from 'vitest'
import { readFolderSchedules, type FolderSchedulesView } from './folder-schedules'
const id = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
function fixture(): FolderSchedulesView {
  return {
    watches: [
      {
        id: id(1),
        root: '/Users/owned/Downloads',
        enabled: true,
        intervalMinutes: 15,
        updatedAt: 100,
        lastAttemptAt: 200,
      },
    ],
    records: [
      {
        version: 1,
        id: id(2),
        watchId: id(1),
        generation: id(3),
        root: '/Users/owned/Downloads',
        trigger: 'scheduled',
        startedAt: 200,
        finishedAt: 300,
        state: 'completed',
        summary: {
          complete: true,
          visited: 3,
          skipped: 0,
          files: 2,
          logicalBytes: 100,
          comparison: 'compared',
          added: 1,
          changed: 0,
          notSeen: 1,
          logicalDeltaBytes: 10,
          changes: [
            { path: '/Users/owned/Downloads/new.txt', kind: 'added' },
            { path: '/Users/owned/Downloads/old.txt', kind: 'notSeen' },
          ],
          changesTruncated: false,
        },
        error: null,
      },
    ],
    activeRequests: [],
    error: null,
  }
}
describe('scheduled directory observations', () => {
  it('accepts historical metadata clues and rejects out-of-scope or incomplete disappearance claims', () => {
    const v = fixture()
    expect(readFolderSchedules(v).records[0].summary?.notSeen).toBe(1)
    v.records[0].summary!.changes[0].path = '/Users/another/private.txt'
    expect(() => readFolderSchedules(v)).toThrow()
    const partial = fixture()
    partial.records[0].state = 'partial'
    partial.records[0].summary!.complete = false
    expect(() => readFolderSchedules(partial)).toThrow()
  })
  it('accepts cancelled partial observations without implying a missing file or a complete comparison', () => {
    const v = fixture(),
      r = v.records[0]
    r.state = 'cancelled'
    r.summary = {
      complete: false,
      visited: 1,
      skipped: 0,
      files: 1,
      logicalBytes: 50,
      comparison: 'unavailable',
      added: null,
      changed: null,
      notSeen: null,
      logicalDeltaBytes: null,
      changes: [],
      changesTruncated: false,
    }
    expect(readFolderSchedules(v).records[0].summary!.notSeen).toBeNull()
    r.state = 'completed'
    expect(() => readFolderSchedules(v)).toThrow()
  })
})
