import { describe, expect, it } from 'vitest'
import { moleUninstallRevision, parseApplicationInspection } from './application-inspection'

const valid = () => ({
  protocol: 8,
  checkedAt: 1700000000000,
  bundleId: 'org.example.app',
  version: '1.2',
  build: '42',
  identity: 'matches',
  installations: {
    version: 2,
    source: 'launchServicesAndMoleList',
    revision: moleUninstallRevision,
    code: 'observed',
    issue: null,
    copies: [] as {
      path: string
      bundleId: string
      version: string | null
      build: string | null
      relation: string
    }[],
  },
  associations: {
    version: 1,
    revision: moleUninstallRevision,
    code: 'unknown',
    issue: 'homeUnavailable',
    entries: [],
  },
  background: {
    version: 1,
    revision: moleUninstallRevision,
    code: 'notRequired',
    marker: 'absent',
    rootKind: null,
    reason: null,
    issue: null,
  },
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
      { protocol: 9 },
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
      installations: { ...valid().installations, code: 'unknown', issue: 'identityChanged' },
      associations: { ...valid().associations, issue: 'identityChanged' },
      background: { ...valid().background, code: 'unknown', issue: 'identityChanged' },
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
      installations: { ...valid().installations, code: 'unknown', issue: 'identityUnavailable' },
      associations: { ...valid().associations, issue: 'identityUnavailable' },
      bundleId: null,
      background: { ...valid().background, code: 'unknown', issue: 'identityUnavailable' },
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
  it('keeps background marker, root membership, restrictions and unknown evidence consistent', () => {
    const base = valid().background
    for (const rootKind of [
      'systemApplications',
      'userApplications',
      'systemInputMethods',
      'userInputMethods',
      'playCover',
      'externalApplications',
    ])
      expect(
        parse({
          ...valid(),
          background: { ...base, code: 'directRoot', marker: 'enabled', rootKind },
        }).background.rootKind,
      ).toBe(rootKind)
    for (const reason of ['outsideRoots', 'unreadableRoot', 'duplicateRoot'])
      expect(
        parse({
          ...valid(),
          background: { ...base, code: 'restricted', marker: 'enabled', reason },
        }).background.reason,
      ).toBe(reason)
    expect(
      parse({
        ...valid(),
        background: { ...base, code: 'unknown', marker: 'unknown', issue: 'markerUnknown' },
      }).background.code,
    ).toBe('unknown')
    for (const background of [
      undefined,
      {},
      { ...base, version: 2 },
      { ...base, revision: '0'.repeat(40) },
      { ...base, code: 'eligible' },
      { ...base, marker: 'enabled' },
      { ...base, code: 'directRoot' },
      { ...base, code: 'directRoot', marker: 'enabled', rootKind: 'other' },
      {
        ...base,
        code: 'directRoot',
        marker: 'enabled',
        rootKind: 'playCover',
        reason: 'outsideRoots',
      },
      { ...base, code: 'restricted', marker: 'enabled' },
      {
        ...base,
        code: 'restricted',
        marker: 'enabled',
        reason: 'outsideRoots',
        rootKind: 'playCover',
      },
      { ...base, code: 'unknown' },
      { ...base, code: 'unknown', issue: 'markerUnknown' },
      { ...base, code: 'unknown', issue: 'rootsChanged' },
      { ...base, code: 'unknown', issue: 'identityChanged' },
      {
        ...base,
        code: 'unknown',
        marker: 'enabled',
        issue: 'rootsUnavailable',
        rootKind: 'playCover',
      },
    ])
      expect(() => parse({ ...valid(), background })).toThrow('无法识别')
  })
  it('bounds exact candidate paths and never accepts an ownership or deletion claim', () => {
    const locations = [
      ['support', 'Library/Application Support', ''],
      ['cache', 'Library/Caches', ''],
      ['logs', 'Library/Logs', ''],
      ['preferences', 'Library/Preferences', '.plist'],
      ['savedState', 'Library/Saved Application State', '.savedState'],
      ['container', 'Library/Containers', ''],
      ['webKit', 'Library/WebKit', ''],
      ['httpStorage', 'Library/HTTPStorages', ''],
    ]
    const entries = locations.map(([kind, root, suffix], index) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: index === 0 ? 'present' : index === 1 ? 'unknown' : 'notFound',
      issue: index === 1 ? 'unsupported' : null,
    }))
    const associations = {
      version: 1,
      revision: moleUninstallRevision,
      code: 'observed',
      issue: null,
      entries,
    }
    expect(parse({ ...valid(), associations }).associations.entries[0].status).toBe('present')
    for (const invalid of [
      undefined,
      {},
      { ...associations, version: 2 },
      { ...associations, revision: 'other' },
      { ...associations, code: 'safe' },
      { ...associations, issue: 'homeUnavailable' },
      { ...associations, entries: entries.slice(1) },
      { ...associations, entries: [...entries, entries[0]] },
      ...[
        { kind: 'other' },
        { relativePath: '/etc/passwd' },
        { relativePath: 'Library/Application Support/../org.example.app' },
        { relativePath: 'Library/Application Support/org.other.app' },
        { status: 'owned' },
        { status: 'unknown', issue: null },
        { issue: 'changed' },
      ].map((entry) => ({
        ...associations,
        entries: [{ ...entries[0], ...entry }, ...entries.slice(1)],
      })),
      { ...associations, code: 'unknown', issue: 'homeUnavailable' },
    ])
      expect(() => parse({ ...valid(), associations: invalid })).toThrow('无法识别')
  })
  it('keeps partial registered copies separate from exhaustive absence and rejects mismatched identities', () => {
    const copy = {
      path: '/Applications/Other.app',
      bundleId: 'ORG.EXAMPLE.APP',
      version: '3.0',
      build: null,
      relation: 'sameIdentifier',
    }
    const installations = { ...valid().installations, copies: [copy] }
    expect(parse({ ...valid(), installations }).installations.copies).toEqual([copy])
    for (const issue of ['limitReached', 'candidateUnavailable', 'candidatesChanged'])
      expect(
        parse({ ...valid(), installations: { ...installations, code: 'partial', issue } })
          .installations.copies,
      ).toHaveLength(1)
    expect(
      parse({
        ...valid(),
        installations: { ...valid().installations, code: 'partial', issue: 'registryUnavailable' },
      }).installations.code,
    ).toBe('partial')
    expect(() =>
      parse(
        {
          ...valid(),
          bundleId: 'org.example.k',
          installations: { ...installations, copies: [{ ...copy, bundleId: 'org.example.K' }] },
        },
        'org.example.k',
      ),
    ).toThrow('无法识别')
    for (const invalid of [
      undefined,
      {},
      { ...installations, version: 3 },
      { ...installations, source: 'inventory' },
      { ...installations, code: 'onlyCopy' },
      { ...installations, code: 'partial' },
      { ...installations, issue: 'limitReached' },
      { ...installations, code: 'unknown', issue: 'registryUnavailable' },
      { ...installations, copies: [copy, copy] },
      {
        ...installations,
        copies: Array.from({ length: 97 }, (_, i) => ({
          ...copy,
          path: `/Applications/Copy${i}.app`,
        })),
      },
      ...[
        { path: 'relative.app' },
        { path: '/Applications/../Other.app' },
        { path: '/Applications/Host.app/Helper.app' },
        { path: '/Applications/Bad\n.app' },
        { bundleId: 'org.other.app' },
        { version: 42 },
        { build: 'bad\n' },
      ].map((change) => ({ ...installations, copies: [{ ...copy, ...change }] })),
    ])
      expect(() => parse({ ...valid(), installations: invalid })).toThrow('无法识别')
  })
  it('retains derived identifier and channel matches without accepting contradictory reasons', () => {
    const copy = {
      path: '/Applications/Example Beta.app',
      bundleId: 'org.example.app.share',
      version: null,
      build: null,
      relation: 'relatedIdentifier',
    }
    const installations = { ...valid().installations, copies: [copy] }
    expect(parse({ ...valid(), installations }).installations.copies[0].relation).toBe(
      'relatedIdentifier',
    )
    const channel = { ...copy, bundleId: 'org.other.app', relation: 'channelName' }
    expect(
      parse({ ...valid(), installations: { ...installations, copies: [channel] } }).installations
        .copies[0].relation,
    ).toBe('channelName')
    for (const change of [
      { relation: 'sameIdentifier' },
      { relation: 'channelName' },
      { relation: 'owned' },
      { relation: undefined },
      { bundleId: 'org.example.apple' },
      { bundleId: 'org.example.app' },
    ])
      expect(() =>
        parse({
          ...valid(),
          installations: { ...installations, copies: [{ ...copy, ...change }] },
        }),
      ).toThrow('无法识别')
  })
})
