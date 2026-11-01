import { describe, expect, test } from 'vitest'
import {
  moleCleanScopes,
  readMoleCleanPlan,
  readMoleCleanSnapshot,
  type MoleCleanPlan,
  type MoleCleanSnapshot,
} from './mole-cleanup'
const id = '31d8c490-1245-414b-8d62-0c6aab303555'
const plan = (): MoleCleanPlan => ({
  version: 1,
  id,
  revision: '308bf9911179ddfa40e5df9fdc42095a18ba5cfc',
  engineVersion: '1.65.0',
  home: '/Users/demo',
  system: false,
  method: 'molePermanent',
  protectedPaths: [],
  createdAt: 1000,
  expiresAt: 121000,
  scope: moleCleanScopes.map(([id]) => id),
})
const snapshot = (): MoleCleanSnapshot => ({
  version: 1,
  activeRequest: null,
  authorizationRequired: false,
  records: [],
  current: {
    id,
    plan: plan(),
    state: 'completed',
    createdAt: 1000,
    finishedAt: 2000,
    beforeBytes: null,
    afterBytes: null,
    error: null,
    diagnostic: '',
    reviewedAt: null,
    events: [],
    eventsTruncated: false,
  },
})
describe('full Mole operation protocol', () => {
  test('keeps the full fixed module scope separate from item IDs and Trash plans', () => {
    expect(readMoleCleanPlan(plan()).scope).toHaveLength(15)
    expect(() => readMoleCleanPlan({ ...plan(), method: 'trash' })).toThrow()
    expect(() => readMoleCleanPlan({ ...plan(), scope: ['userEssentials'] })).toThrow()
    expect(() => readMoleCleanPlan({ ...plan(), revision: 'old' })).toThrow()
    expect(() => readMoleCleanPlan({ ...plan(), expiresAt: 999999 })).toThrow()
  })
  test('missing space observations remain unknown; invalid amounts never become a success or zero', () => {
    const s = readMoleCleanSnapshot(snapshot())
    expect(s.current?.beforeBytes).toBeNull()
    expect(s.current?.afterBytes).toBeNull()
    const invalid = snapshot()
    invalid.current!.afterBytes = -1
    expect(() => readMoleCleanSnapshot(invalid)).toThrow()
    expect(() =>
      readMoleCleanSnapshot({
        ...snapshot(),
        current: { ...snapshot().current, state: 'success' },
      }),
    ).toThrow()
  })
  test('credential prompts bind only a running administrator operation with its exact active record', () => {
    const s = snapshot()
    s.activeRequest = id
    s.authorizationRequired = true
    s.current!.state = 'running'
    s.current!.plan.system = true
    expect(readMoleCleanSnapshot(s).authorizationRequired).toBe(true)
    s.current!.plan.system = false
    expect(() => readMoleCleanSnapshot(s)).toThrow()
    s.current!.plan.system = true
    s.activeRequest = null
    expect(() => readMoleCleanSnapshot(s)).toThrow()
    s.activeRequest = id
    s.current = null
    expect(() => readMoleCleanSnapshot(s)).toThrow()
  })
  test('ambiguous or reordered event identities fail instead of being turned into deletions', () => {
    const s = snapshot()
    s.current!.events = [
      { seq: 1, kind: 'intent', path: '/owned/cache', detail: 'safe_remove', token: id },
      { seq: 2, kind: 'removed', path: '/owned/cache', detail: 'safe_remove: 0', token: id },
    ]
    expect(readMoleCleanSnapshot(s).current!.events).toHaveLength(2)
    s.current!.events[1].seq = 1
    expect(() => readMoleCleanSnapshot(s)).toThrow()
    s.current!.events[1].seq = 2
    s.current!.events[1].token = 'unknown'
    expect(() => readMoleCleanSnapshot(s)).toThrow()
  })
})
