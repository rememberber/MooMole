import { expect, it } from 'vitest'
import { readDeviceBackups, type BackupView } from './device-backups'
const id = '12345678-1234-4123-8123-123456789abc'
function view(): BackupView {
  return {
    phase: 'ready',
    requestId: null,
    activeRequests: [],
    error: null,
    report: {
      id,
      root: '/Users/demo/Backups',
      createdAt: 100,
      expiresAt: 900100,
      complete: false,
      cancelled: false,
      reason: 'unknown formats preserved',
      items: [
        {
          id,
          path: '/Users/demo/Backups/owned',
          name: 'owned',
          deviceName: 'Owned Phone',
          product: null,
          encrypted: null,
          markedFinished: true,
          modifiedAt: 90,
          size: 1024,
          entries: 20,
          eligible: true,
          reason: 'status only',
        },
      ],
    },
    records: [],
  }
}
it('keeps incomplete inventory, unknown encryption and non-reclaimable logical size explicit', () => {
  expect(readDeviceBackups(view()).report).toMatchObject({
    complete: false,
    items: [{ encrypted: null, size: 1024 }],
  })
  for (const change of [
    (v: BackupView) => {
      v.report!.items[0].path = '/other/owned'
    },
    (v: BackupView) => {
      v.report!.items[0].markedFinished = false
    },
    (v: BackupView) => {
      v.activeRequests = [id]
    },
    (v: BackupView) => {
      v.report!.items.push({ ...v.report!.items[0] })
    },
  ]) {
    const v = view()
    change(v)
    expect(() => readDeviceBackups(v)).toThrow()
  }
})
it('requires exact whole-backup records and preserves decimal receipt identities', () => {
  const v = view(),
    item = v.report!.items[0]
  v.records = [
    {
      id,
      requestId: id,
      revision: 'MooMole.device-backups.v1',
      state: 'moved',
      phase: 'finished',
      createdAt: 100,
      finishedAt: 200,
      reviewedAt: null,
      reason: 'whole backup, not freed space',
      plan: { id, root: v.report!.root, item: { row: item } },
      receipt: { destination: '/Users/demo/.Trash/owned', device: '1', inode: '9007199254740993' },
    },
  ]
  expect(readDeviceBackups(v).records[0].receipt!.inode).toBe('9007199254740993')
  const noReceipt = structuredClone(v)
  noReceipt.records[0].receipt = null
  expect(() => readDeviceBackups(noReceipt)).toThrow()
  const unknown = structuredClone(v)
  unknown.records[0].state = 'unknown'
  unknown.records[0].receipt = null
  unknown.records[0].reviewedAt = 300
  expect(readDeviceBackups(unknown).records[0].state).toBe('unknown')
})
