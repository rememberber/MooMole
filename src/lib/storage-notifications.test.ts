import { expect, it } from 'vitest'
import { notificationAllowed, readStorageNotifications } from './storage-notifications'
const id = 'a1000000-0000-4000-8000-000000000001'
const raw = {
  enabled: false,
  monitoringEnabled: false,
  permission: { authorization: 'notDetermined', alerts: false, center: true },
  error: null,
  records: [],
}
it('keeps system authorization, application opt-in and sampling independent', () => {
  const view = readStorageNotifications(raw)
  expect(view.enabled).toBe(false)
  expect(notificationAllowed(view.permission)).toBe(false)
  expect(
    readStorageNotifications({
      ...raw,
      permission: { authorization: 'authorized', alerts: true, center: false },
    }).enabled,
  ).toBe(false)
  expect(notificationAllowed({ authorization: 'authorized', alerts: false, center: false })).toBe(
    false,
  )
  expect(
    readStorageNotifications({ ...raw, permission: null, error: 'requiresPackagedApp' }).error,
  ).toBe('requiresPackagedApp')
  for (const patch of [
    { enabled: 1 },
    { permission: null },
    { permission: { ...raw.permission, alerts: 1 } },
    { records: '[]' },
    { error: 0 },
  ])
    expect(() => readStorageNotifications({ ...raw, ...patch })).toThrow()
})
it('submitted means OS accepted, preserves unknown after interruption, and rejects invented scope or completion', () => {
  const record = {
    id,
    kind: 'lowSpace',
    state: 'unknown',
    createdAt: 1000,
    finishedAt: 1001,
    availableBytes: 1024,
    thresholdBytes: 5 * 1024 ** 3,
    error: 'interrupted',
  }
  expect(readStorageNotifications({ ...raw, records: [record] }).records[0].state).toBe('unknown')
  for (const change of [
    { id: 'bad' },
    { createdAt: -1 },
    { finishedAt: 999 },
    { state: 'submitted' },
    { state: 'submitting' },
    { thresholdBytes: 0 },
    { availableBytes: null },
    { error: null },
    { kind: 'test' },
  ])
    expect(() =>
      readStorageNotifications({ ...raw, records: [{ ...record, ...change }] }),
    ).toThrow()
  expect(() => readStorageNotifications({ ...raw, records: [record, record] })).toThrow()
  expect(() => readStorageNotifications({ ...raw, records: Array(51).fill(record) })).toThrow()
})
it('test notifications contain no invented capacity and pending outcomes remain pending', () => {
  const record = {
    id,
    kind: 'test',
    state: 'submitting',
    createdAt: 1000,
    finishedAt: null,
    availableBytes: null,
    thresholdBytes: null,
    error: null,
  }
  expect(readStorageNotifications({ ...raw, records: [record] }).records[0].finishedAt).toBeNull()
  expect(() =>
    readStorageNotifications({ ...raw, records: [{ ...record, availableBytes: 0 }] }),
  ).toThrow()
  expect(
    readStorageNotifications({
      ...raw,
      records: [{ ...record, state: 'submitted', finishedAt: 1001 }],
    }).records[0].state,
  ).toBe('submitted')
})
