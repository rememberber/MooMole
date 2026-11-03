import { beforeEach, it, expect, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import { mainAppLoginTransport, readLoginView, readLoginRecord } from './main-app-login'
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
const id = 'a1000000-0000-4000-8000-000000000001'
const system = {
  bundleId: 'io.github.rememberber.moomole',
  bundlePath: '/Applications/MooMole.app',
  status: 'notRegistered',
}
const record = {
  version: 1,
  id,
  enabled: true,
  before: system,
  after: { ...system, status: 'enabled' },
  createdAt: 1000,
  finishedAt: 1001,
  state: 'accepted',
  error: null,
  nativeCode: null,
  reviewedAt: null,
}
beforeEach(() => vi.mocked(invoke).mockReset())
it('requires exact own-App state and coherent durable results', () => {
  expect(readLoginRecord(record)).toEqual(record)
  for (const patch of [
    { version: 2 },
    { state: 'running' },
    { after: system },
    { before: { ...system, bundleId: 'org.other.app' } },
    { after: { ...record.after, bundlePath: '/Applications/Other.app' } },
    { nativeCode: Number.MAX_SAFE_INTEGER + 1 },
  ])
    expect(() => readLoginRecord({ ...record, ...patch })).toThrow()
  expect(() =>
    readLoginView({ system, records: [record, record], activeRequests: [], error: null }),
  ).toThrow()
  expect(() =>
    readLoginView({ system, records: [record], activeRequests: [id], error: null }),
  ).toThrow()
  expect(readLoginView({ system, records: [], activeRequests: [], error: null }).records).toEqual(
    [],
  )
})
it('only sends an explicit desired state and request ID, and binds the returned copy', async () => {
  vi.mocked(invoke).mockResolvedValue(record)
  await expect(
    mainAppLoginTransport.change(true, id, { ...system, status: 'notRegistered' }),
  ).resolves.toEqual(record)
  expect(invoke).toHaveBeenCalledWith('set_main_app_login', {
    enabled: true,
    requestId: id,
    confirmed: true,
  })
  vi.mocked(invoke).mockResolvedValue({ ...record, id: 'a2000000-0000-4000-8000-000000000002' })
  await expect(
    mainAppLoginTransport.change(true, id, { ...system, status: 'notRegistered' }),
  ).rejects.toThrow()
})
it('keeps denied or lost outcomes unknown and never manufactures success from a fresh system reading', () => {
  const r = { ...record, state: 'unknown', after: null, error: 'loginResultUnknown' }
  expect(readLoginRecord(r).state).toBe('unknown')
  expect(
    readLoginView({ system: record.after, records: [r], activeRequests: [], error: null })
      .records[0].state,
  ).toBe('unknown')
  expect(() => readLoginRecord({ ...r, state: 'accepted' })).toThrow()
  expect(() => readLoginRecord({ ...record, state: 'notStarted' })).toThrow()
})
