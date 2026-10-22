import { expect, it } from 'vitest'
import { projectMeasurement } from '../../tests/e2e/fixtures/project-artifacts'
import { readProjectMeasurement } from './project-artifacts'
it('binds recent modification evidence to the exact target, timestamp and fixed window', () => {
  const base = projectMeasurement()
  const check = (v: unknown) =>
    readProjectMeasurement(v, base.root, base.path, base.checkedAt, base.checkedAt)
  const recent = {
    ...base,
    activity: {
      ...base.activity,
      state: 'recent',
      reason: null,
      evidence: {
        kind: 'file',
        path: base.path + '/[literal]/output',
        modifiedAt: Math.floor(base.activity.referenceAt / 1000) * 1000,
      },
    },
  }
  expect(check(recent).activity.state).toBe('recent')
  expect(
    check({
      ...recent,
      activity: {
        ...recent.activity,
        evidence: { ...recent.activity.evidence, kind: 'directory', path: base.path },
      },
    }).activity.state,
  ).toBe('recent')
  for (const activity of [
    undefined,
    { ...recent.activity, windowDays: 30 },
    { ...recent.activity, state: 'idle' },
    { ...recent.activity, referenceAt: base.checkedAt + 1 },
    { ...recent.activity, referenceAt: base.checkedAt - 6001 },
    { ...recent.activity, evidence: null },
    { ...recent.activity, reason: 'notObserved' },
    ...[
      { path: base.path + '-other/file' },
      { path: base.path + '/a/../file' },
      { path: base.path + '/a//file' },
      { path: base.path + '/bad\nfile' },
      { path: base.path + '/' + 'a'.repeat(4096) },
      { path: base.path },
      { kind: 'directory', path: base.path + '/child' },
      { modifiedAt: Math.floor(base.checkedAt / 1000) * 1000 + 1000 },
      { modifiedAt: -1000 },
      { modifiedAt: 1.5 },
      { modifiedAt: Math.floor((base.activity.referenceAt - 7 * 86400000) / 1000) * 1000 },
    ].map((evidence) => ({
      ...recent.activity,
      evidence: { ...recent.activity.evidence, ...evidence },
    })),
  ])
    expect(() => check({ ...base, activity })).toThrow()
  expect(() => check({ ...base, protocol: 2 })).toThrow()
})
it('keeps absent, partial and withdrawn observations unknown with mutually exclusive evidence', () => {
  const base = projectMeasurement()
  const check = (value: unknown) =>
    readProjectMeasurement(value, base.root, base.path, base.checkedAt, base.checkedAt)
  expect(check(base).activity.state).toBe('unknown')
  expect(
    check({ ...base, activity: { ...base.activity, reason: 'unavailable' } }).activity.state,
  ).toBe('unknown')
  const partial = {
    ...base,
    completion: 'partial',
    skipped: { ...base.skipped, links: 1 },
    contentSignals: { ...base.contentSignals, reason: 'incomplete' },
    activity: { ...base.activity, reason: 'incomplete' },
  }
  expect(check(partial).activity.state).toBe('unknown')
  expect(() => check({ ...partial, activity: base.activity })).toThrow()
  expect(() => check({ ...base, activity: { ...base.activity, reason: 'incomplete' } })).toThrow()
  expect(() =>
    check({
      ...base,
      activity: {
        ...base.activity,
        evidence: { kind: 'directory', path: base.path, modifiedAt: base.checkedAt },
      },
    }),
  ).toThrow()
})
