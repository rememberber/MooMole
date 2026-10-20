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
const xdgBase = (name: string) =>
  [
    ['nameXdgConfig', `.config/${name}`],
    ['nameXdgCache', `.cache/${name}`],
    ['nameXdgLowerCache', `.cache/${name.replace(/[A-Z]/g, (c) => c.toLowerCase())}`],
    ['nameXdgShare', `.local/share/${name}`],
  ].filter(([, path], index, all) => all.findIndex(([, previous]) => path === previous) === index)
const valid = () => ({
  protocol: 26,
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
    version: 3,
    groups: {
      version: 1,
      source: 'clientEmbeddedIdNameBoundary',
      code: 'notChecked',
      issue: null as string | null,
      entries: 0,
      candidates: 0,
    },
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
    version: 12,
    names: {
      version: 7,
      source: 'moleAppNameUserPaths',
      baseName: null,
      leafNames: [] as string[],
      vscodeChannel: null,
      cliExcluded: 0,
      code: 'notChecked',
      issue: 'nameUnavailable',
      name: null,
      candidates: 0,
    },
    byHost: { code: 'notChecked', issue: null, entries: 0, candidates: 0 },
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
  ['preferencesDirectory', 'Library/Preferences', ''],
  ['downloadCache', 'Library/Caches/com.apple.nsurlsessiond/Downloads', ''],
  [
    'recentDocuments',
    'Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments',
    '.sfl2',
  ],
  [
    'recentDocuments',
    'Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments',
    '.sfl3',
  ],
  [
    'recentDocuments',
    'Library/Application Support/com.apple.sharedfilelist/com.apple.LSSharedFileList.ApplicationRecentDocuments',
    '.sfl4',
  ],
]

const parse = (value: unknown, expected = 'org.example.app', expectedName = 'Example') =>
  parseApplicationInspection(value, expected, expectedName)
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
        version: 12,
        code: 'observed',
        issue: null,
        byHost: { code: 'observed', issue: null, entries: 0, candidates: 0 },
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
      groups: { ...data.extensionData.groups, code: 'observed' },
      byHost: { code: 'observed', issue: null, entries: 20, candidates: 1 },
    }
    expect(parse(data).extensionData.entries).toHaveLength(16)
    const invalids = [
      { ...data.extensionData, version: 4 },
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
  it('bounds client embedded-ID group candidates and requires all matching sources', () => {
    const data = valid()
    const ids = ['org.other.helper', 'org.other.helper.share']
    data.embedded = {
      ...data.embedded,
      entries: 10,
      directories: 5,
      plists: 2,
      bundles: ids.map((bundleId, index) => ({
        bundleId,
        relativePath: `Contents/PlugIns/Helper${index}.appex`,
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
    const fixed = ids.flatMap((bundleId) =>
      locations.map(([kind, root, suffix]) => ({
        relativePath: `${root}/${bundleId}${suffix}`,
        sources: [{ bundleId, kind }],
        status: 'notFound',
        issue: null,
        protectionReasons: [],
        molePathProtection: pathRule(),
      })),
    )
    const group = {
      relativePath: 'Library/Group Containers/TEAM.org.other.helper.share',
      sources: ids.map((bundleId) => ({ bundleId, kind: 'groupContainer' })),
      status: 'present',
      issue: null,
      protectionReasons: ['clientDirectory'],
      molePathProtection: pathRule(),
    }
    const groups = { ...data.extensionData.groups, code: 'observed', entries: 10, candidates: 1 }
    const extensionData = {
      ...data.extensionData,
      identifiers: ids,
      entries: [...fixed, group],
      byHost: { code: 'observed', issue: null, entries: 0, candidates: 0 },
      groups,
    }
    expect(parse({ ...data, extensionData }).extensionData.entries.at(-1)?.sources).toHaveLength(2)
    for (const issue of ['unavailable', 'unsupported', 'changed', 'limitReached'])
      expect(
        parse({
          ...data,
          extensionData: {
            ...extensionData,
            code: 'partial',
            issue: 'groupsIncomplete',
            groups: { ...groups, code: 'partial', issue },
          },
        }).extensionData.groups.issue,
      ).toBe(issue)
    for (const invalid of [
      { ...extensionData, groups: undefined },
      { ...extensionData, groups: { ...groups, version: 2 } },
      { ...extensionData, groups: { ...groups, source: 'moleAuthenticatedGroups' } },
      { ...extensionData, groups: { ...groups, candidates: 0 } },
      { ...extensionData, groups: { ...groups, entries: 1025 } },
      { ...extensionData, groups: { ...groups, candidates: 65 } },
      { ...extensionData, groups: { ...groups, code: 'notChecked' } },
      { ...extensionData, groups: { ...groups, code: 'partial', issue: 'changed' } },
      { ...extensionData, code: 'partial', issue: 'groupsIncomplete' },
      ...[
        { sources: [group.sources[0]] },
        { sources: [...group.sources, group.sources[0]] },
        { relativePath: 'Library/Group Containers/TEAM.org.other.helperapple' },
        { relativePath: 'Library/Group Containers/TEAM.ORG.other.helper.share' },
        { relativePath: 'Library/Group Containers/TEAM.org.other.helper.share/child' },
        { sources: ids.map((bundleId) => ({ bundleId, kind: 'byHost' })) },
      ].map((change) => ({ ...extensionData, entries: [...fixed, { ...group, ...change }] })),
    ])
      expect(() => parse({ ...data, extensionData: invalid })).toThrow()
  })
  it('binds six literal name hints to the original list name and preserves conservative gaps', () => {
    const data = valid()
    const fixed = associationLocations.map(([kind, root, suffix]) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: 'notFound',
      issue: null,
      protectionReasons: [],
      molePathProtection: pathRule(),
    }))
    const templates = [
      ['nameSupport', 'Library/Application Support', ''],
      ['nameCache', 'Library/Caches', ''],
      ['nameLogs', 'Library/Logs', ''],
      ['namePreferencesDirectory', 'Library/Preferences', ''],
      ['namePreferences', 'Library/Preferences', '.plist'],
      ['nameSavedState', 'Library/Saved Application State', '.savedState'],
      ['nameService', 'Library/Services', '.workflow'],
      ['nameQuickLook', 'Library/QuickLook', '.qlgenerator'],
      ['nameInternetPlugin', 'Library/Internet Plug-Ins', '.plugin'],
      ['nameAudioComponent', 'Library/Audio/Plug-Ins/Components', '.component'],
      ['nameVst', 'Library/Audio/Plug-Ins/VST', '.vst'],
      ['nameVst3', 'Library/Audio/Plug-Ins/VST3', '.vst3'],
      ['nameDigidesign', 'Library/Audio/Plug-Ins/Digidesign', '.dpm'],
      ['namePreferencePane', 'Library/PreferencePanes', '.prefPane'],
      ['nameInputMethod', 'Library/Input Methods', '.app'],
      ['nameScreenSaver', 'Library/Screen Savers', '.saver'],
      ['nameFramework', 'Library/Frameworks', '.framework'],
      ['nameContextPlugin', 'Library/Contextual Menu Items', '.plugin'],
      ['nameSpotlight', 'Library/Spotlight', '.mdimporter'],
      ['nameColorPicker', 'Library/ColorPickers', '.colorPicker'],
      ['nameWorkflow', 'Library/Workflows', '.workflow'],
      ['nameAddressBookPlugin', 'Library/Address Book Plug-Ins', '.bundle'],
      ['nameAccessibility', 'Library/Accessibility', '.bundle'],
      ['nameMailPlugin', 'Library/Mail/Bundles', '.mailbundle'],
    ]
    const entries = [
      ...templates.map(([kind, root, suffix]) => [kind, `${root}/Example${suffix}`]),
      ...xdgBase('Example'),
    ].map(([kind, relativePath], index) => ({
      kind,
      relativePath,
      status: index === 0 ? 'present' : index === 1 ? 'unknown' : 'notFound',
      issue: index === 1 ? 'unavailable' : null,
      protectionReasons: index === 3 ? ['clientDirectory'] : [],
      molePathProtection: pathRule(index === 1 ? 'unknown' : 'notFound'),
    }))
    const names = {
      version: 7,
      source: 'moleAppNameUserPaths',
      baseName: null,
      leafNames: [] as string[],
      vscodeChannel: null,
      cliExcluded: 0,
      code: 'observed',
      name: 'Example',
      issue: null,
      candidates: 28,
    }
    const associations = {
      ...data.associations,
      code: 'observed',
      issue: null,
      names,
      entries: [...fixed, ...entries],
      byHost: { code: 'observed', issue: null, entries: 0, candidates: 0 },
      discovery: {
        code: 'observed',
        issue: null,
        rootCount: 4,
        entries: 0,
        candidates: 0,
        firstUncertainRoot: null,
      },
    }
    expect(parse({ ...data, associations }).associations.names.name).toBe('Example')
    for (const invalid of [
      { ...associations, names: undefined },
      { ...associations, names: { ...names, version: 8 } },
      { ...associations, names: { ...names, version: 5 } },
      { ...associations, names: { ...names, version: 4 } },
      { ...associations, names: { ...names, source: 'moleFullUninstall' } },
      { ...associations, names: { ...names, source: 'moleBaseAppNameLibrary' } },
      { ...associations, names: { ...names, source: 'moleLiteralAppNameLibrary' } },
      { ...associations, names: { ...names, source: 'moleAppNameLibrary' } },
      {
        ...associations,
        entries: [
          ...fixed,
          ...entries.slice(0, -1),
          { ...entries.at(-1), relativePath: 'Library/Mail/Bundles/../Example.mailbundle' },
        ],
      },
      { ...associations, names: { ...names, name: 'Another' } },
      { ...associations, names: { ...names, candidates: 27 } },
      { ...associations, names: { ...names, issue: 'nameUnsupported' } },
      { ...associations, entries: [...fixed, ...entries.slice(1)] },
      { ...associations, entries: [...fixed, ...entries, { ...entries[0] }] },
      { ...associations, discovery: { ...associations.discovery, candidates: 6 } },
      ...['../escape', 'App/Child', 'x', '中'.repeat(81)].map((name) => ({
        ...associations,
        names: { ...names, name },
      })),
    ])
      expect(() => parse({ ...data, associations: invalid })).toThrow()
    expect(() => parseApplicationInspection({ ...data, associations }, 'org.example.app')).toThrow()
    const duplicate = {
      ...associations,
      names: { ...names, name: 'org.example.app', candidates: 20 },
      entries: [
        ...fixed,
        ...templates
          .filter(
            ([, root, suffix]) =>
              !associationLocations.some(
                ([, base, end]) =>
                  `${root}/org.example.app${suffix}` === `${base}/org.example.app${end}`,
              ),
          )
          .map(([kind, root, suffix]) => ({
            ...entries[0],
            kind,
            relativePath: `${root}/org.example.app${suffix}`,
            status: 'notFound',
          })),
        ...xdgBase('org.example.app').map(([kind, relativePath]) => ({
          ...entries[0],
          kind,
          relativePath,
          status: 'notFound',
        })),
      ],
    }
    expect(
      parse({ ...data, associations: duplicate }, 'org.example.app', 'org.example.app').associations
        .names.candidates,
    ).toBe(20)
    // Independent expected paths cover C byte length, literal separators, deny roots and ID precedence.
    const compound = (name: string) => {
      const compact = name.replaceAll(' ', '')
      const underscore = name.replaceAll(' ', '_')
      const hyphen = name.replaceAll(' ', '-')
      const extras =
        new TextEncoder().encode(name).length > 3 && name.includes(' ')
          ? [
              ['nameCompactSupport', `Library/Application Support/${compact}`],
              ['nameCompactCache', `Library/Caches/${compact}`],
              ['nameCompactLogs', `Library/Logs/${compact}`],
              ['nameCompactPreferencesDirectory', `Library/Preferences/${compact}`],
              ['nameCompactPreferences', `Library/Preferences/${compact}.plist`],
              ['nameCompactSavedState', `Library/Saved Application State/${compact}.savedState`],
              ['nameUnderscoreSupport', `Library/Application Support/${underscore}`],
              ['nameHyphenSupport', `Library/Application Support/${hyphen}`],
              ['nameUnderscorePreferencesDirectory', `Library/Preferences/${underscore}`],
              ['nameUnderscorePreferences', `Library/Preferences/${underscore}.plist`],
              ['nameHyphenPreferencesDirectory', `Library/Preferences/${hyphen}`],
              ['nameHyphenPreferences', `Library/Preferences/${hyphen}.plist`],
            ]
          : []
      const all = [
        ...templates.map(([kind, root, suffix]) => [kind, `${root}/${name}${suffix}`]),
        ...extras,
        ...xdgBase(name),
        ...(new TextEncoder().encode(name).length > 3 && name.includes(' ')
          ? [
              ...['Config', 'Cache', 'Share'].flatMap((kind, index) => {
                const root = ['.config', '.cache', '.local/share'][index]
                return [
                  ['Compact', compact],
                  ['Hyphen', hyphen],
                  ['Underscore', underscore],
                ].map(([variant, value]) => [
                  `nameXdg${variant}${kind}`,
                  `${root}/${value.replace(/[A-Z]/g, (c) => c.toLowerCase())}`,
                ])
              }),
            ]
          : []),
      ]
        .filter(
          ([, path]) =>
            !['Library/Preferences/ByHost', 'Library/Application Support/Epic'].includes(path) &&
            !fixed.some((entry) => entry.relativePath === path),
        )
        .map(([kind, relativePath], index) => ({
          ...entries[0],
          kind,
          relativePath,
          status: index === 24 ? 'unknown' : 'present',
          issue: index === 24 ? 'unsupported' : null,
          protectionReasons: index === 30 ? ['clientDirectory'] : [],
          molePathProtection: pathRule(index === 24 ? 'unknown' : 'present'),
        }))
      return {
        ...associations,
        names: { ...names, name, candidates: all.length },
        entries: [...fixed, ...all],
      }
    }
    for (const [name, count] of [
      ['Suite Studio', 49],
      ['Suite  Studio', 49],
      ['A B', 28],
      ['中 X', 49],
      ['Suite\u00a0Studio', 28],
      ['By Host', 48],
      ['Epic', 27],
      ['Ep ic', 48],
      ['org.example. app', 42],
    ] as const) {
      const result = compound(name)
      expect(
        parse({ ...data, associations: result }, 'org.example.app', name).associations.names
          .candidates,
      ).toBe(count)
    }
    const space = compound('Suite Studio')
    for (const invalid of [
      { ...space, entries: space.entries.slice(0, -1) },
      { ...space, names: { ...space.names, candidates: 48 }, entries: space.entries.slice(0, -1) },
      { ...space, entries: [...space.entries, { ...space.entries.at(-1) }] },
      ...[
        { kind: 'nameSupport' },
        { relativePath: 'Library/Application Support/suitestudio' },
        { relativePath: 'Library/Application Support/Suite--Studio' },
        { relativePath: 'Library/Application Support/..' },
        { relativePath: '.config/suitestudio' },
      ].map((change) => ({
        ...space,
        entries: space.entries.map((entry, index) =>
          index === 45 ? { ...entry, ...change } : entry,
        ),
      })),
    ])
      expect(() =>
        parse({ ...data, associations: invalid }, 'org.example.app', 'Suite Studio'),
      ).toThrow()
    for (const name of ['    ', ' .  ', ' .. ', 'A\tB', 'A\nB'])
      expect(() =>
        parse({ ...data, associations: compound(name) }, 'org.example.app', name),
      ).toThrow()
    const cliCase = (name: string) => {
      const value = compound(name)
      const rejected = value.entries.filter((entry) =>
        /^\.(?:config|cache)\/\.?(?:claude|codex|gemini|opencode)$|^\.local\/share\/\.?(?:claude|codex|gemini|opencode)$/i.test(
          entry.relativePath,
        ),
      )
      return {
        ...value,
        names: {
          ...value.names,
          candidates: value.names.candidates - rejected.length,
          cliExcluded: rejected.length,
        },
        entries: value.entries.filter((entry) => !rejected.includes(entry)),
      }
    }
    for (const [name, count, skipped] of [
      ['Claude', 24, 4],
      ['claude', 24, 3],
      ['.CoDeX', 24, 4],
      ['..codex', 27, 0],
      ['Co dex', 46, 3],
      ['OpenCode', 24, 4],
      ['codex-beta', 27, 0],
      ['ÜBER', 28, 0],
    ] as const) {
      const result = parse({ ...data, associations: cliCase(name) }, 'org.example.app', name)
      expect(result.associations.names.candidates).toBe(count)
      expect(result.associations.names.cliExcluded).toBe(skipped)
    }
    const cli = cliCase('Claude')
    for (const invalid of [
      { ...cli, names: { ...cli.names, cliExcluded: 0 } },
      { ...cli, names: { ...cli.names, cliExcluded: undefined } },
      { ...cli, names: { ...cli.names, cliExcluded: 5 } },
      {
        ...cli,
        names: { ...cli.names, candidates: 25 },
        entries: [
          ...cli.entries,
          { ...entries[0], kind: 'nameXdgConfig', relativePath: '.config/Claude' },
        ],
      },
    ])
      expect(() => parse({ ...data, associations: invalid }, 'org.example.app', 'Claude')).toThrow()
    for (const [name, count] of [
      ['-n', 27],
      ['-e', 27],
      ['-E', 27],
      ['-neE', 27],
      ['-NE', 28],
    ] as const) {
      const value = compound(name)
      const rows = value.entries.filter(
        (entry) => !/^-[neE]+$/.test(name) || entry.kind !== 'nameXdgLowerCache',
      )
      const actual = {
        ...value,
        entries: rows,
        names: { ...value.names, candidates: rows.length - fixed.length },
      }
      expect(
        parse({ ...data, associations: actual }, 'org.example.app', name).associations.names
          .candidates,
      ).toBe(count)
    }
    // Shared XDG roots and HOME dotdirs cannot be smuggled into the complete name set.
    for (const path of [
      '.config',
      '.cache/',
      '.local/share',
      '.codex',
      '.config/../.codex',
      '.config/SUİTEStudio',
    ]) {
      const value = compound('Suite Studio')
      value.entries[value.entries.length - 1].relativePath = path
      expect(() =>
        parse({ ...data, associations: value }, 'org.example.app', 'Suite Studio'),
      ).toThrow()
    }
    const channelCase = (name: string, base: string | null) => {
      const value = compound(name)
      const lower =
        base !== null && /^-[neE]+$/.test(base)
          ? ''
          : base?.replace(/[A-Z]/g, (c) => c.toLowerCase())
      const paths =
        base === null
          ? []
          : [
              ['nameChannelSupport', `Library/Application Support/${base}`],
              ['nameChannelCache', `Library/Caches/${base}`],
              ['nameChannelLogs', `Library/Logs/${base}`],
              ['nameChannelPreferencesDirectory', `Library/Preferences/${base}`],
              ['nameChannelPreferences', `Library/Preferences/${base}.plist`],
              ['nameChannelSavedState', `Library/Saved Application State/${base}.savedState`],
              ['nameChannelXdgConfig', `.config/${lower}`],
              ['nameChannelXdgCache', `.cache/${lower}`],
              ['nameChannelXdgShare', `.local/share/${lower}`],
            ]
      const rejected = paths.filter(([, path]) =>
        /^\.(?:config|cache)\/\.?(?:claude|codex|gemini|opencode)$|^\.local\/share\/\.?(?:claude|codex|gemini|opencode)$/i.test(
          path,
        ),
      )
      const added = paths
        .filter((path) => !rejected.includes(path))
        .filter(
          ([, path]) =>
            !path.endsWith('/') &&
            !['Library/Preferences/ByHost', 'Library/Application Support/Epic'].includes(path) &&
            !value.entries.some((entry) => entry.relativePath === path),
        )
        .map(([kind, relativePath]) => ({
          ...entries[0],
          kind,
          relativePath,
          status: 'present',
          issue: null,
        }))
      return {
        ...value,
        names: {
          ...value.names,
          baseName: base,
          cliExcluded: rejected.length,
          candidates: value.names.candidates + added.length,
        },
        entries: [...value.entries, ...added],
      }
    }
    for (const [name, base, count, excluded] of [
      ['Zed Nightly', 'Zed', 58, 0],
      ['Zed  Nightly', 'Zed ', 58, 0],
      ['Zed Beta Nightly', 'Zed Beta', 58, 0],
      ['Safari Technology Preview', 'Safari Technology', 58, 0],
      ['Firefox Developer Edition', 'Firefox', 58, 0],
      ['中 Beta', '中', 58, 0],
      ['AB Beta', null, 49, 0],
      ['App beta', null, 49, 0],
      ['App Beta ', null, 49, 0],
      ['App\u00a0Beta', null, 28, 0],
      ['Codex Nightly', 'Codex', 55, 3],
      ['Epic Beta', 'Epic', 57, 0],
      ['ByHost Beta', 'ByHost', 57, 0],
      ['org.example.app Nightly', 'org.example.app', 52, 0],
      ['-eE Beta', '-eE', 55, 0],
    ] as const) {
      const result = parse(
        { ...data, associations: channelCase(name, base) },
        'org.example.app',
        name,
      )
      expect(result.associations.names.baseName).toBe(base)
      expect(result.associations.names.candidates).toBe(count)
      expect(result.associations.names.cliExcluded).toBe(excluded)
    }
    const leafCase = (name: string, id: string, variants: string[]) => {
      const value = compound(name)
      const fixedRows = value.entries.map((entry) => ({
        ...entry,
        relativePath: entry.relativePath.replace('org.example.app', id),
      }))
      const added = variants
        .flatMap((variant) => [
          ['nameLeafSupport', `Library/Application Support/${variant}`],
          ['nameLeafCache', `Library/Caches/${variant}`],
          ['nameLeafLogs', `Library/Logs/${variant}`],
          ['nameLeafPreferences', `Library/Preferences/${variant}.plist`],
          ['nameLeafSavedState', `Library/Saved Application State/${variant}.savedState`],
        ])
        .filter(([, path]) => !fixedRows.some((e) => e.relativePath === path))
        .map(([kind, relativePath]) => ({
          ...entries[0],
          kind,
          relativePath,
          status: 'present',
          issue: null,
        }))
      return {
        ...data,
        bundleId: id,
        associations: {
          ...value,
          names: {
            ...value.names,
            leafNames: variants,
            candidates: value.names.candidates + added.length,
          },
          entries: [...fixedRows, ...added],
        },
      }
    }
    for (const [name, id, variants] of [
      ['AyuGram', 'one.ayugram.AyuGramDesktop', ['AyuGramDesktop', 'AyuGram Desktop']],
      ['Ayu Gram', 'one.ayugram.AyuGramDesktop', ['AyuGramDesktop', 'Ayu Gram Desktop']],
      ['ayugram', 'org.test.AyuGramHTTPServer', ['AyuGramHTTPServer', 'ayugram HTTP Server']],
      ['AyuGram', 'org.test.AyuGram2FAThing', ['AyuGram2FAThing', 'AyuGram 2 FA Thing']],
      ['Wrapper', 'com.wrapper.GoogleChrome', []],
      ['64Gram', 'org.fork.TelegramDesktop', []],
      ['AyuGram', 'org.test.AyuGramdesktop', []],
      ['AyuGramDesktop', 'org.test.AyuGramDesktop', []],
      ['AB', 'org.test.ABDesktop', []],
      ['中', 'org.test.AppDesktop', []],
    ] as [string, string, string[]][]) {
      const fixture = leafCase(name, id, variants)
      expect(parse(fixture, id, name).associations.names.leafNames).toEqual(variants)
    }
    const leafId = 'one.ayugram.AyuGramDesktop'
    const leafFixture = leafCase('AyuGram', leafId, ['AyuGramDesktop', 'AyuGram Desktop'])
    for (const names of [
      { ...leafFixture.associations.names, leafNames: undefined },
      { ...leafFixture.associations.names, leafNames: [] },
      { ...leafFixture.associations.names, leafNames: ['ayugramdesktop', 'AyuGram Desktop'] },
      { ...leafFixture.associations.names, leafNames: ['AyuGram Desktop', 'AyuGramDesktop'] },
      { ...leafFixture.associations.names, code: 'unknown', issue: 'limitReached', name: null },
    ])
      expect(() =>
        parse(
          { ...leafFixture, associations: { ...leafFixture.associations, names } },
          leafId,
          'AyuGram',
        ),
      ).toThrow()
    for (const change of [
      { entries: leafFixture.associations.entries.slice(0, -1) },
      {
        entries: leafFixture.associations.entries.map((e) =>
          e.kind === 'nameLeafSupport'
            ? { ...e, relativePath: 'Library/Application Support/Google Chrome' }
            : e,
        ),
      },
    ])
      expect(() =>
        parse(
          { ...leafFixture, associations: { ...leafFixture.associations, ...change } },
          leafId,
          'AyuGram',
        ),
      ).toThrow()
    const vscodeCase = (name: string, id: string, channel: string | null) => {
      const fixture = leafCase(name, id, [])
      const entries = fixture.associations.entries
      const extra =
        channel === null
          ? []
          : [
              ['nameVSCodeShipItStable', 'Library/Caches/com.microsoft.VSCode.ShipIt'],
              ['nameVSCodeShipItInsiders', 'Library/Caches/com.microsoft.VSCodeInsiders.ShipIt'],
              ['nameVSCodeExtensions', channel === 'insiders' ? '.vscode-insiders' : '.vscode'],
              [
                'nameVSCodeSupport',
                channel === 'insiders'
                  ? 'Library/Application Support/Code - Insiders'
                  : 'Library/Application Support/Code',
              ],
              [
                'nameVSCodeCache',
                channel === 'insiders'
                  ? 'Library/Caches/com.microsoft.VSCodeInsiders'
                  : 'Library/Caches/com.microsoft.VSCode',
              ],
            ]
      const added = extra
        .filter(([, path]) => !entries.some((e) => e.relativePath === path))
        .map(([kind, relativePath]) => ({
          ...entries[0],
          kind,
          relativePath,
          status: 'present',
          issue: null,
        }))
      return {
        ...fixture,
        associations: {
          ...fixture.associations,
          names: {
            ...fixture.associations.names,
            vscodeChannel: channel,
            candidates: fixture.associations.names.candidates + added.length,
          },
          entries: [...entries, ...added],
        },
      }
    }
    for (const [name, id, channel] of [
      ['Code', 'com.microsoft.VSCode', 'stable'],
      ['Visual Studio Code', 'com.microsoft.VSCode', 'stable'],
      ['Visual Studio Code Insiders', 'com.microsoft.VSCodeInsiders', 'insiders'],
      ['Code', 'org.notmicrosoft.foo.vScodeSomething', 'stable'],
      ['Code', 'com.microsoft.VSCodeINSIDERS', 'stable'],
      ['Code', 'com.Microsoft.VSCode', null],
      ['Code', 'com.microsoft.VSCODE', null],
      ['Code', 'com.cursor.editor', null],
    ] as [string, string, string | null][]) {
      const fixture = vscodeCase(name, id, channel)
      expect(parse(fixture, id, name).associations.names.vscodeChannel).toBe(channel)
      if (id === 'com.microsoft.VSCode' && name === 'Code')
        expect(fixture.associations.names.candidates).toBe(31)
    }
    const codeId = 'com.microsoft.VSCode'
    const codeFixture = vscodeCase('Code', codeId, 'stable')
    for (const names of [
      { ...codeFixture.associations.names, vscodeChannel: undefined },
      { ...codeFixture.associations.names, vscodeChannel: null },
      { ...codeFixture.associations.names, vscodeChannel: 'insiders' },
      { ...codeFixture.associations.names, vscodeChannel: 'Stable' },
      { ...codeFixture.associations.names, version: 6 },
    ])
      expect(() =>
        parse(
          { ...codeFixture, associations: { ...codeFixture.associations, names } },
          codeId,
          'Code',
        ),
      ).toThrow()
    for (const entries of [
      codeFixture.associations.entries.slice(0, -1),
      codeFixture.associations.entries.map((e) =>
        e.kind === 'nameVSCodeExtensions' ? { ...e, relativePath: '.vscode-insiders' } : e,
      ),
      codeFixture.associations.entries.map((e) =>
        e.kind === 'nameVSCodeShipItStable' ? { ...e, relativePath: '.docker' } : e,
      ),
    ])
      expect(() =>
        parse(
          { ...codeFixture, associations: { ...codeFixture.associations, entries } },
          codeId,
          'Code',
        ),
      ).toThrow()
    const channel = channelCase('Zed Nightly', 'Zed')
    for (const invalid of [
      { ...channel, names: { ...channel.names, baseName: null } },
      { ...channel, names: { ...channel.names, baseName: 'zed' } },
      { ...channel, names: { ...channel.names, baseName: 'Another' } },
      { ...channel, names: { ...channel.names, baseName: undefined } },
      { ...channel, entries: channel.entries.slice(0, -1) },
      {
        ...channel,
        names: { ...channel.names, candidates: 57 },
        entries: channel.entries.slice(0, -1),
      },
      {
        ...channel,
        entries: channel.entries.map((entry, index) =>
          index === channel.entries.length - 1
            ? { ...entry, relativePath: '.local/share/Zed' }
            : entry,
        ),
      },
    ])
      expect(() =>
        parse({ ...data, associations: invalid }, 'org.example.app', 'Zed Nightly'),
      ).toThrow()
    for (const [code, issue] of [
      ['notChecked', 'nameUnavailable'],
      ['notChecked', 'nameUnsupported'],
      ['unknown', 'limitReached'],
    ])
      expect(
        parse({
          ...data,
          associations: {
            ...associations,
            names: { ...names, code, issue, name: null, candidates: 0 },
            entries: fixed,
          },
        }).associations.names.code,
      ).toBe(code)
  })
  it('rejects future protocols, malformed time, unknown enums and unsupported identity claims', () => {
    for (const change of [
      { protocol: 27 },
      { protocol: 25 },
      { protocol: 24 },
      { protocol: 23 },
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
      version: 12,
      names: {
        version: 7,
        source: 'moleAppNameUserPaths',
        baseName: null,
        leafNames: [] as string[],
        vscodeChannel: null,
        cliExcluded: 0,
        code: 'notChecked',
        issue: 'nameUnavailable',
        name: null,
        candidates: 0,
      },
      byHost: { code: 'observed', issue: null, entries: 0, candidates: 0 },
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
      { ...associations, version: 13 },
      { ...associations, version: 11 },
      { ...associations, version: 10 },
      { ...associations, version: 9 },
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
  it('bounds main ByHost independently and keeps settings, recent records and protections explicit', () => {
    const fixed = associationLocations.map(([kind, root, suffix]) => ({
      kind,
      relativePath: `${root}/org.example.app${suffix}`,
      status: 'notFound',
      issue: null,
      protectionReasons: [],
      molePathProtection: pathRule(),
    }))
    const entry = {
      kind: 'byHost',
      relativePath: 'Library/Preferences/ByHost/org.example.app.plist',
      status: 'present',
      issue: null,
      protectionReasons: ['clientDirectory'],
      molePathProtection: pathRule(),
    }
    const associations = {
      ...valid().associations,
      code: 'observed',
      issue: null,
      entries: [...fixed, entry],
      discovery: {
        code: 'observed',
        issue: null,
        rootCount: 4,
        entries: 0,
        candidates: 0,
        firstUncertainRoot: null,
      },
      byHost: { code: 'observed', issue: null, entries: 7, candidates: 1 },
    }
    expect(parse({ ...valid(), associations }).associations.entries).toHaveLength(22)
    for (const issue of ['unavailable', 'unsupported', 'changed', 'limitReached'])
      expect(
        parse({
          ...valid(),
          associations: {
            ...associations,
            byHost: { ...associations.byHost, code: 'partial', issue },
          },
        }).associations.byHost.issue,
      ).toBe(issue)
    for (const invalid of [
      { ...associations, byHost: undefined },
      { ...associations, byHost: { ...associations.byHost, candidates: 0 } },
      { ...associations, byHost: { ...associations.byHost, candidates: 65 } },
      { ...associations, byHost: { ...associations.byHost, entries: 1025 } },
      { ...associations, byHost: { ...associations.byHost, code: 'notChecked' } },
      { ...associations, byHost: { ...associations.byHost, issue: 'changed' } },
      ...[
        'Library/Preferences/ByHost/org.example.apple.A.plist',
        'Library/Preferences/ByHost/TEAM.org.example.app.A.plist',
        'Library/Preferences/ByHost/org.example.app.A.PLIST',
        'Library/Preferences/ByHost/org.example.app.A/child.plist',
      ].map((relativePath) => ({
        ...associations,
        entries: [...fixed, { ...entry, relativePath }],
      })),
      { ...associations, discovery: { ...associations.discovery, candidates: 1 } },
    ])
      expect(() => parse({ ...valid(), associations: invalid })).toThrow()
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
      version: 12,
      names: {
        version: 7,
        source: 'moleAppNameUserPaths',
        baseName: null,
        leafNames: [] as string[],
        vscodeChannel: null,
        cliExcluded: 0,
        code: 'notChecked',
        issue: 'nameUnavailable',
        name: null,
        candidates: 0,
      },
      byHost: { code: 'observed', issue: null, entries: 0, candidates: 0 },
      revision: moleUninstallRevision,
      code: 'observed',
      issue: null,
      entries: [...fixed, entry],
      discovery,
    }
    expect(parse({ ...valid(), associations }).associations.entries).toHaveLength(22)
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
