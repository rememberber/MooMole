import type { ApplicationInspection } from '../types'

export const moleUninstallRevision = 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f'
const officialVendors = [
  'ESET',
  'Jamf',
  'CrowdStrike',
  'SentinelOne',
  'GlobalProtect',
  'Cisco',
  'Alibaba Cloud Security',
]

export function parseApplicationInspection(
  value: unknown,
  expectedId: string,
  expectedName?: string,
): ApplicationInspection {
  const fail = () => {
    throw new Error('应用状态结果无法识别，请重新检查。')
  }
  if (!value || typeof value !== 'object') return fail()
  const data = value as ApplicationInspection
  const text = (value: unknown, limit: number) =>
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= limit &&
    !/[\u0000-\u001f\u007f]/.test(value)
  if (
    data.protocol !== 27 ||
    !Number.isSafeInteger(data.checkedAt) ||
    data.checkedAt <= 0 ||
    data.checkedAt > 8.64e15 ||
    !['matches', 'different', 'unreported', 'unavailable'].includes(data.identity) ||
    ![data.version, data.build].every((v) => v === null || text(v, 128)) ||
    !(
      data.bundleId === null ||
      (text(data.bundleId, 255) && /^[A-Za-z0-9_.-]+$/.test(data.bundleId))
    )
  )
    return fail()
  const pathProtection = (
    entry:
      | ApplicationInspection['associations']['entries'][number]
      | ApplicationInspection['extensionData']['entries'][number],
  ) => {
    const protection = entry.molePathProtection
    return Boolean(
      protection &&
      protection.version === 2 &&
      protection.revision === moleUninstallRevision &&
      protection.source === 'moleUninstallPathSubset' &&
      (protection.code === 'protected'
        ? [
            'sharedRoot',
            'containerRuntime',
            'systemComponent',
            'criticalData',
            'endpointSecurity',
            'endpointSecurityIdentity',
          ].includes(protection.reason ?? '') && protection.issue === null
        : protection.code === 'notMatched'
          ? protection.reason === null && protection.issue === null && entry.status !== 'unknown'
          : protection.code === 'unknown' &&
            protection.reason === null &&
            [
              'rulesUnavailable',
              'locationUnverified',
              'pathUnsupported',
              'identityUnverified',
              'identityLimitReached',
            ].includes(protection.issue ?? '') &&
            (protection.issue !== 'locationUnverified' || entry.status === 'unknown')),
    )
  }
  const embedded = data.embedded
  const reverseDns = (value: unknown): value is string =>
    text(value, 255) &&
    /^[A-Za-z0-9][A-Za-z0-9-]*(\.[A-Za-z0-9][A-Za-z0-9-]*)+$/.test(value as string)
  if (
    !embedded ||
    embedded.version !== 1 ||
    embedded.revision !== moleUninstallRevision ||
    !['observed', 'partial', 'notChecked'].includes(embedded.code) ||
    !Number.isInteger(embedded.entries) ||
    embedded.entries < 0 ||
    embedded.entries > 2048 ||
    !Number.isInteger(embedded.directories) ||
    embedded.directories < 0 ||
    embedded.directories > 128 ||
    !Number.isInteger(embedded.plists) ||
    embedded.plists < 0 ||
    embedded.plists > 128 ||
    embedded.plists > embedded.entries ||
    embedded.directories > embedded.entries + 1 ||
    !Array.isArray(embedded.bundles) ||
    embedded.bundles.length > 32 ||
    embedded.bundles.length > embedded.plists ||
    (embedded.issue === 'changed' && embedded.bundles.length !== 0) ||
    new Set(embedded.bundles.map((item) => item?.relativePath)).size !== embedded.bundles.length ||
    (embedded.code === 'notChecked'
      ? ![
          'identityChanged',
          'identityUnavailable',
          'rulesUnavailable',
          'bundleIdUnsupported',
        ].includes(embedded.issue ?? '') ||
        embedded.entries !== 0 ||
        embedded.directories !== 0 ||
        embedded.plists !== 0 ||
        embedded.bundles.length !== 0
      : ['different', 'unavailable'].includes(data.identity) ||
        !reverseDns(data.bundleId) ||
        (embedded.code === 'observed'
          ? embedded.issue !== null || embedded.directories === 0
          : !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(
              embedded.issue ?? '',
            ))) ||
    (data.identity === 'different' && embedded.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && embedded.issue !== 'identityUnavailable') ||
    (embedded.issue === 'identityChanged' && data.identity !== 'different') ||
    (embedded.issue === 'identityUnavailable' && data.identity !== 'unavailable') ||
    (embedded.issue === 'bundleIdUnsupported' && reverseDns(data.bundleId)) ||
    embedded.bundles.some((item) => {
      if (
        !item ||
        !text(item.relativePath, 1024) ||
        /[\u007f-\u009f]/.test(item.relativePath) ||
        !item.relativePath.startsWith('Contents/') ||
        item.relativePath.split('/').length > 11 ||
        item.relativePath.split('/').some((part) => !part || part === '.' || part === '..') ||
        !reverseDns(item.bundleId) ||
        item.bundleId === data.bundleId ||
        item.bundleId.startsWith('org.sparkle-project.')
      )
        return true
      const extension = item.relativePath.split('.').at(-1)?.toLowerCase()
      return item.kind === 'appex'
        ? extension !== 'appex'
        : item.kind === 'xpc'
          ? extension !== 'xpc'
          : item.kind !== 'loginItem' ||
            extension !== 'app' ||
            !item.relativePath.startsWith('Contents/Library/LoginItems/')
    })
  )
    return fail()
  const extensionData = data.extensionData
  const extensionLocations = [
    ['scripts', 'Library/Application Scripts', ''],
    ['fileProvider', 'Library/Application Support/FileProvider', ''],
    ['cache', 'Library/Caches', ''],
    ['container', 'Library/Containers', ''],
    ['httpStorage', 'Library/HTTPStorages', ''],
    ['httpCookies', 'Library/HTTPStorages', '.binarycookies'],
    ['preferences', 'Library/Preferences', '.plist'],
    ['webKit', 'Library/WebKit', ''],
  ]
  const availableIds = [...new Set(embedded.bundles.map((bundle) => bundle.bundleId))].sort()
  if (
    !extensionData ||
    extensionData.version !== 3 ||
    extensionData.revision !== moleUninstallRevision ||
    !['observed', 'partial', 'notChecked', 'unknown'].includes(extensionData.code) ||
    !Array.isArray(extensionData.identifiers) ||
    extensionData.identifiers.length > 8 ||
    !extensionData.identifiers.every((id) => reverseDns(id) && availableIds.includes(id)) ||
    new Set(extensionData.identifiers).size !== extensionData.identifiers.length ||
    !Array.isArray(extensionData.entries) ||
    extensionData.entries.length > 192 ||
    new Set(extensionData.entries.map((entry) => entry?.relativePath)).size !==
      extensionData.entries.length ||
    (['notChecked', 'unknown'].includes(extensionData.code)
      ? extensionData.identifiers.length !== 0 ||
        extensionData.entries.length !== 0 ||
        !(
          extensionData.code === 'unknown'
            ? ['embeddedChanged', 'embeddedUnavailable', 'limitReached']
            : [
                'identityChanged',
                'identityUnavailable',
                'rulesUnavailable',
                'bundleIdUnsupported',
                'homeUnavailable',
                'embeddedUnavailable',
              ]
        ).includes(extensionData.issue ?? '')
      : embedded.code === 'notChecked' ||
        embedded.issue === 'changed' ||
        JSON.stringify(extensionData.identifiers) !== JSON.stringify(availableIds.slice(0, 8)) ||
        (extensionData.code === 'observed'
          ? extensionData.issue !== null || embedded.code === 'partial' || availableIds.length > 8
          : ![
              'embeddedIncomplete',
              'byHostIncomplete',
              'groupsIncomplete',
              'limitReached',
            ].includes(extensionData.issue ?? ''))) ||
    (extensionData.issue === 'embeddedIncomplete' && embedded.code !== 'partial') ||
    (extensionData.issue === 'embeddedChanged' && embedded.issue !== 'changed') ||
    (embedded.code === 'notChecked' &&
      (extensionData.code !== 'notChecked' || extensionData.issue !== embedded.issue))
  )
    return fail()
  const expectedExtension = new Map<string, string[]>()
  for (const id of extensionData.identifiers)
    for (const [kind, root, suffix] of extensionLocations) {
      const path = `${root}/${id}${suffix}`
      expectedExtension.set(path, [...(expectedExtension.get(path) ?? []), `${id}:${kind}`])
    }
  const byHostMatch = (name: string, id: string) =>
    name.startsWith(`${id}.`) &&
    name.endsWith('.plist') &&
    name.length >= id.length + 7 &&
    !name.includes('/')
  const groupMatch = (name: string, id: string) =>
    name === id || name.startsWith(`${id}.`) || name.endsWith(`.${id}`) || name.includes(`.${id}.`)
  let groupCount = 0
  let byHostCount = 0
  for (const entry of extensionData.entries) {
    if (
      !entry ||
      !pathProtection(entry) ||
      !text(entry.relativePath, 1024) ||
      /[\u007f-\u009f]/.test(entry.relativePath) ||
      !Array.isArray(entry.sources) ||
      entry.sources.length === 0 ||
      entry.sources.length > 8 ||
      !entry.sources.every(
        (source) => source && extensionData.identifiers.includes(source.bundleId),
      ) ||
      !Array.isArray(entry.protectionReasons) ||
      entry.protectionReasons.length > 3 ||
      new Set(entry.protectionReasons).size !== entry.protectionReasons.length ||
      entry.protectionReasons.some(
        (reason) => !['systemLocation', 'currentApplication', 'clientDirectory'].includes(reason),
      ) ||
      !['present', 'notFound', 'unknown'].includes(entry.status) ||
      (entry.status === 'unknown'
        ? !['unavailable', 'unsupported', 'changed'].includes(entry.issue ?? '')
        : entry.issue !== null)
    )
      return fail()
    const expected = expectedExtension.get(entry.relativePath)
    const sources = entry.sources.map((source) => `${source.bundleId}:${source.kind}`).sort()
    if (expected) {
      if (JSON.stringify(sources) !== JSON.stringify([...expected].sort())) return fail()
      expectedExtension.delete(entry.relativePath)
    } else {
      const byHostRoot = 'Library/Preferences/ByHost/'
      const groupRoot = 'Library/Group Containers/'
      const isGroup = entry.relativePath.startsWith(groupRoot)
      const root = isGroup ? groupRoot : byHostRoot
      if (!entry.relativePath.startsWith(root)) return fail()
      const name = entry.relativePath.slice(root.length)
      if (!name || name.includes('/') || name === '.' || name === '..') return fail()
      const matching = extensionData.identifiers
        .filter((id) => (isGroup ? groupMatch(name, id) : byHostMatch(name, id)))
        .map((id) => `${id}:${isGroup ? 'groupContainer' : 'byHost'}`)
        .sort()
      if (!matching.length || JSON.stringify(sources) !== JSON.stringify(matching)) return fail()
      if (isGroup) groupCount++
      else byHostCount++
    }
  }
  const byHost = extensionData.byHost
  if (
    expectedExtension.size !== 0 ||
    !byHost ||
    !['observed', 'partial', 'notChecked'].includes(byHost.code) ||
    !Number.isInteger(byHost.entries) ||
    byHost.entries < 0 ||
    byHost.entries > 1024 ||
    !Number.isInteger(byHost.candidates) ||
    byHost.candidates < 0 ||
    byHost.candidates > 64 ||
    byHost.candidates > byHost.entries ||
    byHost.candidates !== byHostCount ||
    (byHost.code === 'notChecked') !== (extensionData.identifiers.length === 0) ||
    (byHost.code === 'notChecked' && (byHost.entries !== 0 || byHost.candidates !== 0)) ||
    (byHost.code === 'partial'
      ? extensionData.code !== 'partial' ||
        !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(byHost.issue ?? '')
      : byHost.issue !== null) ||
    (extensionData.issue === 'byHostIncomplete' && byHost.code !== 'partial')
  )
    return fail()
  const groups = extensionData.groups
  if (
    !groups ||
    groups.version !== 1 ||
    groups.source !== 'clientEmbeddedIdNameBoundary' ||
    !['observed', 'partial', 'notChecked'].includes(groups.code) ||
    !Number.isInteger(groups.entries) ||
    groups.entries < 0 ||
    groups.entries > 1024 ||
    !Number.isInteger(groups.candidates) ||
    groups.candidates < 0 ||
    groups.candidates > 64 ||
    groups.candidates > groups.entries ||
    groups.candidates !== groupCount ||
    (groups.code === 'notChecked') !== (extensionData.identifiers.length === 0) ||
    (groups.code === 'notChecked' && (groups.entries !== 0 || groups.candidates !== 0)) ||
    (groups.code === 'partial'
      ? extensionData.code !== 'partial' ||
        !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(groups.issue ?? '')
      : groups.issue !== null) ||
    (extensionData.issue === 'groupsIncomplete' && groups.code !== 'partial')
  )
    return fail()
  const installations = data.installations
  if (
    !installations ||
    installations.version !== 5 ||
    installations.source !== 'moleListRegistryRootsVolumesAndReceipts' ||
    installations.revision !== moleUninstallRevision ||
    !Array.isArray(installations.copies) ||
    installations.copies.length > 32 ||
    new Set(installations.copies.map((copy) => copy?.path)).size !== installations.copies.length ||
    (installations.code === 'unknown'
      ? installations.copies.length !== 0 ||
        !['identityChanged', 'identityUnavailable', 'rulesUnavailable'].includes(
          installations.issue ?? '',
        )
      : !['observed', 'partial'].includes(installations.code) ||
        !data.bundleId ||
        ['different', 'unavailable'].includes(data.identity) ||
        (installations.code === 'observed'
          ? installations.issue !== null
          : ![
              'limitReached',
              'candidateUnavailable',
              'candidatesChanged',
              'registryUnavailable',
              'listUnavailable',
              'rootsIncomplete',
              'volumesIncomplete',
              'receiptsIncomplete',
            ].includes(installations.issue ?? ''))) ||
    (data.identity === 'different' && installations.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && installations.issue !== 'identityUnavailable') ||
    (installations.issue === 'identityChanged' && data.identity !== 'different') ||
    (installations.issue === 'identityUnavailable' && data.identity !== 'unavailable') ||
    installations.copies.some(
      (copy) =>
        !copy ||
        !text(copy.path, 4096) ||
        !copy.path.startsWith('/') ||
        !copy.path.endsWith('.app') ||
        copy.path
          .split('/')
          .some(
            (part, index, parts) =>
              part === '..' ||
              part === '.' ||
              (index < parts.length - 1 && part.toLowerCase().endsWith('.app')),
          ) ||
        !text(copy.bundleId, 255) ||
        !/^[A-Za-z0-9_.-]+$/.test(copy.bundleId) ||
        !['sameIdentifier', 'relatedIdentifier', 'channelName'].includes(copy.relation) ||
        (copy.relation === 'sameIdentifier'
          ? copy.bundleId.toLowerCase() !== data.bundleId?.toLowerCase()
          : copy.bundleId.toLowerCase() === data.bundleId?.toLowerCase()) ||
        (copy.relation !== 'sameIdentifier' &&
          (copy.bundleId.toLowerCase().startsWith(`${data.bundleId?.toLowerCase()}.`) ||
            data.bundleId?.toLowerCase().startsWith(`${copy.bundleId.toLowerCase()}.`)) !==
            (copy.relation === 'relatedIdentifier')) ||
        ![copy.version, copy.build].every((v) => v === null || text(v, 128)),
    )
  )
    return fail()
  const roots = installations.roots
  if (
    !roots ||
    !['observed', 'partial', 'notChecked'].includes(roots.code) ||
    !Number.isInteger(roots.rootCount) ||
    roots.rootCount < 0 ||
    roots.rootCount > 9 ||
    !Number.isInteger(roots.entries) ||
    roots.entries < 0 ||
    roots.entries > 2048 ||
    !Number.isInteger(roots.candidates) ||
    roots.candidates < 0 ||
    roots.candidates > 128 ||
    roots.candidates > roots.entries ||
    !(
      roots.firstUncertainPath === null ||
      (text(roots.firstUncertainPath, 4096) &&
        roots.firstUncertainPath.startsWith('/') &&
        !roots.firstUncertainPath.split('/').some((part) => part === '..' || part === '.'))
    ) ||
    (roots.code === 'partial'
      ? !['unavailable', 'changed', 'limitReached'].includes(roots.issue ?? '') ||
        installations.code !== 'partial'
      : roots.issue !== null || roots.firstUncertainPath !== null) ||
    (roots.code === 'observed' && roots.rootCount !== 9) ||
    (roots.code === 'notChecked' &&
      (roots.rootCount !== 0 || roots.entries !== 0 || roots.candidates !== 0)) ||
    (roots.code === 'notChecked') !== (installations.code === 'unknown') ||
    (installations.issue === 'rootsIncomplete' && roots.code !== 'partial')
  )
    return fail()
  const volumes = installations.volumes
  if (
    !volumes ||
    !['observed', 'partial', 'notChecked'].includes(volumes.code) ||
    !Number.isInteger(volumes.rootCount) ||
    volumes.rootCount < 0 ||
    volumes.rootCount > 64 ||
    !Number.isInteger(volumes.entries) ||
    volumes.entries < 0 ||
    volumes.entries > 2048 ||
    !Number.isInteger(volumes.candidates) ||
    volumes.candidates < 0 ||
    volumes.candidates > 128 ||
    volumes.candidates > volumes.entries ||
    !(
      volumes.firstUncertainPath === null ||
      (text(volumes.firstUncertainPath, 4096) &&
        (volumes.firstUncertainPath === '/Volumes' ||
          volumes.firstUncertainPath.startsWith('/Volumes/')) &&
        !volumes.firstUncertainPath.split('/').some((part) => part === '..' || part === '.'))
    ) ||
    (volumes.code === 'partial'
      ? !['unavailable', 'changed', 'limitReached', 'unsupportedVolume'].includes(
          volumes.issue ?? '',
        ) || installations.code !== 'partial'
      : volumes.issue !== null || volumes.firstUncertainPath !== null) ||
    (volumes.code === 'notChecked' &&
      (volumes.rootCount !== 0 || volumes.entries !== 0 || volumes.candidates !== 0)) ||
    (volumes.code === 'notChecked') !== (installations.code === 'unknown') ||
    (installations.issue === 'volumesIncomplete' && volumes.code !== 'partial')
  )
    return fail()
  const receipts = installations.receipts
  if (
    !receipts ||
    !['observed', 'partial', 'notChecked'].includes(receipts.code) ||
    !Number.isInteger(receipts.packageCount) ||
    receipts.packageCount < 0 ||
    receipts.packageCount > 512 ||
    !Number.isInteger(receipts.checkedCount) ||
    receipts.checkedCount < 0 ||
    receipts.checkedCount > 64 ||
    receipts.checkedCount > receipts.packageCount ||
    !Number.isInteger(receipts.entries) ||
    receipts.entries < 0 ||
    receipts.entries > 32768 ||
    !Number.isInteger(receipts.candidates) ||
    receipts.candidates < 0 ||
    receipts.candidates > 128 ||
    receipts.candidates > receipts.entries ||
    (receipts.checkedCount === 0 && (receipts.entries !== 0 || receipts.candidates !== 0)) ||
    (receipts.code === 'partial'
      ? !['unavailable', 'changed', 'limitReached', 'unsupportedReceipt'].includes(
          receipts.issue ?? '',
        ) || installations.code !== 'partial'
      : receipts.issue !== null) ||
    (receipts.code === 'notChecked' &&
      (receipts.packageCount !== 0 ||
        receipts.checkedCount !== 0 ||
        receipts.entries !== 0 ||
        receipts.candidates !== 0)) ||
    (receipts.code === 'notChecked') !== (installations.code === 'unknown') ||
    (installations.issue === 'receiptsIncomplete' && receipts.code !== 'partial')
  )
    return fail()
  const associations = data.associations
  const locations = [
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
  const nameLocations = [
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
  const spaceLocations = [
    ['nameCompactSupport', 'Library/Application Support', '', ''],
    ['nameCompactCache', 'Library/Caches', '', ''],
    ['nameCompactLogs', 'Library/Logs', '', ''],
    ['nameCompactPreferencesDirectory', 'Library/Preferences', '', ''],
    ['nameCompactPreferences', 'Library/Preferences', '.plist', ''],
    ['nameCompactSavedState', 'Library/Saved Application State', '.savedState', ''],
    ['nameUnderscoreSupport', 'Library/Application Support', '', '_'],
    ['nameHyphenSupport', 'Library/Application Support', '', '-'],
    ['nameUnderscorePreferencesDirectory', 'Library/Preferences', '', '_'],
    ['nameUnderscorePreferences', 'Library/Preferences', '.plist', '_'],
    ['nameHyphenPreferencesDirectory', 'Library/Preferences', '', '-'],
    ['nameHyphenPreferences', 'Library/Preferences', '.plist', '-'],
  ]
  const xdgLocations = [
    ['nameXdgConfig', '.config', 'app_name'],
    ['nameXdgCache', '.cache', 'app_name'],
    ['nameXdgLowerCache', '.cache', 'lowercase_name'],
    ['nameXdgShare', '.local/share', 'app_name'],
    ['nameXdgCompactConfig', '.config', 'lowercase_nospace'],
    ['nameXdgHyphenConfig', '.config', 'lowercase_hyphen'],
    ['nameXdgUnderscoreConfig', '.config', 'lowercase_underscore'],
    ['nameXdgCompactCache', '.cache', 'lowercase_nospace'],
    ['nameXdgHyphenCache', '.cache', 'lowercase_hyphen'],
    ['nameXdgUnderscoreCache', '.cache', 'lowercase_underscore'],
    ['nameXdgCompactShare', '.local/share', 'lowercase_nospace'],
    ['nameXdgHyphenShare', '.local/share', 'lowercase_hyphen'],
    ['nameXdgUnderscoreShare', '.local/share', 'lowercase_underscore'],
  ]
  const channelLocations = [
    ['nameChannelSupport', 'Library/Application Support', ''],
    ['nameChannelCache', 'Library/Caches', ''],
    ['nameChannelLogs', 'Library/Logs', ''],
    ['nameChannelPreferencesDirectory', 'Library/Preferences', ''],
    ['nameChannelPreferences', 'Library/Preferences', '.plist'],
    ['nameChannelSavedState', 'Library/Saved Application State', '.savedState'],
    ['nameChannelXdgConfig', '.config', ''],
    ['nameChannelXdgCache', '.cache', ''],
    ['nameChannelXdgShare', '.local/share', ''],
  ]
  const leafLocations = [
    ['nameLeafSupport', 'Library/Application Support', ''],
    ['nameLeafCache', 'Library/Caches', ''],
    ['nameLeafLogs', 'Library/Logs', ''],
    ['nameLeafPreferences', 'Library/Preferences', '.plist'],
    ['nameLeafSavedState', 'Library/Saved Application State', '.savedState'],
  ]
  const vscodeChannel =
    associations?.names?.code === 'observed' &&
    reverseDns(data.bundleId) &&
    /microsoft.*[vV][sS][cC]ode/.test(data.bundleId)
      ? /[iI]nsiders/.test(data.bundleId)
        ? 'insiders'
        : 'stable'
      : null
  const vscodeLocations =
    vscodeChannel === null
      ? []
      : [
          ['nameVSCodeShipItStable', 'Library/Caches/com.microsoft.VSCode.ShipIt'],
          ['nameVSCodeShipItInsiders', 'Library/Caches/com.microsoft.VSCodeInsiders.ShipIt'],
          ['nameVSCodeExtensions', vscodeChannel === 'insiders' ? '.vscode-insiders' : '.vscode'],
          [
            'nameVSCodeSupport',
            vscodeChannel === 'insiders'
              ? 'Library/Application Support/Code - Insiders'
              : 'Library/Application Support/Code',
          ],
          [
            'nameVSCodeCache',
            vscodeChannel === 'insiders'
              ? 'Library/Caches/com.microsoft.VSCodeInsiders'
              : 'Library/Caches/com.microsoft.VSCode',
          ],
        ]
  const nameKinds = new Set(
    [...nameLocations, ...spaceLocations, ...xdgLocations, ...channelLocations, ...leafLocations]
      .map(([kind]) => kind)
      .concat([
        'nameVSCodeShipItStable',
        'nameVSCodeShipItInsiders',
        'nameVSCodeExtensions',
        'nameVSCodeSupport',
        'nameVSCodeCache',
      ]),
  )
  const names = associations?.names
  const nameEntries = Array.isArray(associations?.entries)
    ? associations.entries.filter((e) => nameKinds.has(e?.kind))
    : []
  const namePaths = new Map<string, string>()
  const cliPaths = new Set<string>()
  const seenNamePaths = new Set<string>()
  let baseName: string | null = null
  let leafNames: string[] = []
  const asciiLower = (name: string) => name.replace(/[A-Z]/g, (char) => char.toLowerCase())
  const echoLower = (name: string) => (/^-[neE]+$/.test(name) ? '' : asciiLower(name))
  const hasSpaceVariants =
    typeof names?.name === 'string' &&
    new TextEncoder().encode(names.name).length > 3 &&
    names.name.includes(' ')
  if (names?.code === 'observed' && typeof names.name === 'string') {
    const add = (kind: string, root: string, suffix: string, name: string) => {
      const path = `${root ? `${root}/` : ''}${name}${suffix}`
      if (seenNamePaths.has(path)) return
      seenNamePaths.add(path)
      if (
        ['Library/Preferences/ByHost', 'Library/Application Support/Epic'].includes(path) ||
        [
          '.cache',
          '.config',
          '.local',
          '.local/bin',
          '.local/lib',
          '.local/share',
          '.local/state',
        ].includes(asciiLower(path.replace(/\/$/, ''))) ||
        locations.some(([, root, suffix]) => path === `${root}/${data.bundleId}${suffix}`)
      )
        return
      if (
        ['.config', '.cache', '.local/share'].includes(root) &&
        ['claude', 'opencode', 'codex', 'gemini'].includes(asciiLower(name.replace(/^\./, '')))
      ) {
        cliPaths.add(path)
        return
      }
      namePaths.set(path, kind)
    }
    for (const [kind, root, suffix] of nameLocations) add(kind, root, suffix, names.name)
    if (hasSpaceVariants)
      for (const [kind, root, suffix, separator] of spaceLocations)
        add(kind, root, suffix, names.name.replaceAll(' ', separator))
    for (const [index, [kind, root, variable]] of xdgLocations.entries()) {
      if (index >= 4 && !hasSpaceVariants) continue
      const derived: Record<string, string> = {
        app_name: names.name,
        lowercase_name: echoLower(names.name),
        lowercase_nospace: echoLower(names.name.replaceAll(' ', '')),
        lowercase_hyphen: echoLower(names.name.replaceAll(' ', '-')),
        lowercase_underscore: echoLower(names.name.replaceAll(' ', '_')),
      }
      add(kind, root, '', derived[variable])
    }
    const suffixes = [
      'Nightly',
      'Beta',
      'Alpha',
      'Dev',
      'Canary',
      'Preview',
      'Insider',
      'Edge',
      'Stable',
      'Release',
      'RC',
      'LTS',
      'Developer Edition',
      'Technology Preview',
    ]
    let prefix = ''
    for (const suffix of suffixes) {
      if (!names.name.endsWith(suffix)) continue
      const candidate = names.name.slice(0, -suffix.length)
      if (
        candidate.length > 1 &&
        /[ \t\n\r\v\f]$/.test(candidate) &&
        candidate.length - 1 > prefix.length
      )
        prefix = candidate.slice(0, -1)
    }
    if (new TextEncoder().encode(prefix).length > 2) baseName = prefix
    if (baseName !== null)
      for (const [kind, root, suffix] of channelLocations)
        add(kind, root, suffix, root.startsWith('.') ? echoLower(baseName) : baseName)
    if (reverseDns(data.bundleId)) {
      const leaf = data.bundleId.split('.').at(-1)!
      const compact = names.name.replaceAll(' ', '')
      if (
        new TextEncoder().encode(names.name).length >= 3 &&
        compact.length >= 3 &&
        leaf.length >= 8 &&
        leaf !== names.name &&
        /[a-z][A-Z]/.test(leaf) &&
        leaf.length > compact.length &&
        asciiLower(leaf).startsWith(asciiLower(compact)) &&
        /^[A-Z0-9]/.test(leaf.slice(compact.length))
      ) {
        const rest = leaf
          .slice(compact.length)
          .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
          .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
        leafNames = [leaf, `${names.name} ${rest}`]
        for (const variant of leafNames)
          for (const [kind, root, suffix] of leafLocations) add(kind, root, suffix, variant)
      }
    }
    for (const [kind, path] of vscodeLocations) {
      const slash = path.lastIndexOf('/')
      add(kind, slash < 0 ? '' : path.slice(0, slash), '', path.slice(slash + 1))
    }
  }
  if (
    !names ||
    names.version !== 8 ||
    names.source !== 'moleAppNameUserPaths' ||
    !['observed', 'notChecked', 'unknown'].includes(names.code) ||
    !Number.isInteger(names.candidates) ||
    names.candidates < 0 ||
    names.candidates > 73 ||
    names.candidates !== nameEntries.length ||
    !Number.isInteger(names.cliExcluded) ||
    names.cliExcluded < 0 ||
    names.cliExcluded > 16 ||
    names.cliExcluded !== cliPaths.size ||
    names.baseName !== baseName ||
    names.vscodeChannel !== vscodeChannel ||
    !Array.isArray(names.leafNames) ||
    JSON.stringify(names.leafNames) !== JSON.stringify(leafNames) ||
    (names.code === 'observed'
      ? associations.code !== 'observed' ||
        names.issue !== null ||
        typeof names.name !== 'string' ||
        names.name !== expectedName ||
        !text(names.name, 240) ||
        new TextEncoder().encode(names.name).length < 2 ||
        new TextEncoder().encode(names.name).length > 240 ||
        /[\u007f-\u009f]/.test(names.name) ||
        names.name.includes('/') ||
        ['.', '..'].includes(names.name) ||
        (hasSpaceVariants && ['', '.', '..'].includes(names.name.replaceAll(' ', ''))) ||
        names.candidates !== namePaths.size
      : names.name !== null ||
        names.candidates !== 0 ||
        (names.code === 'unknown'
          ? associations.code !== 'observed' || names.issue !== 'limitReached'
          : ![
              'identityChanged',
              'identityUnavailable',
              'rulesUnavailable',
              'bundleIdUnsupported',
              'homeUnavailable',
              'nameUnavailable',
              'nameUnsupported',
            ].includes(names.issue ?? ''))) ||
    (associations.code === 'unknown' && names.code !== 'notChecked') ||
    !nameEntries.every((e) => namePaths.get(e.relativePath) === e.kind)
  )
    return fail()
  const vendor = names.vendor
  const vendorRoots: Record<string, string> = {
    vendorSupport: 'Library/Application Support',
    vendorCache: 'Library/Caches',
    vendorLogs: 'Library/Logs',
  }
  const vendorEntries = Array.isArray(associations.entries)
    ? associations.entries.filter((e) => Object.hasOwn(vendorRoots, e?.kind))
    : []
  const commonNames =
    'music notes photos finder safari preview calendar contacts messages reminders clock weather stocks books news podcasts voice files store system helper agent daemon service update sync backup cloud manager monitor server client worker runner launcher driver plugin extension widget utility'.split(
      ' ',
    )
  const idParts = reverseDns(data.bundleId) ? data.bundleId.split('.') : []
  const vendorToken = idParts.at(-2)?.toLowerCase() ?? ''
  const productToken = idParts.at(-1)?.toLowerCase() ?? ''
  const vendorEligible =
    names.code === 'observed' &&
    typeof names.name === 'string' &&
    new TextEncoder().encode(names.name).length >= 4 &&
    !commonNames.includes(asciiLower(names.name)) &&
    vendorToken.length >= 3 &&
    productToken.length >= 3
  const vendorPath = (entry: (typeof vendorEntries)[number]) => {
    const root = vendorRoots[entry.kind]
    if (
      entry.status !== 'present' ||
      entry.issue !== null ||
      !root ||
      !text(entry.relativePath, 1024) ||
      new TextEncoder().encode(entry.relativePath).length > 1024 ||
      !entry.relativePath.startsWith(`${root}/`)
    )
      return false
    const parts = entry.relativePath.slice(root.length + 1).split('/')
    if (
      parts.length !== 2 ||
      parts.some((p) => !p || p === '.' || p === '..' || /[\u007f-\u009f]/.test(p)) ||
      asciiLower(parts[0]) !== vendorToken
    )
      return false
    const name = asciiLower(names.name ?? '')
    const child = asciiLower(parts[1])
    return (
      [
        name,
        name.replaceAll(' ', ''),
        name.replaceAll(' ', '-'),
        name.replaceAll(' ', '_'),
        productToken,
      ].some(
        (variant) =>
          variant &&
          (child === variant ||
            [' ', '-', '_', '.'].some((separator) => child.startsWith(`${variant}${separator}`))),
      ) &&
      !namePaths.has(entry.relativePath) &&
      !locations.some(
        ([, root, suffix]) => entry.relativePath === `${root}/${data.bundleId}${suffix}`,
      )
    )
  }
  if (
    !vendor ||
    vendor.version !== 1 ||
    vendor.source !== 'moleVendorNestedUserPaths' ||
    !['observed', 'partial', 'notChecked'].includes(vendor.code) ||
    !Number.isInteger(vendor.rootCount) ||
    vendor.rootCount < 0 ||
    vendor.rootCount > 3 ||
    !Number.isInteger(vendor.entries) ||
    vendor.entries < 0 ||
    vendor.entries > 1024 ||
    !Number.isInteger(vendor.candidates) ||
    vendor.candidates < 0 ||
    vendor.candidates > 64 ||
    vendor.candidates > vendor.entries ||
    vendor.candidates !== vendorEntries.length ||
    !vendorEntries.every(vendorPath) ||
    (vendor.code === 'notChecked'
      ? vendor.vendorToken !== null ||
        vendor.productToken !== null ||
        vendor.rootCount !== 0 ||
        vendor.entries !== 0 ||
        vendor.candidates !== 0 ||
        vendor.firstUncertainRoot !== null ||
        ![
          'identityChanged',
          'identityUnavailable',
          'rulesUnavailable',
          'bundleIdUnsupported',
          'homeUnavailable',
          'nameUnavailable',
          'nameUnsupported',
          'notApplicable',
          'limitReached',
        ].includes(vendor.issue ?? '')
      : !vendorEligible ||
        vendor.vendorToken !== vendorToken ||
        vendor.productToken !== productToken ||
        (vendor.code === 'observed'
          ? vendor.issue !== null || vendor.rootCount !== 3 || vendor.firstUncertainRoot !== null
          : vendor.candidates !== 0 ||
            !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(
              vendor.issue ?? '',
            ) ||
            !Object.values(vendorRoots).includes(vendor.firstUncertainRoot ?? ''))) ||
    (names.code !== 'observed' && vendor.code !== 'notChecked') ||
    (vendor.issue === 'notApplicable' && vendorEligible)
  )
    return fail()
  const relatedRoots: Record<string, string> = {
    groupContainer: 'Library/Group Containers',
    derivedScripts: 'Library/Application Scripts',
    derivedContainer: 'Library/Containers',
    fileProvider: 'Library/Application Support/FileProvider',
  }
  const relatedPath = (entry: ApplicationInspection['associations']['entries'][number]) => {
    const root = Object.hasOwn(relatedRoots, entry.kind) ? relatedRoots[entry.kind] : null
    if (!root || !text(entry.relativePath, 1024) || /[\u007f-\u009f]/.test(entry.relativePath))
      return false
    const prefix = `${root}/`
    if (!entry.relativePath.startsWith(prefix)) return false
    const name = entry.relativePath.slice(prefix.length)
    const id = data.bundleId ?? ''
    return (
      !name.includes('/') &&
      name !== '.' &&
      name !== '..' &&
      (name === id ||
        name.startsWith(`${id}.`) ||
        name.endsWith(`.${id}`) ||
        name.includes(`.${id}.`)) &&
      !locations.some(([, path, suffix]) => entry.relativePath === `${path}/${id}${suffix}`)
    )
  }
  const mainByHostPath = (path: unknown) => {
    if (!text(path, 1024) || /[\u007f-\u009f]/.test(path as string)) return false
    const root = 'Library/Preferences/ByHost/'
    if (!(path as string).startsWith(root)) return false
    const name = (path as string).slice(root.length)
    return !name.includes('/') && name.startsWith(`${data.bundleId}.`) && name.endsWith('.plist')
  }
  const mainByHost = associations?.byHost
  const mainByHostCount = Array.isArray(associations?.entries)
    ? associations.entries.filter((e) => e?.kind === 'byHost').length
    : 0
  if (
    !mainByHost ||
    !['observed', 'partial', 'notChecked'].includes(mainByHost.code) ||
    !Number.isInteger(mainByHost.entries) ||
    mainByHost.entries < 0 ||
    mainByHost.entries > 1024 ||
    !Number.isInteger(mainByHost.candidates) ||
    mainByHost.candidates < 0 ||
    mainByHost.candidates > 64 ||
    mainByHost.candidates > mainByHost.entries ||
    mainByHost.candidates !== mainByHostCount ||
    (mainByHost.code === 'notChecked') !== (associations.code === 'unknown') ||
    (mainByHost.code === 'notChecked' &&
      (mainByHost.entries !== 0 || mainByHost.candidates !== 0)) ||
    (mainByHost.code === 'partial'
      ? !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(mainByHost.issue ?? '')
      : mainByHost.issue !== null)
  )
    return fail()
  if (
    !associations ||
    associations.version !== 13 ||
    associations.revision !== moleUninstallRevision ||
    !Array.isArray(associations.entries) ||
    (associations.code === 'unknown'
      ? associations.entries.length !== 0 ||
        ![
          'identityChanged',
          'identityUnavailable',
          'rulesUnavailable',
          'bundleIdUnsupported',
          'homeUnavailable',
        ].includes(associations.issue ?? '')
      : associations.code !== 'observed' ||
        associations.issue !== null ||
        ['different', 'unavailable'].includes(data.identity) ||
        !data.bundleId ||
        !/^[A-Za-z0-9][A-Za-z0-9-]*(\.[A-Za-z0-9][A-Za-z0-9-]*)+$/.test(data.bundleId) ||
        associations.entries.length < locations.length ||
        associations.entries.length > locations.length + 265) ||
    (data.identity === 'different' && associations.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && associations.issue !== 'identityUnavailable') ||
    (associations.issue === 'identityChanged' && data.identity !== 'different') ||
    (associations.issue === 'identityUnavailable' && data.identity !== 'unavailable') ||
    associations.entries.some(
      (entry, index) =>
        !entry ||
        !pathProtection(entry) ||
        (index < locations.length
          ? entry.kind !== locations[index]?.[0] ||
            entry.relativePath !==
              `${locations[index]?.[1]}/${data.bundleId}${locations[index]?.[2]}`
          : nameKinds.has(entry.kind)
            ? namePaths.get(entry.relativePath) !== entry.kind
            : Object.hasOwn(vendorRoots, entry.kind)
              ? !vendorPath(entry)
              : entry.kind === 'byHost'
                ? !mainByHostPath(entry.relativePath)
                : !relatedPath(entry)) ||
        !Array.isArray(entry.protectionReasons) ||
        entry.protectionReasons.length > 3 ||
        new Set(entry.protectionReasons).size !== entry.protectionReasons.length ||
        entry.protectionReasons.some(
          (reason) => !['systemLocation', 'currentApplication', 'clientDirectory'].includes(reason),
        ) ||
        !['present', 'notFound', 'unknown'].includes(entry.status) ||
        (entry.status === 'unknown'
          ? !['unavailable', 'unsupported', 'changed'].includes(entry.issue ?? '')
          : entry.issue !== null),
    )
  )
    return fail()
  const discovery = associations.discovery
  if (
    !discovery ||
    !['observed', 'partial', 'notChecked'].includes(discovery.code) ||
    !Number.isInteger(discovery.rootCount) ||
    discovery.rootCount < 0 ||
    discovery.rootCount > 4 ||
    !Number.isInteger(discovery.entries) ||
    discovery.entries < 0 ||
    discovery.entries > 1024 ||
    !Number.isInteger(discovery.candidates) ||
    discovery.candidates < 0 ||
    discovery.candidates > 64 ||
    discovery.candidates > discovery.entries ||
    new Set(associations.entries.map((entry) => entry.relativePath)).size !==
      associations.entries.length ||
    (discovery.code === 'notChecked') !== (associations.code === 'unknown') ||
    (discovery.code === 'notChecked'
      ? discovery.rootCount !== 0 || discovery.entries !== 0 || discovery.candidates !== 0
      : discovery.candidates !==
        associations.entries.length -
          locations.length -
          mainByHostCount -
          nameEntries.length -
          vendorEntries.length) ||
    (discovery.code === 'observed' && discovery.rootCount !== 4) ||
    (discovery.code === 'partial'
      ? !['unavailable', 'unsupported', 'changed', 'limitReached'].includes(
          discovery.issue ?? '',
        ) || !Object.values(relatedRoots).includes(discovery.firstUncertainRoot ?? '')
      : discovery.issue !== null || discovery.firstUncertainRoot !== null)
  )
    return fail()
  const background = data.background
  if (
    !background ||
    background.version !== 1 ||
    background.revision !== moleUninstallRevision ||
    !['notRequired', 'directRoot', 'restricted', 'unknown'].includes(background.code) ||
    !['absent', 'enabled', 'disabled', 'unknown'].includes(background.marker) ||
    (background.code === 'unknown'
      ? background.rootKind !== null ||
        background.reason !== null ||
        ![
          'identityChanged',
          'identityUnavailable',
          'rulesUnavailable',
          'markerUnknown',
          'rootsUnavailable',
          'rootsChanged',
        ].includes(background.issue ?? '')
      : background.issue !== null ||
        !data.bundleId ||
        ['different', 'unavailable'].includes(data.identity)) ||
    (background.code === 'notRequired' &&
      (!['absent', 'disabled'].includes(background.marker) ||
        background.rootKind !== null ||
        background.reason !== null)) ||
    (background.code === 'directRoot' &&
      (background.marker !== 'enabled' ||
        background.reason !== null ||
        ![
          'systemApplications',
          'userApplications',
          'systemInputMethods',
          'userInputMethods',
          'playCover',
          'externalApplications',
        ].includes(background.rootKind ?? ''))) ||
    (background.code === 'restricted' &&
      (background.marker !== 'enabled' ||
        background.rootKind !== null ||
        !['outsideRoots', 'unreadableRoot', 'duplicateRoot'].includes(background.reason ?? ''))) ||
    (background.issue === 'markerUnknown' && background.marker !== 'unknown') ||
    (['rootsUnavailable', 'rootsChanged'].includes(background.issue ?? '') &&
      background.marker !== 'enabled') ||
    (data.identity === 'different' && background.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && background.issue !== 'identityUnavailable') ||
    (background.issue === 'identityChanged' && data.identity !== 'different') ||
    (background.issue === 'identityUnavailable' && data.identity !== 'unavailable')
  )
    return fail()
  const official = data.officialUninstaller
  if (
    !official ||
    official.version !== 1 ||
    official.revision !== moleUninstallRevision ||
    !['officialRequired', 'notMatched', 'unknown'].includes(official.code) ||
    (official.code === 'unknown'
      ? official.vendor !== null ||
        !['identityChanged', 'identityUnavailable', 'rulesUnavailable', 'nameUnavailable'].includes(
          official.issue ?? '',
        )
      : official.issue !== null ||
        !data.bundleId ||
        ['different', 'unavailable'].includes(data.identity)) ||
    (official.code === 'officialRequired'
      ? !officialVendors.includes(official.vendor ?? '')
      : official.vendor !== null) ||
    (data.identity === 'different' && official.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && official.issue !== 'identityUnavailable') ||
    (official.issue === 'identityChanged' && data.identity !== 'different') ||
    (official.issue === 'identityUnavailable' && data.identity !== 'unavailable')
  )
    return fail()
  const mole = data.moleProtection
  if (
    !mole ||
    mole.version !== 1 ||
    mole.revision !== moleUninstallRevision ||
    !['protected', 'notMatched', 'unknown'].includes(mole.code) ||
    (mole.code === 'unknown'
      ? !['identityChanged', 'identityUnavailable', 'rulesUnavailable'].includes(mole.issue ?? '')
      : mole.issue !== null ||
        !data.bundleId ||
        ['different', 'unavailable'].includes(data.identity)) ||
    (data.identity === 'different' && mole.issue !== 'identityChanged') ||
    (data.identity === 'unavailable' && mole.issue !== 'identityUnavailable') ||
    (mole.issue === 'identityChanged' && data.identity !== 'different') ||
    (mole.issue === 'identityUnavailable' && data.identity !== 'unavailable')
  )
    return fail()
  const protection = data.protection
  if (
    !protection ||
    !Array.isArray(protection.reasons) ||
    protection.reasons.length > 3 ||
    new Set(protection.reasons).size !== protection.reasons.length ||
    !protection.reasons.every((reason) =>
      ['systemLocation', 'currentApplication', 'clientDirectory'].includes(reason),
    ) ||
    !Array.isArray(protection.matchedPaths) ||
    protection.matchedPaths.length > 100 ||
    new Set(protection.matchedPaths).size !== protection.matchedPaths.length ||
    !protection.matchedPaths.every(
      (path) => text(path, 4096) && path.startsWith('/') && !path.split('/').includes('..'),
    ) ||
    protection.reasons.includes('clientDirectory') !== protection.matchedPaths.length > 0
  )
    return fail()
  if (
    (data.identity === 'matches' && (!expectedId || data.bundleId !== expectedId)) ||
    (data.identity === 'different' &&
      (!expectedId || !data.bundleId || data.bundleId === expectedId)) ||
    (data.identity === 'unreported' && (expectedId || !data.bundleId)) ||
    (data.identity === 'unavailable' && data.bundleId !== null)
  )
    return fail()
  const running = data.running
  if (
    ['different', 'unavailable'].includes(data.identity) &&
    (running?.code !== 'unknown' || running?.otherCopies !== 0)
  )
    return fail()
  if (
    !running ||
    !['observed', 'notObserved', 'unknown'].includes(running.code) ||
    !Array.isArray(running.pids) ||
    running.pids.length > 32 ||
    new Set(running.pids).size !== running.pids.length ||
    !running.pids.every((pid) => Number.isSafeInteger(pid) && pid > 0 && pid <= 2147483647) ||
    !Number.isSafeInteger(running.otherCopies) ||
    running.otherCopies < 0 ||
    running.otherCopies > 32 ||
    !text(running.reason, 512) ||
    (running.code === 'observed'
      ? !running.pids.length || !data.bundleId || data.identity === 'different'
      : running.pids.length > 0)
  )
    return fail()
  return data
}
