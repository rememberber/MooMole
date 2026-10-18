import { describe, expect, it } from 'vitest'
import { moleUninstallRevision, parseApplicationInspection } from './application-inspection'

const valid = () => ({
  protocol: 4,
  checkedAt: 1700000000000,
  bundleId: 'org.example.app',
  version: '1.2',
  build: '42',
  identity: 'matches',
  moleProtection: { version: 1, revision: moleUninstallRevision, code: 'notMatched', issue: null },
  officialUninstaller: {
    version: 1,
    revision: moleUninstallRevision,
    code: 'notMatched',
    vendor: null,
    issue: null,
  },
  protection: { reasons: [] as string[], matchedPaths: [] as string[] },
  running: {
    code: 'observed',
    pids: [17],
    otherCopies: 0,
    reason: '本次发现主应用，其他进程未检查。',
  },
})
const parse = (value: unknown, expected = 'org.example.app') =>
  parseApplicationInspection(value, expected)
describe('application inspection boundary', () => {
  it('keeps observed, not observed and unknown distinct without turning a different copy into this app', () => {
    expect(parse(valid()).running.code).toBe('observed')
    const other = valid()
    other.running = {
      code: 'notObserved',
      pids: [],
      otherCopies: 2,
      reason: '只在其他位置发现主应用。',
    }
    expect(parse(other).running.otherCopies).toBe(2)
    other.running.code = 'unknown'
    expect(parse(other).running.code).toBe('unknown')
  })
  it('rejects future protocols, malformed time, unknown enums and unsupported identity claims', () => {
    for (const change of [
      { protocol: 5 },
      { checkedAt: NaN },
      { checkedAt: -1 },
      { checkedAt: 9e15 },
      { identity: 'verified' },
      { bundleId: 'bad/id' },
      { version: 'unsafe\ntext' },
      { build: 17 },
    ])
      expect(() => parse({ ...valid(), ...change })).toThrow('无法识别')
    expect(() => parse(valid(), 'different.id')).toThrow()
    const mismatch = {
      ...valid(),
      identity: 'different',
      officialUninstaller: {
        ...valid().officialUninstaller,
        code: 'unknown',
        issue: 'identityChanged',
      },
      moleProtection: { ...valid().moleProtection, code: 'unknown', issue: 'identityChanged' },
      running: { code: 'unknown', pids: [], otherCopies: 0, reason: '标识已变化。' },
    }
    expect(parse(mismatch, 'other.id').identity).toBe('different')
    expect(() => parse({ ...mismatch, running: valid().running }, 'other.id')).toThrow()
    const missing = {
      ...valid(),
      identity: 'unavailable',
      bundleId: null,
      officialUninstaller: {
        ...valid().officialUninstaller,
        code: 'unknown',
        issue: 'identityUnavailable',
      },
      moleProtection: { ...valid().moleProtection, code: 'unknown', issue: 'identityUnavailable' },
      running: { code: 'unknown', pids: [], otherCopies: 0, reason: '无法读取。' },
    }
    expect(parse(missing).identity).toBe('unavailable')
  })
  it('rejects invalid PIDs, duplicated IDs, count overflow and inconsistent negative observations', () => {
    for (const running of [
      { ...valid().running, pids: [] },
      { ...valid().running, pids: [0] },
      { ...valid().running, pids: [17, 17] },
      { ...valid().running, pids: [2147483648] },
      { ...valid().running, code: 'notObserved' },
      { ...valid().running, otherCopies: 33 },
      { ...valid().running, code: 'notRunning' },
    ])
      expect(() => parse({ ...valid(), running })).toThrow()
  })
  it('keeps guards separate from unknown ownership and rejects malformed or contradictory guards', () => {
    expect(parse(valid()).protection.reasons).toEqual([])
    for (const reason of ['systemLocation', 'currentApplication'])
      expect(
        parse({ ...valid(), protection: { reasons: [reason], matchedPaths: [] } }).protection
          .reasons,
      ).toEqual([reason])
    const protection = { reasons: ['clientDirectory'], matchedPaths: ['/Applications'] }
    expect(parse({ ...valid(), protection }).protection.matchedPaths).toEqual(['/Applications'])
    for (const invalid of [
      undefined,
      null,
      {},
      { reasons: ['safeToUninstall'], matchedPaths: [] },
      { reasons: ['currentApplication', 'currentApplication'], matchedPaths: [] },
      { reasons: ['clientDirectory'], matchedPaths: [] },
      { reasons: [], matchedPaths: ['/Applications'] },
      { ...protection, matchedPaths: ['relative'] },
      { ...protection, matchedPaths: ['/Applications/../elsewhere'] },
      { ...protection, matchedPaths: ['/bad\npath'] },
      { ...protection, matchedPaths: ['/Applications', '/Applications'] },
      { ...protection, matchedPaths: Array.from({ length: 101 }, (_, i) => `/app${i}`) },
    ])
      expect(() => parse({ ...valid(), protection: invalid })).toThrow('无法识别')
  })
  it('binds Mole observations to the adapted revision and keeps failed rules distinct from no match', () => {
    const base = valid().moleProtection
    expect(
      parse({ ...valid(), moleProtection: { ...base, code: 'protected' } }).moleProtection.code,
    ).toBe('protected')
    const failed = {
      ...valid(),
      protection: { reasons: ['clientDirectory'], matchedPaths: ['/Applications'] },
      moleProtection: { ...base, code: 'unknown', issue: 'rulesUnavailable' },
    }
    expect(parse(failed).protection.reasons).toEqual(['clientDirectory'])
    for (const mole of [
      undefined,
      {},
      { ...base, version: 2 },
      { ...base, revision: '0'.repeat(40) },
      { ...base, code: 'passed' },
      { ...base, code: 'unknown' },
      { ...base, issue: 'rulesUnavailable' },
      { ...base, code: 'unknown', issue: 'other' },
      { ...base, code: 'unknown', issue: 'identityChanged' },
      { ...base, code: 'unknown', issue: 'identityUnavailable' },
    ])
      expect(() => parse({ ...valid(), moleProtection: mole })).toThrow('无法识别')
    expect(() =>
      parse(
        {
          ...valid(),
          identity: 'different',
          moleProtection: { ...base, code: 'protected' },
          running: { code: 'unknown', pids: [], otherCopies: 0, reason: '已变化' },
        },
        'other.id',
      ),
    ).toThrow()
  })
  it('validates official vendors, no-match, missing context and unknown without inventing an uninstall method', () => {
    const base = valid().officialUninstaller
    expect(
      parse({
        ...valid(),
        officialUninstaller: { ...base, code: 'officialRequired', vendor: 'ESET' },
      }).officialUninstaller.vendor,
    ).toBe('ESET')
    expect(
      parse({
        ...valid(),
        officialUninstaller: { ...base, code: 'unknown', issue: 'nameUnavailable' },
      }).officialUninstaller.code,
    ).toBe('unknown')
    for (const official of [
      undefined,
      {},
      { ...base, version: 2 },
      { ...base, revision: '0'.repeat(40) },
      { ...base, code: 'allowed' },
      { ...base, code: 'officialRequired' },
      { ...base, code: 'officialRequired', vendor: 'Unknown vendor' },
      { ...base, vendor: 'ESET' },
      { ...base, code: 'unknown' },
      { ...base, issue: 'nameUnavailable' },
      { ...base, code: 'unknown', issue: 'other' },
      { ...base, code: 'unknown', issue: 'nameUnavailable', vendor: 'ESET' },
      { ...base, code: 'unknown', issue: 'identityChanged' },
      { ...base, code: 'unknown', issue: 'identityUnavailable' },
    ])
      expect(() => parse({ ...valid(), officialUninstaller: official })).toThrow('无法识别')
  })
})
