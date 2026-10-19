import { describe, expect, it } from 'vitest'
import { moleUninstallRevision, parseApplicationInspection } from './application-inspection'

const pathRule = (status = 'notFound') => ({
  version: 2,
  revision: moleUninstallRevision,
  source: 'moleUninstallPathSubset',
  code: status === 'unknown' ? 'unknown' : 'notMatched',
  reason: null as string | null,
  issue: status === 'unknown' ? 'locationUnverified' : (null as string | null),
})
const valid = () => ({
  protocol: 17,
  checkedAt: 1700000000000,
  bundleId: 'org.example.app',
  version: '1.2',
  build: '42',
  identity: 'matches',
  embedded: {
    version: 1,
    revision: moleUninstallRevision,
    code: 'observed',
    issue: null,
    entries: 1,
    directories: 1,
    plists: 1,
    bundles: [] as { relativePath: string; bundleId: string; kind: string }[],
  },
  extensionData: {
    version: 2,
    revision: moleUninstallRevision,
    code: 'observed',
    issue: null as string | null,
    identifiers: [] as string[],
    entries: [] as {
      relativePath: string
      sources: { bundleId: string; kind: string }[]
      status: string
      issue: string | null
      protectionReasons: string[]
      molePathProtection: ReturnType<typeof pathRule>
    }[],
    byHost: { code: 'notChecked', issue: null, entries: 0, candidates: 0 },
  },
  installations: {
    version: 5,
    source: 'moleListRegistryRootsVolumesAndReceipts',
    roots: {
      code: 'observed',
      issue: null,
      rootCount: 9,
      entries: 40,
      candidates: 4,
      firstUncertainPath: null,
    },
    volumes: {
      code: 'observed',
      issue: null,
      rootCount: 2,
      entries: 10,
      candidates: 2,
      firstUncertainPath: null,
    },
    receipts: {
      code: 'observed',
      issue: null,
      packageCount: 20,
      checkedCount: 2,
      entries: 40,
      candidates: 2,
    },
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
    version: 4,
    revision: moleUninstallRevision,
    code: 'unknown',
    issue: 'homeUnavailable',
    entries: [],
    discovery: {
      code: 'notChecked',
      issue: null,
      rootCount: 0,
      entries: 0,
      candidates: 0,
      firstUncertainRoot: null,
    },
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
const associationLocations = [
  ['support', 'Library/Application Support', ''],
  ['cache', 'Library/Caches', ''],
  ['logs', 'Library/Logs', ''],
  ['preferences', 'Library/Preferences', '.plist'],
  ['savedState', 'Library/Saved Application State', '.savedState'],
  ['container', 'Library/Containers', ''],
  ['webKit', 'Library/WebKit', ''],
  ['httpStorage', 'Library/HTTPStorages', ''],
  ['webContent', 'Library/WebKit/com.apple.WebKit.WebContent', ''],
  ['httpCookies', 'Library/HTTPStorages', '.binarycookies'],
  ['cookies', 'Library/Cookies', '.binarycookies'],
  ['scripts', 'Library/Application Scripts', ''],
  ['inputMethod', 'Library/Input Methods', '.app'],
  ['autosave', 'Library/Autosave Information', ''],
  ['syncedPreferences', 'Library/SyncedPreferences', '.plist'],
  ['crashReports', 'Library/Caches/com.plausiblelabs.crashreporter.data', ''],
]

const parse = (value: unknown, expected = 'org.example.app') =>
  parseApplicationInspection(value, expected)
describe('application inspection boundary', () => {
  it('keeps literal uninstall path guards separate from existence and rejects wider or contradictory claims', () => {
    const entries = associationLocations.map(([kind, root, suffix]) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: 'notFound',
      issue: null,
      protectionReasons: [],
      molePathProtection: pathRule(),
    }))
    const base = {
      ...valid(),
      associations: {
        ...valid().associations,
        version: 4,
        code: 'observed',
        issue: null,
        entries,
        discovery: {
          code: 'observed',
          issue: null,
          rootCount: 4,
          entries: 0,
          candidates: 0,
          firstUncertainRoot: null,
        },
      },
    }
    for (const rule of [
      { ...pathRule(), code: 'protected', reason: 'endpointSecurityIdentity' },
      { ...pathRule(), code: 'unknown', issue: 'identityUnverified' },
      { ...pathRule(), code: 'unknown', issue: 'identityLimitReached' },
    ]) {
      for (const status of ['present', 'notFound', 'unknown']) {
        expect(
          parse({
            ...base,
            associations: {
              ...base.associations,
              entries: [
                {
                  ...entries[0],
                  status,
                  issue: status === 'unknown' ? 'unavailable' : null,
                  molePathProtection: rule,
                },
                ...entries.slice(1),
              ],
            },
          }).associations.entries[0].molePathProtection.code,
        ).toBe(rule.code)
      }
    }
    const protectedRule = { ...pathRule(), code: 'protected', reason: 'criticalData' }
    for (const status of ['present', 'notFound', 'unknown']) {
      const entry = {
        ...entries[0],
        status,
        issue: status === 'unknown' ? 'unavailable' : null,
        molePathProtection: protectedRule,
      }
      expect(
        parse({
          ...base,
          associations: { ...base.associations, entries: [entry, ...entries.slice(1)] },
        }).associations.entries[0].molePathProtection.code,
      ).toBe('protected')
    }
    for (const rule of [
      undefined,
      { ...protectedRule, version: 1 },
      { ...protectedRule, revision: 'other' },
      { ...protectedRule, source: 'moleCleanupPath' },
      { ...protectedRule, source: 'moleUninstallLiteralPath' },
      { ...protectedRule, code: 'safe' },
      { ...protectedRule, reason: null },
      { ...protectedRule, reason: 'whitelist' },
      { ...protectedRule, issue: 'rulesUnavailable' },
      { ...pathRule(), reason: 'criticalData' },
      { ...pathRule(), code: 'unknown', issue: null },
      { ...pathRule(), code: 'unknown', issue: 'locationUnverified' },
      { ...pathRule(), code: 'unknown', issue: 'passed' },
    ]) {
      expect(() =>
        parse({
          ...base,
          associations: {
            ...base.associations,
            entries: [{ ...entries[0], molePathProtection: rule }, ...entries.slice(1)],
          },
        }),
      ).toThrow()
    }
    expect(
      parse({
        ...base,
        associations: {
          ...base.associations,
          entries: [
            {
              ...entries[0],
              molePathProtection: { ...pathRule(), code: 'unknown', issue: 'rulesUnavailable' },
            },
            ...entries.slice(1),
          ],
        },
      }).associations.entries[0].status,
    ).toBe('notFound')
    expect(() =>
      parse({
        ...base,
        associations: {
          ...base.associations,
          entries: [
            { ...entries[0], status: 'unknown', issue: 'unavailable' },
            ...entries.slice(1),
          ],
        },
      }),
    ).toThrow()
  })

  it('requires complete exact extension sources, merges collisions, and binds ByHost files to fresh IDs', () => {
    const data = valid()
    const ids = ['org.example.extension', 'org.example.extension.binarycookies']
    data.embedded = {
      ...data.embedded,
      entries: 20,
      directories: 6,
      plists: 3,
      bundles: ids.map((id, index) => ({
        bundleId: id,
        relativePath: `Contents/PlugIns/Extension${index}.appex`,
        kind: 'appex',
      })),
    }
    const locations = [
      ['scripts', 'Library/Application Scripts', ''],
      ['fileProvider', 'Library/Application Support/FileProvider', ''],
      ['cache', 'Library/Caches', ''],
      ['container', 'Library/Containers', ''],
      ['httpStorage', 'Library/HTTPStorages', ''],
      ['httpCookies', 'Library/HTTPStorages', '.binarycookies'],
      ['preferences', 'Library/Preferences', '.plist'],
      ['webKit', 'Library/WebKit', ''],
    ]
    const entries: typeof data.extensionData.entries = []
    for (const id of ids)
      for (const [kind, root, suffix] of locations) {
        const path = `${root}/${id}${suffix}`
        const found = entries.find((entry) => entry.relativePath === path)
        if (found) found.sources.push({ bundleId: id, kind })
        else
          entries.push({
            relativePath: path,
            sources: [{ bundleId: id, kind }],
            status: 'notFound',
            issue: null,
            protectionReasons: [],
            molePathProtection: pathRule(),
          })
      }
    const device = {
      relativePath: 'Library/Preferences/ByHost/org.example.extension.binarycookies.DEVICE.plist',
      sources: ids.map((id) => ({ bundleId: id, kind: 'byHost' })),
      status: 'unknown',
      issue: 'unavailable',
      protectionReasons: ['clientDirectory'],
      molePathProtection: pathRule('unknown'),
    }
    entries.push(device)
    data.extensionData = {
      ...data.extensionData,
      identifiers: ids,
      entries,
      byHost: { code: 'observed', issue: null, entries: 20, candidates: 1 },
    }
    expect(parse(data).extensionData.entries).toHaveLength(16)
    const invalids = [
      { ...data.extensionData, version: 3 },
      { ...data.extensionData, revision: 'other' },
      { ...data.extensionData, code: 'observed', issue: 'embeddedIncomplete' },
      { ...data.extensionData, identifiers: [ids[0]] },
      { ...data.extensionData, entries: entries.slice(1) },
      { ...data.extensionData, entries: [...entries, device] },
      {
        ...data.extensionData,
        entries: [...entries.slice(0, -1), { ...device, sources: [device.sources[0]] }],
      },
      {
        ...data.extensionData,
        entries: [
          ...entries.slice(0, -1),
          { ...device, relativePath: 'Library/Preferences/ByHost/' + ids[0] + '.plist' },
        ],
      },
      {
        ...data.extensionData,
        entries: [
          ...entries.slice(0, -1),
          { ...device, relativePath: 'Library/Preferences/ByHost/../' + ids[0] + '.A.plist' },
        ],
      },
      {
        ...data.extensionData,
        entries: [
          ...entries.slice(0, -1),
          { ...device, sources: [{ bundleId: ids[0], kind: 'constructor' }] },
        ],
      },
      {
        ...data.extensionData,
        entries: [
          ...entries.slice(0, -1),
          { ...device, protectionReasons: ['clientDirectory', 'clientDirectory'] },
        ],
      },
      {
        ...data.extensionData,
        byHost: { code: 'partial', issue: 'changed', entries: 20, candidates: 1 },
      },
      {
        ...data.extensionData,
        byHost: { code: 'observed', issue: null, entries: 1025, candidates: 1 },
      },
      {
        ...data.extensionData,
        byHost: { code: 'observed', issue: null, entries: 20, candidates: 0 },
      },
    ]
    for (const extensionData of invalids) expect(() => parse({ ...data, extensionData })).toThrow()
    expect(
      parse({
        ...data,
        extensionData: {
          ...data.extensionData,
          code: 'partial',
          issue: 'byHostIncomplete',
          byHost: { code: 'partial', issue: 'changed', entries: 20, candidates: 1 },
        },
      }).extensionData.code,
    ).toBe('partial')
    const changed = {
      ...data,
      embedded: { ...data.embedded, code: 'partial', issue: 'changed', bundles: [] },
      extensionData: { ...valid().extensionData, code: 'unknown', issue: 'embeddedChanged' },
    }
    expect(parse(changed).extensionData.entries).toEqual([])
    expect(() => parse({ ...changed, extensionData: data.extensionData })).toThrow()
  })

  it('validates embedded identity scope without asserting ownership or accepting unsafe paths', () => {
    const data = valid()
    data.embedded = {
      ...data.embedded,
      entries: 20,
      directories: 6,
      plists: 4,
      bundles: [
        {
          relativePath: 'Contents/PlugIns/Share.appex',
          bundleId: 'org.example.share',
          kind: 'appex',
        },
        {
          relativePath: 'Contents/Library/LoginItems/Helper.App',
          bundleId: 'org.vendor.helper',
          kind: 'loginItem',
        },
        {
          relativePath: 'Contents/XPCServices/Service.XPC',
          bundleId: 'org.vendor.service',
          kind: 'xpc',
        },
      ],
    }
    data.extensionData.code = 'notChecked'
    data.extensionData.issue = 'homeUnavailable'
    expect(parse(data).embedded.bundles).toHaveLength(3)
    for (const change of [
      { version: 2 },
      { revision: 'unknown' },
      { code: 'complete' },
      { issue: 'unavailable' },
      { entries: 2049 },
      { entries: -1 },
      { directories: 129 },
      { directories: 0 },
      { plists: 129 },
      { plists: 2 },
      { code: 'notChecked', issue: 'rulesUnavailable' },
      { code: 'partial', issue: null },
      { code: 'partial', issue: 'changed' },
    ])
      expect(() => parse({ ...data, embedded: { ...data.embedded, ...change } })).toThrow()
    for (const bundle of [
      { relativePath: '/Contents/PlugIns/X.appex', bundleId: 'org.example.share', kind: 'appex' },
      { relativePath: 'Contents/../X.appex', bundleId: 'org.example.share', kind: 'appex' },
      { relativePath: 'Contents//X.appex', bundleId: 'org.example.share', kind: 'appex' },
      { relativePath: 'Contents/Other.app', bundleId: 'org.example.share', kind: 'loginItem' },
      { relativePath: 'Contents/PlugIns/X.appex', bundleId: 'org.example.app', kind: 'appex' },
      {
        relativePath: 'Contents/PlugIns/X.appex',
        bundleId: 'org.sparkle-project.helper',
        kind: 'appex',
      },
      { relativePath: 'Contents/PlugIns/X.appex', bundleId: 'bad_id', kind: 'appex' },
      { relativePath: 'Contents/PlugIns/X.appex', bundleId: 'org.example.share', kind: 'xpc' },
      {
        relativePath: 'Contents/PlugIns/X.appex',
        bundleId: 'org.example.share',
        kind: 'constructor',
      },
      {
        relativePath: 'Contents/' + 'a/'.repeat(10) + 'X.appex',
        bundleId: 'org.example.share',
        kind: 'appex',
      },
    ])
      expect(() => parse({ ...data, embedded: { ...data.embedded, bundles: [bundle] } })).toThrow()
    expect(() =>
      parse({
        ...data,
        embedded: {
          ...data.embedded,
          bundles: [data.embedded.bundles[0], data.embedded.bundles[0]],
        },
      }),
    ).toThrow()
    for (const issue of ['unavailable', 'unsupported', 'limitReached']) {
      expect(
        parse({ ...data, embedded: { ...data.embedded, code: 'partial', issue } }).embedded.code,
      ).toBe('partial')
    }
    expect(
      parse({
        ...data,
        embedded: { ...data.embedded, code: 'partial', issue: 'changed', bundles: [] },
      }).embedded.bundles,
    ).toEqual([])
  })

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
      { protocol: 18 },
      { protocol: 16 },
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
      installations: {
        ...valid().installations,
        code: 'unknown',
        issue: 'identityChanged',
        receipts: {
          code: 'notChecked',
          issue: null,
          packageCount: 0,
          checkedCount: 0,
          entries: 0,
          candidates: 0,
        },
        volumes: {
          code: 'notChecked',
          issue: null,
          rootCount: 0,
          entries: 0,
          candidates: 0,
          firstUncertainPath: null,
        },
        roots: {
          code: 'notChecked',
          issue: null,
          rootCount: 0,
          entries: 0,
          candidates: 0,
          firstUncertainPath: null,
        },
      },
      embedded: {
        ...valid().embedded,
        code: 'notChecked',
        issue: 'identityChanged',
        entries: 0,
        directories: 0,
        plists: 0,
      },
      extensionData: { ...valid().extensionData, code: 'notChecked', issue: 'identityChanged' },
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
      installations: {
        ...valid().installations,
        code: 'unknown',
        issue: 'identityUnavailable',
        receipts: {
          code: 'notChecked',
          issue: null,
          packageCount: 0,
          checkedCount: 0,
          entries: 0,
          candidates: 0,
        },
        volumes: {
          code: 'notChecked',
          issue: null,
          rootCount: 0,
          entries: 0,
          candidates: 0,
          firstUncertainPath: null,
        },
        roots: {
          code: 'notChecked',
          issue: null,
          rootCount: 0,
          entries: 0,
          candidates: 0,
          firstUncertainPath: null,
        },
      },
      embedded: {
        ...valid().embedded,
        code: 'notChecked',
        issue: 'identityUnavailable',
        entries: 0,
        directories: 0,
        plists: 0,
      },
      extensionData: { ...valid().extensionData, code: 'notChecked', issue: 'identityUnavailable' },
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
    const locations = associationLocations
    const entries = locations.map(([kind, root, suffix], index) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: index === 0 ? 'present' : index === 1 ? 'unknown' : 'notFound',
      issue: index === 1 ? 'unsupported' : null,
      protectionReasons: [] as string[],
      molePathProtection: pathRule(index === 1 ? 'unknown' : 'notFound'),
    }))
    const associations = {
      version: 4,
      revision: moleUninstallRevision,
      code: 'observed',
      issue: null,
      entries,
      discovery: {
        code: 'observed',
        issue: null,
        rootCount: 4,
        entries: 20,
        candidates: 0,
        firstUncertainRoot: null,
      },
    }
    expect(parse({ ...valid(), associations }).associations.entries[0].status).toBe('present')
    for (const status of ['present', 'notFound', 'unknown']) {
      const entry = {
        ...entries[0],
        status,
        molePathProtection: pathRule(status),
        issue: status === 'unknown' ? 'unavailable' : null,
        protectionReasons: ['clientDirectory'],
      }
      expect(
        parse({
          ...valid(),
          associations: { ...associations, entries: [entry, ...entries.slice(1)] },
        }).associations.entries[0].protectionReasons,
      ).toEqual(['clientDirectory'])
    }

    for (const invalid of [
      undefined,
      {},
      { ...associations, version: 5 },
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
        { protectionReasons: undefined },
        { protectionReasons: ['safe'] },
        { protectionReasons: ['clientDirectory', 'clientDirectory'] },
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
      { ...installations, version: 6 },
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
  it('keeps root-scan scope and partial budgets explicit and rejects contradictions', () => {
    const installations = valid().installations
    const partial = { ...installations, code: 'partial', issue: 'rootsIncomplete' }
    const roots = {
      ...installations.roots,
      code: 'partial',
      issue: 'changed',
      firstUncertainPath: '/Applications',
    }
    expect(
      parse({ ...valid(), installations: { ...partial, roots } }).installations.roots.issue,
    ).toBe('changed')
    for (const invalid of [
      undefined,
      {},
      { ...roots, code: 'complete' },
      { ...roots, rootCount: 10 },
      { ...roots, rootCount: -1 },
      { ...roots, entries: 2049 },
      { ...roots, candidates: 129 },
      { ...roots, candidates: 41 },
      { ...roots, firstUncertainPath: 'relative' },
      { ...roots, firstUncertainPath: '/Apps/../Applications' },
      { ...roots, issue: 'safe' },
      { ...roots, code: 'observed' },
      { ...roots, code: 'notChecked' },
      { ...installations.roots, rootCount: 8 },
    ])
      expect(() => parse({ ...valid(), installations: { ...partial, roots: invalid } })).toThrow(
        '无法识别',
      )
    expect(() => parse({ ...valid(), installations: { ...installations, roots } })).toThrow(
      '无法识别',
    )
    expect(() =>
      parse({ ...valid(), installations: { ...partial, roots: installations.roots } }),
    ).toThrow('无法识别')
  })
  it('keeps volume uncertainty and limits visible and rejects contradictory or unsafe scope', () => {
    const installations = valid().installations
    const volumes = {
      ...installations.volumes,
      code: 'partial',
      issue: 'unsupportedVolume',
      firstUncertainPath: '/Volumes/Share',
    }
    const partial = { ...installations, code: 'partial', issue: 'volumesIncomplete', volumes }
    expect(parse({ ...valid(), installations: partial }).installations.volumes.issue).toBe(
      'unsupportedVolume',
    )
    for (const issue of ['unavailable', 'changed', 'limitReached'])
      expect(
        parse({ ...valid(), installations: { ...partial, volumes: { ...volumes, issue } } })
          .installations.code,
      ).toBe('partial')
    for (const invalid of [
      { ...volumes, rootCount: 65 },
      { ...volumes, rootCount: -1 },
      { ...volumes, entries: 2049 },
      { ...volumes, candidates: 129 },
      { ...volumes, candidates: 11 },
      { ...volumes, code: 'observed' },
      { ...volumes, code: 'notChecked' },
      { ...volumes, issue: 'safe' },
      { ...volumes, firstUncertainPath: '/VolumesFake/Share' },
      { ...volumes, firstUncertainPath: '/Volumes/../Applications' },
      { ...volumes, firstUncertainPath: '/Applications' },
    ])
      expect(() => parse({ ...valid(), installations: { ...partial, volumes: invalid } })).toThrow()
    expect(() => parse({ ...valid(), installations: { ...installations, volumes } })).toThrow()
    expect(() =>
      parse({ ...valid(), installations: { ...partial, volumes: installations.volumes } }),
    ).toThrow()
    const absent = { ...valid(), installations: { ...installations, volumes: undefined } }
    expect(() => parse(absent)).toThrow()
  })
  it('rejects inconsistent receipt scope and preserves incomplete reasons', () => {
    const base = valid().installations
    for (const issue of ['unavailable', 'changed', 'limitReached', 'unsupportedReceipt']) {
      const receipts = { ...base.receipts, code: 'partial', issue }
      expect(
        parse({
          ...valid(),
          installations: { ...base, code: 'partial', issue: 'receiptsIncomplete', receipts },
        }).installations.receipts.issue,
      ).toBe(issue)
    }
    for (const receipts of [
      undefined,
      { ...base.receipts, packageCount: 513 },
      { ...base.receipts, checkedCount: 65 },
      { ...base.receipts, checkedCount: 21 },
      { ...base.receipts, entries: 32769 },
      { ...base.receipts, candidates: 129 },
      { ...base.receipts, candidates: 41 },
      { ...base.receipts, checkedCount: 0 },
      { ...base.receipts, code: 'notChecked' },
      { ...base.receipts, issue: 'changed' },
      { ...base.receipts, code: 'partial', issue: 'safe' },
      { ...base.receipts, entries: -1 },
      { ...base.receipts, checkedCount: 0.5 },
    ])
      expect(() => parse({ ...valid(), installations: { ...base, receipts } })).toThrow()
    expect(() =>
      parse({
        ...valid(),
        installations: { ...base, code: 'partial', issue: 'receiptsIncomplete' },
      }),
    ).toThrow()
  })
  it('bounds group and derived discovery without accepting arbitrary roots, loose matches or complete partial scans', () => {
    const fixed = associationLocations.map(([kind, root, suffix]) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: 'notFound',
      issue: null,
      protectionReasons: [] as string[],
      molePathProtection: pathRule(),
    }))
    const entry = {
      kind: 'groupContainer',
      relativePath: 'Library/Group Containers/TEAM.org.example.app.shared',
      status: 'present',
      issue: null,
      protectionReasons: ['clientDirectory'],
      molePathProtection: pathRule(),
    }
    const discovery = {
      code: 'observed',
      issue: null,
      rootCount: 4,
      entries: 40,
      candidates: 1,
      firstUncertainRoot: null,
    }
    const associations = {
      version: 4,
      revision: moleUninstallRevision,
      code: 'observed',
      issue: null,
      entries: [...fixed, entry],
      discovery,
    }
    expect(parse({ ...valid(), associations }).associations.entries).toHaveLength(17)
    for (const issue of ['unavailable', 'unsupported', 'changed', 'limitReached'])
      expect(
        parse({
          ...valid(),
          associations: {
            ...associations,
            discovery: {
              ...discovery,
              code: 'partial',
              issue,
              firstUncertainRoot: 'Library/Group Containers',
            },
          },
        }).associations.discovery.issue,
      ).toBe(issue)
    for (const invalid of [
      { ...discovery, rootCount: 3 },
      { ...discovery, rootCount: 5 },
      { ...discovery, entries: 1025 },
      { ...discovery, entries: -1 },
      { ...discovery, candidates: 2 },
      { ...discovery, candidates: 65 },
      { ...discovery, code: 'notChecked' },
      { ...discovery, issue: 'changed' },
      { ...discovery, code: 'partial', issue: 'changed', firstUncertainRoot: '/etc' },
      { ...discovery, code: 'partial', issue: 'safe', firstUncertainRoot: 'Library/Containers' },
    ])
      expect(() =>
        parse({ ...valid(), associations: { ...associations, discovery: invalid } }),
      ).toThrow()
    for (const invalid of [
      { ...entry, relativePath: 'Library/Group Containers/TEAM.org.example.apple' },
      { ...entry, relativePath: 'Library/Group Containers/TEAM.Org.example.app' },
      { ...entry, relativePath: 'Library/Group Containers/Xorg.example.app.shared' },
      { ...entry, relativePath: 'Library/Group Containers/TEAM.org.example.app/child' },
      { ...entry, relativePath: 'Library/Group Containers/../org.example.app' },
      { ...entry, relativePath: 'Library/Group Containers/TEAM.org.example.app\u0085' },
      {
        ...entry,
        kind: 'constructor',
        relativePath: `${String(Object)}/TEAM.org.example.app.shared`,
      },
      { ...entry, kind: 'toString' },
      { ...entry, kind: 'derivedScripts' },
      { ...entry, kind: 'owned' },
      { ...entry, kind: 'derivedScripts', relativePath: fixed[11].relativePath },
    ])
      expect(() =>
        parse({ ...valid(), associations: { ...associations, entries: [...fixed, invalid] } }),
      ).toThrow()
  })
})
