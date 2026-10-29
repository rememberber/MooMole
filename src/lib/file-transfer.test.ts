import { expect, it } from 'vitest'
import { readTransferRecord } from './file-transfer'
it('a verified destination copy cannot be presented as a completed move without the original Trash receipt', () => {
  const r = {
    protocol: 1,
    revision: 'MooMole.cross-volume-move.v1',
    id: '12345678-1234-4123-8123-123456789abc',
    requestId: '22345678-1234-4123-8123-123456789abc',
    createdAt: 100,
    finishedAt: 200,
    state: 'completed',
    reviewedAt: null,
    reason: 'done',
    items: [
      {
        id: '32345678-1234-4123-8123-123456789abc',
        path: '/Users/demo/Downloads/owned.txt',
        target: '/Volumes/Owned/owned.txt',
        temporaryPath: '/Volumes/Owned/.partial',
        state: 'moved',
        reason: 'done',
        copyReceipt: { destination: '/Volumes/Owned/owned.txt', device: '2', inode: '20' },
        sourceTrashReceipt: {
          destination: '/Users/demo/.Trash/owned.txt',
          device: '1',
          inode: '10',
        },
        sha256: 'a'.repeat(64),
        metadataSha256: 'b'.repeat(64),
      },
    ],
  }
  expect(readTransferRecord(r).state).toBe('completed')
  expect(() =>
    readTransferRecord({ ...r, items: [{ ...r.items[0], sourceTrashReceipt: null }] }),
  ).toThrow()
  const copyOnly = {
    ...r,
    state: 'partial',
    items: [{ ...r.items[0], state: 'copyOnly', sourceTrashReceipt: null }],
  }
  expect(readTransferRecord(copyOnly).items[0].state).toBe('copyOnly')
  expect(() => readTransferRecord({ ...copyOnly, state: 'completed' })).toThrow()
  expect(readTransferRecord({ ...copyOnly, state: 'interrupted', reviewedAt: 300 }).state).toBe(
    'interrupted',
  )
})
