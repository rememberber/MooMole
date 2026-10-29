import { expect, it } from 'vitest'
import { readStartupRecord, readStartupView, startupRecordName } from './startup-management'
it('startup completion requires a real configuration receipt and reviewed interruption stays unknown', () => {
  const r = {
    id: '12345678-1234-4123-8123-123456789abc',
    requestId: '22345678-1234-4123-8123-123456789abc',
    createdAt: 100,
    finishedAt: 200,
    action: 'pause',
    path: '/Users/demo/Library/LaunchAgents/owned.plist',
    target: '/Users/demo/Library/LaunchAgents/owned.plist.moomole-disabled',
    label: 'org.owned',
    program: '/bin/sleep',
    state: 'completed',
    phase: 'finished',
    configurationChanged: true,
    reason: 'done',
    reviewedAt: null,
    receipt: {
      destination: '/Users/demo/Library/LaunchAgents/owned.plist.moomole-disabled',
      device: '1',
      inode: '2',
    },
  }
  expect(readStartupRecord(r).state).toBe('completed')
  expect(() => readStartupRecord({ ...r, configurationChanged: false })).toThrow()
  expect(() => readStartupRecord({ ...r, receipt: null })).toThrow()
  expect(() =>
    readStartupRecord({ ...r, receipt: { ...r.receipt, destination: '/elsewhere' } }),
  ).toThrow()
  const interrupted = readStartupRecord({ ...r, state: 'interrupted', reviewedAt: 300 })
  expect(startupRecordName(interrupted)).not.toBe(startupRecordName(readStartupRecord(r)))
  expect(
    readStartupView({ report: null, records: [interrupted], activeRequest: null }).records[0].state,
  ).toBe('interrupted')
})
