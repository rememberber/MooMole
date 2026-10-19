import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'

async function setup(page: Page) {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const previous = internal.invoke
    internal.invoke = async (command, args) => {
      if (command === 'plugin:dialog|open') return '/Applications'
      if (command === 'save_settings') {
        sessionStorage.setItem('inspection-settings', JSON.stringify(args?.settings))
        return args?.settings
      }
      if (command !== 'inspect_application') return previous(command, args)
      const calls = JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]')
      calls.push(args)
      sessionStorage.setItem('inspection-calls', JSON.stringify(calls))
      const mode = sessionStorage.getItem('inspection-mode')
      const saved = JSON.parse(sessionStorage.getItem('inspection-settings') ?? 'null')
      const protection = {
        reasons:
          saved?.protectedPaths.includes('/Applications') ||
          mode === 'protected' ||
          mode === 'officialProtected' ||
          mode === 'backgroundProtected' ||
          mode === 'moleUnknownProtected'
            ? ['clientDirectory']
            : mode === 'system'
              ? ['systemLocation']
              : mode === 'self'
                ? ['currentApplication']
                : [],
        matchedPaths:
          saved?.protectedPaths.includes('/Applications') ||
          mode === 'protected' ||
          mode === 'officialProtected' ||
          mode === 'backgroundProtected' ||
          mode === 'moleUnknownProtected'
            ? ['/Applications']
            : [],
      }
      if (mode === 'pending')
        while (!sessionStorage.getItem('inspection-release'))
          await new Promise((resolve) => setTimeout(resolve, 20))
      sessionStorage.setItem(
        'inspection-completed',
        String(Number(sessionStorage.getItem('inspection-completed') ?? 0) + 1),
      )
      if (mode === 'error')
        throw new Error('应用状态读取超时（核对安装收据），结果无法确认；请稍后重试。')
      const pathRule = (status: string, index = -1) => ({
        version: 2,
        revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
        source: 'moleUninstallPathSubset',
        code:
          (mode?.endsWith('MoleProtected') || mode?.endsWith('MoleIdentity')) &&
          [0, 1, 3].includes(index)
            ? 'protected'
            : status === 'unknown' ||
                ((mode?.endsWith('MoleUnknown') ||
                  mode?.endsWith('MoleIdentityUnknown') ||
                  mode?.endsWith('MoleIdentityLimit')) &&
                  [0, 1, 3].includes(index))
              ? 'unknown'
              : 'notMatched',
        reason:
          (mode?.endsWith('MoleProtected') || mode?.endsWith('MoleIdentity')) &&
          [0, 1, 3].includes(index)
            ? mode?.endsWith('MoleIdentity')
              ? 'endpointSecurityIdentity'
              : 'systemComponent'
            : null,
        issue:
          (mode?.endsWith('MoleProtected') || mode?.endsWith('MoleIdentity')) &&
          [0, 1, 3].includes(index)
            ? null
            : (mode?.endsWith('MoleUnknown') ||
                  mode?.endsWith('MoleIdentityUnknown') ||
                  mode?.endsWith('MoleIdentityLimit')) &&
                [0, 1, 3].includes(index)
              ? mode?.endsWith('MoleIdentityUnknown')
                ? 'identityUnverified'
                : mode?.endsWith('MoleIdentityLimit')
                  ? 'identityLimitReached'
                  : 'rulesUnavailable'
              : status === 'unknown'
                ? 'locationUnverified'
                : null,
      })
      const dataMode = mode?.startsWith('extension') ?? false
      const groupMode = mode?.startsWith('extensionGroups') ?? false
      const groupIssue =
        mode === 'extensionGroupsChanged'
          ? 'changed'
          : mode === 'extensionGroupsLimit'
            ? 'limitReached'
            : mode === 'extensionGroupsUnsupported'
              ? 'unsupported'
              : null
      const sourceChanged = mode === 'extensionSourceChanged'
      const sourcePartial = mode === 'extensionPartialSource'
      const sourceIds =
        !sourceChanged &&
        (dataMode ||
          (mode?.startsWith('embedded') &&
            !['embeddedChanged', 'embeddedUnavailable'].includes(mode)))
          ? ['org.example.helper', 'org.example.same.share']
          : []
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
      const dataEntries = sourceIds.flatMap((id) =>
        locations.map(([kind, root, suffix]) => ({
          relativePath: `${root}/${id}${suffix}`,
          sources: [{ bundleId: id, kind }],
          status:
            dataMode && id === sourceIds[0] && kind === 'scripts'
              ? 'present'
              : dataMode && id === sourceIds[0] && kind === 'fileProvider'
                ? 'unknown'
                : 'notFound',
          issue: dataMode && id === sourceIds[0] && kind === 'fileProvider' ? 'unavailable' : null,
          protectionReasons:
            dataMode &&
            mode !== 'extensionGroupsShared' &&
            id === sourceIds[0] &&
            ['scripts', 'preferences'].includes(kind)
              ? ['clientDirectory']
              : [],
        })),
      )
      if (dataMode && sourceIds.length)
        dataEntries.push({
          relativePath: 'Library/Preferences/ByHost/org.example.same.share.DEVICE.plist',
          sources: [{ bundleId: 'org.example.same.share', kind: 'byHost' }],
          status: mode === 'extensionByHostChanged' ? 'unknown' : 'present',
          issue: mode === 'extensionByHostChanged' ? 'changed' : null,
          protectionReasons: [],
        })
      if (groupMode) {
        for (const [name, ids] of [
          ['TEAM.org.example.helper.org.example.same.share', sourceIds],
          ['TEAM.org.example.same.share.dev.orbstack', ['org.example.same.share']],
        ] as [string, string[]][]) {
          dataEntries.push({
            relativePath: `Library/Group Containers/${name}`,
            sources: ids.map((bundleId) => ({ bundleId, kind: 'groupContainer' })),
            status:
              groupIssue === 'changed' || (groupIssue === 'unsupported' && ids.length === 1)
                ? 'unknown'
                : 'present',
            issue:
              groupIssue === 'changed'
                ? 'changed'
                : groupIssue === 'unsupported' && ids.length === 1
                  ? 'unsupported'
                  : null,
            protectionReasons:
              ids.length > 1 && mode !== 'extensionGroupsShared' ? ['clientDirectory'] : [],
          })
        }
      }
      return {
        protocol: mode === 'future' ? 22 : 21,
        checkedAt: Date.now(),
        bundleId: mode === 'mismatch' ? 'org.example.changed' : 'org.example.same',
        version: mode === 'pending' ? 'old version' : '2.3',
        build: '42',
        identity: mode === 'mismatch' ? 'different' : 'matches',
        protection,
        embedded: {
          version: 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch'
              ? 'notChecked'
              : [
                    'embeddedChanged',
                    'embeddedLimit',
                    'embeddedUnavailable',
                    'extensionSourceChanged',
                    'extensionPartialSource',
                  ].includes(mode ?? '')
                ? 'partial'
                : 'observed',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode === 'embeddedChanged' || sourceChanged
                ? 'changed'
                : mode === 'embeddedLimit'
                  ? 'limitReached'
                  : mode === 'embeddedUnavailable'
                    ? 'unavailable'
                    : sourcePartial
                      ? 'unsupported'
                      : null,
          entries: mode === 'mismatch' ? 0 : 25,
          directories: mode === 'mismatch' ? 0 : 6,
          plists: mode === 'mismatch' ? 0 : 4,
          bundles:
            mode === 'embeddedUnavailable' ||
            mode === 'embeddedChanged' ||
            mode === 'mismatch' ||
            (!mode?.startsWith('embedded') && !dataMode) ||
            sourceChanged
              ? []
              : [
                  {
                    relativePath: 'Contents/PlugIns/Share.appex',
                    bundleId: 'org.example.same.share',
                    kind: 'appex',
                  },
                  {
                    relativePath: 'Contents/Library/LoginItems/Helper.app',
                    bundleId: 'org.example.helper',
                    kind: 'loginItem',
                  },
                ],
        },
        extensionData: {
          version: 3,
          groups: {
            version: 1,
            source: 'clientEmbeddedIdNameBoundary',
            code: sourceIds.length ? (groupIssue ? 'partial' : 'observed') : 'notChecked',
            issue: groupIssue,
            entries: groupMode ? 12 : 0,
            candidates: groupMode ? 2 : 0,
          },
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch'
              ? 'notChecked'
              : sourceChanged || mode === 'embeddedChanged'
                ? 'unknown'
                : groupIssue ||
                    sourcePartial ||
                    [
                      'extensionByHostChanged',
                      'extensionLimit',
                      'embeddedLimit',
                      'embeddedUnavailable',
                    ].includes(mode ?? '')
                  ? 'partial'
                  : 'observed',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : sourceChanged || mode === 'embeddedChanged'
                ? 'embeddedChanged'
                : groupIssue
                  ? 'groupsIncomplete'
                  : sourcePartial || ['embeddedLimit', 'embeddedUnavailable'].includes(mode ?? '')
                    ? 'embeddedIncomplete'
                    : ['extensionByHostChanged', 'extensionLimit'].includes(mode ?? '')
                      ? 'byHostIncomplete'
                      : null,
          identifiers: sourceIds,
          entries: dataEntries.map((entry, index) => ({
            ...entry,
            molePathProtection:
              mode === 'extensionGroupsMole' && entry.relativePath.endsWith('dev.orbstack')
                ? {
                    ...pathRule(entry.status, index),
                    code: 'protected',
                    reason: 'containerRuntime',
                    issue: null,
                  }
                : pathRule(entry.status, index),
          })),
          byHost: {
            code: !sourceIds.length
              ? 'notChecked'
              : ['extensionByHostChanged', 'extensionLimit'].includes(mode ?? '')
                ? 'partial'
                : 'observed',
            issue:
              mode === 'extensionByHostChanged'
                ? 'changed'
                : mode === 'extensionLimit'
                  ? 'limitReached'
                  : null,
            entries: sourceIds.length ? 30 : 0,
            candidates: dataMode && sourceIds.length ? 1 : 0,
          },
        },
        installations: {
          version: mode === 'copiesFuture' ? 6 : 5,
          source: 'moleListRegistryRootsVolumesAndReceipts',
          roots: mode?.startsWith('roots')
            ? {
                code: 'partial',
                issue:
                  mode === 'rootsChanged'
                    ? 'changed'
                    : mode === 'rootsLimit'
                      ? 'limitReached'
                      : 'unavailable',
                rootCount: mode === 'rootsLimit' ? 3 : 9,
                entries: mode === 'rootsLimit' ? 2048 : 200,
                candidates: 80,
                firstUncertainPath: '/Applications',
              }
            : mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  rootCount: 0,
                  entries: 0,
                  candidates: 0,
                  firstUncertainPath: null,
                }
              : {
                  code: 'observed',
                  issue: null,
                  rootCount: 9,
                  entries: 40,
                  candidates: 4,
                  firstUncertainPath: null,
                },
          volumes:
            mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  rootCount: 0,
                  entries: 0,
                  candidates: 0,
                  firstUncertainPath: null,
                }
              : mode?.startsWith('volumes')
                ? {
                    code: 'partial',
                    issue:
                      mode === 'volumesChanged'
                        ? 'changed'
                        : mode === 'volumesLimit'
                          ? 'limitReached'
                          : 'unsupportedVolume',
                    rootCount: 2,
                    entries: 30,
                    candidates: 3,
                    firstUncertainPath: '/Volumes/Shared Disk',
                  }
                : {
                    code: 'observed',
                    issue: null,
                    rootCount: 2,
                    entries: 10,
                    candidates: 2,
                    firstUncertainPath: null,
                  },
          receipts:
            mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  packageCount: 0,
                  checkedCount: 0,
                  entries: 0,
                  candidates: 0,
                }
              : {
                  code: mode?.startsWith('receipts') ? 'partial' : 'observed',
                  issue:
                    mode === 'receiptsChanged'
                      ? 'changed'
                      : mode === 'receiptsLimit'
                        ? 'limitReached'
                        : mode === 'receiptsUnsupported'
                          ? 'unsupportedReceipt'
                          : mode === 'receiptsFailed'
                            ? 'unavailable'
                            : null,
                  packageCount: 20,
                  checkedCount: 2,
                  entries: 40,
                  candidates: 2,
                },
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch'
              ? 'unknown'
              : mode === 'copiesPartial' ||
                  mode === 'copiesChanged' ||
                  mode?.startsWith('roots') ||
                  mode?.startsWith('volumes') ||
                  mode?.startsWith('receipts')
                ? 'partial'
                : 'observed',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode === 'copiesPartial'
                ? 'candidateUnavailable'
                : mode === 'copiesChanged'
                  ? 'candidatesChanged'
                  : mode?.startsWith('roots')
                    ? 'rootsIncomplete'
                    : mode?.startsWith('volumes')
                      ? 'volumesIncomplete'
                      : mode?.startsWith('receipts')
                        ? 'receiptsIncomplete'
                        : null,
          copies: ['copiesFound', 'copiesPartial', 'sharedRelated', 'sharedChannel'].includes(
            mode ?? '',
          )
            ? [
                {
                  path: '/Applications/Example Beta.app',
                  bundleId:
                    mode === 'sharedRelated'
                      ? 'org.example.same.share'
                      : mode === 'sharedChannel'
                        ? 'org.other.app'
                        : 'ORG.EXAMPLE.SAME',
                  relation:
                    mode === 'sharedRelated'
                      ? 'relatedIdentifier'
                      : mode === 'sharedChannel'
                        ? 'channelName'
                        : 'sameIdentifier',
                  version: '3.0',
                  build: '70',
                },
              ]
            : [],
        },
        associations: {
          names: {
            version: mode === 'associationNamesLegacy' ? 1 : 2,
            source:
              mode === 'associationNamesOldSource'
                ? 'moleBaseAppNameLibrary'
                : 'moleLiteralAppNameLibrary',
            code:
              mode === 'associationNamesLimit'
                ? 'unknown'
                : mode?.startsWith('associationNames') && mode !== 'associationNamesUnavailable'
                  ? 'observed'
                  : 'notChecked',
            issue:
              mode === 'associationNamesLimit'
                ? 'limitReached'
                : mode?.startsWith('associationNames') && mode !== 'associationNamesUnavailable'
                  ? null
                  : 'nameUnavailable',
            name:
              mode === 'associationNamesWrong'
                ? 'Another'
                : mode?.startsWith('associationNames') &&
                    !['associationNamesLimit', 'associationNamesUnavailable'].includes(mode)
                  ? 'Example'
                  : null,
            candidates:
              mode?.startsWith('associationNames') &&
              !['associationNamesLimit', 'associationNamesUnavailable'].includes(mode)
                ? 24
                : 0,
          },
          byHost: {
            code:
              mode?.startsWith('associationMainFiles') && mode !== 'associationMainFiles'
                ? 'partial'
                : mode?.startsWith('association') && mode !== 'associationFuture'
                  ? 'observed'
                  : 'notChecked',
            issue:
              mode === 'associationMainFilesChanged'
                ? 'changed'
                : mode === 'associationMainFilesLimit'
                  ? 'limitReached'
                  : null,
            entries: mode?.startsWith('associationMainFiles') ? 12 : 0,
            candidates: mode?.startsWith('associationMainFiles') ? 2 : 0,
          },
          version: mode === 'associationFuture' ? 8 : 7,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode?.startsWith('association') && mode !== 'associationFuture'
              ? 'observed'
              : 'unknown',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('association') && mode !== 'associationFuture'
                ? null
                : 'homeUnavailable',
          discovery:
            mode?.startsWith('association') && mode !== 'associationFuture'
              ? {
                  code: mode?.startsWith('associationRelated') ? 'partial' : 'observed',
                  issue:
                    mode === 'associationRelatedChanged'
                      ? 'changed'
                      : mode === 'associationRelatedLimit'
                        ? 'limitReached'
                        : mode?.startsWith('associationRelated')
                          ? 'unavailable'
                          : null,
                  rootCount: 4,
                  entries: 20,
                  candidates: mode?.startsWith('associationRelated') ? 2 : 0,
                  firstUncertainRoot: mode?.startsWith('associationRelated')
                    ? 'Library/Group Containers'
                    : null,
                }
              : {
                  code: 'notChecked',
                  issue: null,
                  rootCount: 0,
                  entries: 0,
                  candidates: 0,
                  firstUncertainRoot: null,
                },
          entries:
            mode?.startsWith('association') && mode !== 'associationFuture'
              ? [
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
                  .map(([kind, root, suffix], index) => ({
                    kind,
                    molePathProtection: pathRule(
                      mode === 'associationChanged' || index === 1 ? 'unknown' : 'notFound',
                      index,
                    ),
                    relativePath: `${root}/org.example.same${suffix}`,
                    status:
                      mode === 'associationChanged'
                        ? 'unknown'
                        : index === 0 || (mode?.startsWith('associationMainFiles') && index >= 16)
                          ? 'present'
                          : index === 1
                            ? 'unknown'
                            : 'notFound',
                    issue:
                      mode === 'associationChanged'
                        ? 'changed'
                        : index === 1
                          ? 'unsupported'
                          : null,
                    protectionReasons:
                      mode === 'associationProtected' && [0, 1, 11].includes(index)
                        ? ['clientDirectory']
                        : [],
                  }))
                  .concat(
                    mode?.startsWith('associationRelated')
                      ? [
                          {
                            kind: 'groupContainer',
                            molePathProtection: pathRule(
                              mode === 'associationRelatedChanged' ? 'unknown' : 'present',
                            ),
                            relativePath: 'Library/Group Containers/TEAM.org.example.same.shared',
                            status: mode === 'associationRelatedChanged' ? 'unknown' : 'present',
                            issue: mode === 'associationRelatedChanged' ? 'changed' : null,
                            protectionReasons: ['clientDirectory'],
                          },
                          {
                            kind: 'groupContainer',
                            molePathProtection: pathRule(
                              mode === 'associationRelatedChanged' ? 'unknown' : 'present',
                            ),
                            relativePath: 'Library/Group Containers/group.org.example.same.shared',
                            status: mode === 'associationRelatedChanged' ? 'unknown' : 'present',
                            issue: mode === 'associationRelatedChanged' ? 'changed' : null,
                            protectionReasons: [],
                          },
                        ]
                      : [],
                  )
                  .concat(
                    mode?.startsWith('associationMainFiles')
                      ? [
                          {
                            kind: 'byHost',
                            relativePath: 'Library/Preferences/ByHost/org.example.same.plist',
                            status: mode === 'associationMainFilesChanged' ? 'unknown' : 'present',
                            issue: mode === 'associationMainFilesChanged' ? 'changed' : null,
                            protectionReasons: ['clientDirectory'],
                            molePathProtection: pathRule(
                              mode === 'associationMainFilesChanged' ? 'unknown' : 'present',
                            ),
                          },
                          {
                            kind: 'byHost',
                            relativePath:
                              'Library/Preferences/ByHost/org.example.same.DEVICE.plist',
                            status: 'present',
                            issue: null,
                            protectionReasons: [],
                            molePathProtection: pathRule(),
                          },
                        ]
                      : [],
                  )
                  .concat(
                    mode?.startsWith('associationNames') &&
                      !['associationNamesLimit', 'associationNamesUnavailable'].includes(mode)
                      ? [
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
                        ].map(([kind, root, suffix], index) => ({
                          kind,
                          relativePath: `${root}/Example${suffix}`,
                          status:
                            mode === 'associationNamesChanged' ||
                            index === 1 ||
                            (mode === 'associationNamesComponents' && index === 7)
                              ? 'unknown'
                              : index === 0 ||
                                  (mode === 'associationNamesComponents' && [6, 11].includes(index))
                                ? 'present'
                                : 'notFound',
                          issue:
                            mode === 'associationNamesChanged'
                              ? 'changed'
                              : index === 1 ||
                                  (mode === 'associationNamesComponents' && index === 7)
                                ? 'unavailable'
                                : null,
                          protectionReasons:
                            index === 3 || (mode === 'associationNamesComponents' && index === 23)
                              ? ['clientDirectory']
                              : [],
                          molePathProtection: pathRule(
                            mode === 'associationNamesChanged' ||
                              index === 1 ||
                              (mode === 'associationNamesComponents' && index === 7)
                              ? 'unknown'
                              : index === 0 ||
                                  (mode === 'associationNamesComponents' && [6, 11].includes(index))
                                ? 'present'
                                : 'notFound',
                          ),
                        }))
                      : [],
                  )
              : [],
        },
        background: {
          version: mode === 'backgroundFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' ||
            mode?.startsWith('moleUnknown') ||
            mode === 'backgroundUnknown' ||
            mode === 'backgroundMarkerUnknown' ||
            mode === 'backgroundChanged' ||
            mode === 'backgroundProtected'
              ? 'unknown'
              : mode === 'backgroundRoot'
                ? 'directRoot'
                : mode === 'backgroundRestricted'
                  ? 'restricted'
                  : 'notRequired',
          marker:
            mode === 'backgroundMarkerUnknown'
              ? 'unknown'
              : mode?.startsWith('background') && mode !== 'backgroundFuture'
                ? 'enabled'
                : 'absent',
          rootKind: mode === 'backgroundRoot' ? 'externalApplications' : null,
          reason: mode === 'backgroundRestricted' ? 'outsideRoots' : null,
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : mode === 'backgroundUnknown' || mode === 'backgroundProtected'
                  ? 'rootsUnavailable'
                  : mode === 'backgroundMarkerUnknown'
                    ? 'markerUnknown'
                    : mode === 'backgroundChanged'
                      ? 'rootsChanged'
                      : null,
        },
        officialUninstaller: {
          version: mode === 'officialFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' || mode?.startsWith('moleUnknown') || mode === 'officialMissingName'
              ? 'unknown'
              : mode === 'official' || mode === 'officialProtected'
                ? 'officialRequired'
                : 'notMatched',
          vendor: mode === 'official' || mode === 'officialProtected' ? 'ESET' : null,
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : mode === 'officialMissingName'
                  ? 'nameUnavailable'
                  : null,
        },
        moleProtection: {
          version: mode === 'moleFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' || mode?.startsWith('moleUnknown')
              ? 'unknown'
              : mode === 'moleProtected'
                ? 'protected'
                : 'notMatched',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : null,
        },
        running:
          mode === 'mismatch'
            ? {
                code: 'unknown',
                pids: [],
                otherCopies: 0,
                reason: '本地应用标识与 Mole 清单不同，请重新读取列表。',
              }
            : mode === 'other'
              ? {
                  code: 'notObserved',
                  pids: [],
                  otherCopies: 1,
                  reason: '本次未发现此安装位置的匹配主应用；辅助进程、后台服务和文件占用未检查。',
                }
              : {
                  code: 'observed',
                  pids: [123],
                  otherCopies: 0,
                  reason: '本次发现此安装位置的主应用运行；未检查辅助进程或后台服务。',
                },
      }
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await expect(page.getByText('应用列表进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(5)
  const row = library.getByRole('article', {
    name: 'Example · /Applications/Example.app',
    exact: true,
  })
  return { library, row, inspection: row.getByRole('region', { name: '应用状态检查' }) }
}

test('opening details checks only the chosen task entry and distinguishes running, other copies and unknown identity', async ({
  page,
}) => {
  const { library, row, inspection } = await setup(page)
  expect(await page.evaluate(() => sessionStorage.getItem('inspection-calls'))).toBeNull()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await expect(inspection).toContainText('2.3 · 构建 42')
  await expect(inspection).toContainText('未验证签名')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('完整归属仍待核实')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(Object.keys(calls[0]).sort()).toEqual(['index', 'taskId'])
  expect(calls[0].index).toBe(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await row.getByText('位置与来源', { exact: true }).click()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await expect(inspection).toContainText('另有 1 个同标识安装位置')
  await expect(inspection).toContainText('不代表可卸载')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'mismatch'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toHaveText('运行状态无法确认')
  await expect(inspection).toContainText('本地应用标识与 Mole 清单不同')
  await expect(inspection).not.toContainText('主应用 PID')
  await expect(library.getByRole('button', { name: /退出应用|卸载|删除/ })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-inspection-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
})

test('a refreshed list discards a delayed observation from its old task', async ({ page }) => {
  const { library, row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection.getByRole('button', { name: '检查中…' })).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('inspection-completed')))
    .toBe('2')
  await expect(inspection).not.toContainText('old version')
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(calls).toHaveLength(2)
  expect(calls[0].taskId).not.toBe(calls[1].taskId)
})

test('background-only restrictions keep root observations, unknowns and client protection separate', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('未标记为后台专用应用')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundRoot'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('后台专用应用，位于 Mole 搜索目录（外接卷 Applications）')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('不代表可卸载')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundRestricted'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留 · 当前仅供查看')
  await expect(inspection).toContainText('此后台应用受 Mole 目录范围限制')
  await expect(inspection).toContainText('请保留并通过所属应用处理')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    await page.screenshot({
      path: `test-results/application-background-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  for (const [mode, reason] of [
    ['backgroundUnknown', '应用目录、搜索位置或访问状态无法确认'],
    ['backgroundChanged', '搜索目录或访问状态在检查期间发生变化'],
    ['backgroundMarkerUnknown', '后台专用标记格式无法识别'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(inspection).toContainText('后台应用条件无法确认')
    await expect(inspection).toContainText(reason)
    await expect(inspection).toContainText('归属待确认')
    await expect(inspection).not.toContainText('Mole 要求保留')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('后台应用条件无法确认')
  await expect(inspection).toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('official uninstaller hints explain the next step and preserve protection and unknown boundaries', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('本次未命中官方卸载器限制')
  await expect(inspection).toContainText('归属待确认')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'official'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('请使用官方卸载器 · 当前仅供查看')
  await expect(inspection).toContainText('Mole 要求使用 ESET 官方卸载器')
  await expect(inspection).toContainText('组织管理的应用请联系管理员')
  await expect(inspection).not.toContainText('归属待确认')
  await inspection.getByText('规则来源', { exact: true }).click()
  await expect(inspection).toContainText('official_uninstaller_vendor')
  await expect(inspection).toContainText('厂商身份未认证')
  await inspection.getByText('规则来源', { exact: true }).click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-official-uninstaller-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).toContainText('Mole 要求使用 ESET 官方卸载器')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialMissingName'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('官方卸载器要求无法确认')
  await expect(inspection).toContainText('Mole 清单名称缺失或无法识别，请重新读取列表')
  await expect(inspection).not.toContainText('ESET')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknownProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('官方卸载器要求无法确认')
  await expect(
    inspection.getByText('内置规则资源未通过校验或无法读取，请检查引擎状态后重试。', {
      exact: true,
    }),
  ).toHaveCount(1)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('Mole protections, no-match and unavailable rules remain distinct while client guards survive', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('本次未命中系统组件禁用规则')
  await inspection.getByText('规则来源', { exact: true }).click()
  await expect(inspection.getByText(/should_protect_from_uninstall/)).toBeVisible()
  await expect(inspection).toContainText('ba3a0f57f78e')
  await inspection.getByText('规则来源', { exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留 · 当前仅供查看')
  await expect(inspection).toContainText('命中 Mole 系统组件保护')
  await expect(inspection).not.toContainText('归属待确认')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-mole-rules-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknownProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 系统组件规则无法确认')
  await expect(inspection).toContainText('内置规则资源未通过校验或无法读取')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).not.toContainText('命中 Mole 系统组件保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknown'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('Mole 系统组件规则无法确认')
})

test('guards explain system location, current client and protected directories without claiming uninstall eligibility', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  for (const [mode, reason] of [
    ['system', '位于系统目录'],
    ['self', '当前客户端或同标识应用（未验证签名）'],
    ['protected', '与已保存保护目录重叠'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(inspection).toContainText('已命中客户端保护 · 当前仅供查看')
    await expect(inspection).toContainText(reason)
    await expect(inspection).not.toContainText('归属待确认')
    await expect(inspection).toContainText('完整归属仍待核实')
  }
  await expect(inspection).toContainText('/Applications')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-protection-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
})

test('saved protection changes invalidate an in-flight observation and its delayed result', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await page.getByRole('button', { name: '设置', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '设置', exact: true })
  await dialog.getByRole('button', { name: '保护列表', exact: true }).click()
  await dialog.getByRole('button', { name: '添加目录', exact: true }).click()
  await expect(dialog.getByText('/Applications', { exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).toContainText('/Applications')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('inspection-completed')))
    .toBe('2')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).not.toContainText('old version')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await dialog.getByRole('button', { name: '保护列表', exact: true }).click()
  await dialog.getByRole('button', { name: '移除保护 /Applications', exact: true }).click()
  await page.keyboard.press('Escape')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).not.toContainText('已命中客户端保护')
})

test('read errors and unrecognized protocol stay unknown and retry without keeping a positive result', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'protected'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await expect(inspection).toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'error'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toHaveText(
    '应用状态读取超时（核对安装收据），结果无法确认；请稍后重试。',
  )
  await expect(inspection.getByRole('status')).toHaveText('应用状态无法确认')
  await expect(inspection).not.toContainText('主应用 PID')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await inspection.screenshot({
      path: `test-results/application-timeout-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'future'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await page.evaluate(() => sessionStorage.removeItem('inspection-mode'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})

test('an open observation becomes visibly older and rechecking withdraws the old conclusion while pending', async ({
  page,
}) => {
  await page.clock.install()
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await page.clock.fastForward(60001)
  await expect(inspection.getByRole('status')).toHaveText('较早结果 · 发现主应用运行')
  await expect(inspection).toContainText('较早保护结果 · 归属待确认')
  await expect(inspection).toContainText('请重新检查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection).not.toContainText('主应用 PID')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})

test('exact association locations stay retained, partial and revocable', async ({ page }) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationObserved'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(
    inspection.getByText(
      '关联位置 · 1 处发现 · 1 处无法确认 · 1 处路径保护未确认 · 名称位置未检查',
      {
        exact: true,
      },
    ),
  ).toBeVisible()
  await inspection.locator('.application-associations summary').click()
  const associations = inspection.locator('.application-associations')
  await expect(associations).toContainText('全部保留')
  await expect(associations).toContainText('缓存也可能含有个人数据')
  await expect(associations).toContainText('发现位置 · 归属待确认')
  await expect(associations).toContainText('~/Library/Application Support/org.example.same')
  await expect(associations).toContainText('链接、挂载或文件类型暂不支持')
  await expect(associations).toContainText('19 处常见位置本次未发现，不代表没有其他关联文件')
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-associations-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('0 处发现 · 21 处无法确认')
  await expect(associations).toContainText('检查期间发生变化，请重查')
  await expect(associations).not.toContainText('发现位置 · 归属待确认')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'mismatch'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('本地标识已变化，请重新读取应用列表')
  await expect(associations.locator('.association-path')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection.locator('.application-associations')).toHaveCount(0)
})

test('registered installation copies explain shared data and keep incomplete or changed results conservative', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('关联数据可能共用')
  const copies = inspection.locator('.application-installations')
  await copies.locator('summary').click()
  await expect(copies).toContainText('发现 1 个其他位置')
  await expect(copies).toContainText('/Applications/Example Beta.app')
  await expect(copies).toContainText('版本 3.0 · 构建 70')
  await expect(copies).toContainText('可能共用关联数据，请保留')
  await expect(copies).toContainText('网络卷与其他安装位置尚未完整覆盖')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await copies.screenshot({
      path: `test-results/application-copies-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesPartial'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies.locator('summary')).toContainText('检查不完整')
  await expect(copies).toContainText('结果不完整')
  await expect(copies).toContainText('/Applications/Example Beta.app')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies).toContainText('检查期间发生变化，已撤下对应结果')
  await expect(copies).not.toContainText('/Applications/Example Beta.app')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies).toContainText('本次未命中不代表数据独占')
  await expect(inspection).toContainText('另有 1 个同标识安装位置的主应用在运行')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection.locator('.application-installations')).toHaveCount(0)
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('Mole retention reasons distinguish derived identifiers and channel names without claiming ownership', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'sharedRelated'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(inspection).toContainText('关联数据可能共用')
  await expect(shared).toContainText('派生标识，需保留关联数据')
  await expect(shared).toContainText('核对 Mole 清单前 64 项、系统登记前 32 项')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'sharedChannel'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await shared.locator('summary').click()
  await expect(shared).toContainText('渠道包名命中，需保留关联数据')
  await expect(shared).not.toContainText('派生标识，需保留关联数据')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-shared-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(shared.getByRole('checkbox')).toHaveCount(0)
  await expect(shared.getByRole('button')).toHaveCount(0)
})

test('installation-root discovery exposes inaccessible, changed and limited scope without clearing retention', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'rootsUnavailable'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared.locator('summary')).toContainText('检查不完整')
  await expect(shared).toContainText('已探查 9 个常用安装根')
  await expect(shared).toContainText('部分目录无法访问或布局不支持')
  await expect(shared).toContainText('/Applications')
  await expect(shared).toContainText('本次未命中不代表数据独占')
  for (const [mode, reason] of [
    ['rootsChanged', '目录在检查期间发生变化，请重查'],
    ['rootsLimit', '扫描达到数量或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared.locator('summary')).toContainText('检查不完整')
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-roots-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(shared.getByRole('button')).toHaveCount(0)
  await expect(shared.getByRole('checkbox')).toHaveCount(0)
})

test('volume limitations keep data retained and show network, change and budget reasons', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'volumesNetwork'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared).toContainText('检查不完整')
  await expect(shared).toContainText('网络卷、自动挂载或系统隐藏卷暂不支持，结果不完整')
  await expect(shared).toContainText('/Volumes/Shared Disk')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const [mode, reason] of [
    ['volumesChanged', '挂载或卷目录发生变化，请重查'],
    ['volumesLimit', '卷扫描达到数量或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-volumes-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('receipt discovery keeps failure, changes, bounds and unsupported records visible without clearing retention', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'receiptsFailed'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared.locator('summary')).toContainText('检查不完整')
  await expect(shared).toContainText('已核对 2 份非 Apple 安装收据')
  await expect(shared).toContainText('部分收据未能完整读取，请重试')
  await expect(shared).toContainText('只补查 /usr/local、/opt')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const [mode, reason] of [
    ['receiptsChanged', '安装收据在检查期间变化，请重查'],
    ['receiptsLimit', '收据核对达到数量或时间上限'],
    ['receiptsUnsupported', '部分收据格式或路径暂不支持'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-receipts-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('per-location protection stays visible for found, unknown and missing data and clears after rechecking', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationProtected'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('关联位置受保护')
  const associations = inspection.locator('.application-associations')
  await expect(associations.locator('summary')).toContainText('3 处受保护')
  await associations.locator('summary').click()
  await expect(associations.getByText('已保存保护目录覆盖，保留', { exact: true })).toHaveCount(3)
  await expect(associations).toContainText('无法确认')
  await expect(associations).toContainText('本次未发现')
  await expect(associations).toContainText('~/Library/Application Scripts/org.example.same')
  await expect(associations).toContainText('21 个常见位置')
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-associated-protection-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationObserved'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).not.toContainText('处受保护')
  await expect(associations).not.toContainText('已保存保护目录覆盖')
  await expect(associations).not.toContainText('~/Library/Application Scripts/org.example.same')
})

test('group and derived directory candidates stay retained while source failures, changes and limits remain visible', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationRelated'))
  await row.getByText('位置与来源', { exact: true }).click()
  const associations = inspection.locator('.application-associations')
  await expect(associations.locator('summary')).toContainText('组与扩展检查不完整')
  await associations.locator('summary').click()
  await expect(associations).toContainText('已检查 4 个应用组与扩展目录')
  await expect(associations.getByText('应用组容器', { exact: true })).toHaveCount(2)
  await expect(associations).toContainText('部分目录无法完整读取，请重试')
  await expect(associations).toContainText(
    '~/Library/Group Containers/TEAM.org.example.same.shared',
  )
  await expect(associations).toContainText('目录名规则命中，可能由多个应用共用，保留')
  await expect(associations).toContainText('已保存保护目录覆盖，保留')
  await expect(associations).toContainText('完整归属仍待核实')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-related-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationRelatedChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('目录在检查期间变化，请重查')
  await expect(associations).toContainText('检查期间发生变化，请重查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationRelatedLimit'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('扫描达到数量或时间上限')
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  await expect(associations.getByRole('button')).toHaveCount(0)
})

test('embedded identities expose scope and withdraw changed results without offering data actions', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'embeddedFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  const embedded = inspection.getByLabel('嵌入扩展', { exact: true })
  await expect(embedded.locator('summary')).toContainText('2 个位置')
  await embedded.locator('summary').click()
  await expect(embedded).toContainText('org.example.same.share')
  await expect(embedded).toContainText('Contents/Library/LoginItems/Helper.app')
  await expect(embedded).toContainText('签名、运行状态和数据归属未验证')
  await expect(embedded).toContainText('不证明组成员、数据独占或可卸载')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    const identityColumn = await embedded.locator('dd').first().boundingBox()
    expect(identityColumn?.width).toBeGreaterThan(300)
    await embedded.screenshot({
      path: `test-results/application-embedded-${theme}.png`,
      animations: 'disabled',
    })
  }
  for (const [mode, message] of [
    ['embeddedChanged', '组件在检查期间变化'],
    ['embeddedUnavailable', '部分组件无法完整读取'],
    ['embeddedLimit', '检查达到数量、深度或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(embedded.locator('summary')).toContainText('检查不完整')
    await expect(embedded).toContainText(message)
    if (mode === 'embeddedUnavailable') await expect(embedded).toContainText('未取得可查询标识')
    if (mode !== 'embeddedLimit') await expect(embedded).not.toContainText('org.example.same.share')
  }
  await expect(embedded.getByRole('checkbox')).toHaveCount(0)
  await expect(embedded.getByRole('button')).toHaveCount(0)
})

test('extension data keeps found, unknown and protected missing locations distinct and withdraws changed identities', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('关联位置受保护')
  const embedded = inspection.getByLabel('嵌入扩展', { exact: true })
  await embedded.locator('summary').click()
  const data = embedded.getByLabel('扩展关联数据', { exact: true })
  await expect(data).toContainText('2 处发现 · 1 处无法确认')
  await expect(data).toContainText('2 处受保护')
  await expect(data).toContainText('本次未发现')
  await expect(data).toContainText('~/Library/Preferences/org.example.helper.plist')
  await expect(data).toContainText(
    '~/Library/Preferences/ByHost/org.example.same.share.DEVICE.plist',
  )
  await expect(data).toContainText('全部保留')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect((await data.locator('dd').first().boundingBox())?.width).toBeGreaterThan(300)
    await data.screenshot({
      path: `test-results/application-extension-data-${theme}.png`,
      animations: 'disabled',
    })
  }
  for (const [mode, message] of [
    ['extensionByHostChanged', '检查期间变化，请重查'],
    ['extensionLimit', '扫描达到数量或时间上限'],
    ['extensionPartialSource', '组件检查仍不完整'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(embedded.locator('summary')).toContainText('检查不完整')
    await expect(data).toContainText(message)
    await expect(data).toContainText('~/Library/Application Scripts/org.example.helper')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionSourceChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(data).toContainText('已撤下数据位置')
  await expect(data).not.toContainText('~/Library/Application Scripts/org.example.helper')
  await expect(inspection).not.toContainText('关联位置受保护')
  await expect(data.getByRole('checkbox')).toHaveCount(0)
  await expect(data.getByRole('button')).toHaveCount(0)
})

test('Mole literal uninstall path retention stays visible for unknown and missing locations and resets on recheck', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationMoleProtected'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('Mole 要求保留')
  const associations = inspection.locator('.application-associations')
  await expect(associations.locator('summary')).toContainText('Mole 保留 3 处')
  await associations.locator('summary').click()
  await expect(associations.getByText('Mole：系统组件路径，保留', { exact: true })).toHaveCount(3)
  await expect(associations).toContainText('本次未发现')
  await expect(associations).toContainText('无法确认')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-mole-path-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationMoleUnknown'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('请检查内置规则后重试')
  await expect(associations).not.toContainText('Mole：系统组件路径，保留')
  await expect(inspection).not.toContainText('Mole 要求保留')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionMoleProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留')
  const embedded = inspection.getByLabel('嵌入扩展', { exact: true })
  await embedded.locator('summary').click()
  const data = embedded.getByLabel('扩展关联数据', { exact: true })
  await expect(data).toContainText('Mole 保留 3 处')
  await expect(data.getByText('Mole：系统组件路径，保留', { exact: true })).toHaveCount(3)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionFound'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(data).not.toContainText('Mole：系统组件路径，保留')
  await expect(data).toContainText('已保存保护目录覆盖，保留')
  await expect(data.getByRole('checkbox')).toHaveCount(0)
  await expect(data.getByRole('button')).toHaveCount(0)
  const source = inspection
    .locator('details')
    .filter({ has: page.locator('summary').getByText('规则来源', { exact: true }) })
  await source.locator('summary').click()
  await expect(source).toContainText('不含完整别名解析、白名单或执行许可')
})

test('endpoint ancestor identity retention and unknown budgets remain explicit without modification controls', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationMoleIdentity'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('Mole 要求保留')
  const associations = inspection.locator('.application-associations')
  await associations.locator('summary').click()
  await expect(
    associations.getByText('Mole：终端安全缓存所在目录，保留', { exact: true }),
  ).toHaveCount(3)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-mole-identity-${theme}.png`,
      animations: 'disabled',
    })
  }
  for (const [mode, message] of [
    ['associationMoleIdentityUnknown', '目录身份尚未核实'],
    ['associationMoleIdentityLimit', '目录身份检查未完成'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(associations).toContainText(message)
    await expect(associations).not.toContainText('终端安全缓存所在目录，保留')
    await expect(inspection).not.toContainText('Mole 要求保留')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionMoleIdentity'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  const embedded = inspection.getByLabel('嵌入扩展', { exact: true })
  await embedded.locator('summary').click()
  const data = embedded.getByLabel('扩展关联数据', { exact: true })
  await expect(data.getByText('Mole：终端安全缓存所在目录，保留', { exact: true })).toHaveCount(3)
  await expect(data).toContainText('已保存保护目录覆盖，保留')
  await expect(data.getByRole('checkbox')).toHaveCount(0)
  await expect(data.getByRole('button')).toHaveCount(0)
})

test('main device settings and recent document records stay retained with explicit incomplete scope', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationMainFiles'))
  await row.getByText('位置与来源', { exact: true }).click()
  const associations = inspection.locator('.application-associations')
  await associations.locator('summary').click()
  await expect(associations).toContainText('设置目录')
  await expect(associations).toContainText('下载缓存')
  await expect(
    associations.getByText('个人使用记录，保留；不读取或处理原文档。', { exact: true }),
  ).toHaveCount(3)
  await expect(associations.getByText('设备专用设置，保留。', { exact: true })).toHaveCount(2)
  await expect(associations).toContainText('~/Library/Preferences/ByHost/org.example.same.plist')
  await expect(associations).toContainText('已保存保护目录覆盖，保留')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(
      await associations
        .locator('.association-path')
        .evaluateAll((elements) =>
          elements.every((element) => element.scrollWidth <= element.clientWidth + 1),
        ),
    ).toBe(true)
    await associations.screenshot({
      path: `test-results/application-main-files-${theme}.png`,
      animations: 'disabled',
    })
  }
  for (const [mode, message] of [
    ['associationMainFilesChanged', '目录或设置在检查期间变化'],
    ['associationMainFilesLimit', '扫描达到数量或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(associations.locator('summary')).toContainText('设备设置检查不完整')
    await expect(associations).toContainText(message)
    await expect(associations).toContainText('设备专用设置，保留。')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationFound'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).not.toContainText('设备设置检查不完整')
  await expect(associations).not.toContainText(
    '~/Library/Preferences/ByHost/org.example.same.plist',
  )
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  await expect(associations.getByRole('button')).toHaveCount(0)
})

test('embedded group name candidates merge sources, retain data and withdraw with their identity', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionGroupsFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  const embedded = inspection.getByLabel('嵌入扩展', { exact: true })
  await embedded.locator('summary').click()
  const data = embedded.getByLabel('扩展关联数据', { exact: true })
  await expect(data.getByText('扩展关联应用组', { exact: true })).toHaveCount(2)
  await expect(
    data.getByText('org.example.helper、org.example.same.share', { exact: true }),
  ).toHaveCount(1)
  await expect(
    data.getByText('目录名与扩展标识关联，可能由多个应用共用，保留；组成员身份未认证。', {
      exact: true,
    }),
  ).toHaveCount(2)
  await expect(data).toContainText('已保存保护目录覆盖，保留')
  await expect(data).toContainText('客户端按已取得标识补查的名称线索')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(
      await data
        .locator('.association-path')
        .evaluateAll((elements) =>
          elements.every((element) => element.scrollWidth <= element.clientWidth + 1),
        ),
    ).toBe(true)
    await data.screenshot({
      path: `test-results/application-extension-groups-${theme}.png`,
      animations: 'disabled',
    })
  }
  for (const [mode, message] of [
    ['extensionGroupsChanged', '目录在检查期间变化'],
    ['extensionGroupsLimit', '扫描达到数量或时间上限'],
    ['extensionGroupsUnsupported', '部分名称、链接或目录布局暂不支持'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(embedded.locator('summary')).toContainText('检查不完整')
    await expect(data).toContainText(message)
    await expect(data).toContainText('组成员身份未认证')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionGroupsShared'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('关联数据可能共用')
  await expect(inspection).not.toContainText('关联位置受保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionGroupsMole'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留')
  await expect(data).toContainText('Mole：容器运行数据，保留')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'extensionSourceChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(data).not.toContainText('~/Library/Group Containers/TEAM.')
  await expect(data).not.toContainText('Mole：容器运行数据，保留')
  await expect(data).toContainText('已撤下数据位置')
  await expect(data.getByRole('checkbox')).toHaveCount(0)
  await expect(data.getByRole('button')).toHaveCount(0)
})

test('literal app-name locations remain separate clues and clear on missing, timed-out or mismatched sources', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  const associations = inspection.locator('.application-associations')
  await associations.locator('summary').click()
  await expect(inspection).toContainText('关联位置受保护')
  await expect(associations).toContainText('另按 Mole 清单名称「Example」查询 24 个位置')
  await expect(associations).toContainText('本地显示名称和归属未认证')
  await expect(associations.getByText('名称线索，不证明归属，保留。', { exact: true })).toHaveCount(
    3,
  )
  await expect(associations).toContainText('~/Library/Application Support/Example')
  await expect(associations).toContainText('已保存保护目录覆盖，保留')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(
      await associations
        .locator('.association-path')
        .evaluateAll((elements) =>
          elements.every((element) => element.scrollWidth <= element.clientWidth + 1),
        ),
    ).toBe(true)
    await associations.screenshot({
      path: `test-results/application-name-hints-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('检查期间发生变化，请重查')
  await expect(associations.getByText('名称线索，不证明归属，保留。', { exact: true })).toHaveCount(
    6,
  )
  await expect(
    associations.getByText('名称线索，注册、使用状态和归属未确认，保留。', { exact: true }),
  ).toHaveCount(18)
  for (const [mode, message] of [
    ['associationNamesUnavailable', '这份 Mole 清单未提供可用名称'],
    ['associationNamesLimit', '检查达到时间上限，请保留并重查'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(associations.locator('summary')).toContainText('名称位置未检查')
    await expect(associations).toContainText(message)
    await expect(associations).not.toContainText('~/Library/Application Support/Example')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesWrong'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('应用状态结果无法识别')
  await expect(inspection).not.toContainText('名称线索，不证明归属')
  await expect(inspection.getByRole('button', { name: '重试检查' })).toBeVisible()
})

test('literal service and plugin locations retain registration unknowns and reject old name sources', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesComponents'))
  await row.getByText('位置与来源', { exact: true }).click()
  const associations = inspection.locator('.application-associations')
  await associations.locator('summary').click()
  await expect(associations).toContainText('查询 24 个位置')
  for (const label of ['服务工作流', 'Quick Look', 'VST3', '邮件插件']) {
    await expect(associations.getByText(label, { exact: true })).toBeVisible()
  }
  await expect(
    associations.getByText('名称线索，注册、使用状态和归属未确认，保留。', { exact: true }),
  ).toHaveCount(4)
  const vst = associations
    .locator('dl > div')
    .filter({ hasText: '~/Library/Audio/Plug-Ins/VST3/Example.vst3' })
  await expect(vst).toContainText('发现位置 · 归属待确认')
  const quickLook = associations
    .locator('dl > div')
    .filter({ hasText: '~/Library/QuickLook/Example.qlgenerator' })
  await expect(quickLook).toContainText('无法确认 · 请检查访问权限后重试')
  const mail = associations
    .locator('dl > div')
    .filter({ hasText: '~/Library/Mail/Bundles/Example.mailbundle' })
  await expect(mail).toContainText('本次未发现')
  await expect(mail).toContainText('已保存保护目录覆盖，保留')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 1100 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(
      await associations
        .locator('.association-path')
        .evaluateAll((elements) =>
          elements.every((element) => element.scrollWidth <= element.clientWidth + 1),
        ),
    ).toBe(true)
    await associations.evaluate((element) => element.scrollIntoView({ block: 'center' }))
    await associations.screenshot({
      path: `test-results/application-component-hints-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(associations.getByRole('button')).toHaveCount(0)
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(vst).toContainText('检查期间发生变化，请重查')
  await expect(vst).not.toContainText('发现位置')
  for (const mode of ['associationNamesLegacy', 'associationNamesOldSource']) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: /重新检查状态|重试检查/ }).click()
    await expect(inspection.getByRole('alert')).toContainText('无法识别')
    await expect(inspection).not.toContainText('Example.vst3')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationNamesComponents'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(associations.locator('summary')).toContainText('处发现')
  await associations.locator('summary').click()
  await expect(vst).toContainText('发现位置 · 归属待确认')
  await page.evaluate(() =>
    sessionStorage.setItem('inspection-mode', 'associationNamesUnavailable'),
  )
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations.locator('summary')).toContainText('名称位置未检查')
  await expect(inspection).not.toContainText('Example.vst3')
})
