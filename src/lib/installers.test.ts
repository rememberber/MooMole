import { expect, it } from 'vitest'
import {
  installersReport,
  installerZipInspection,
  installerZipDataInspection,
  installerConnectionInspection,
  installerUsageInspection,
  installerImageInspection,
} from '../../tests/e2e/fixtures/installers'
import {
  readInstallers,
  installerLocations,
  installerView,
  installerIncompleteReason,
  readInstallerZip,
  readInstallerZipData,
  zipDataInspectionText,
  zipInspectionText,
  readInstallerConnection,
  connectionInspectionText,
  readInstallerUsage,
  usageInspectionText,
  readInstallerImage,
  imageInspectionText,
} from './installers'
it('binds installer metadata to scope, time, supported names and non-executable state', () => {
  const fixture = installersReport()
  expect(readInstallers(fixture, fixture.root, fixture.checkedAt, fixture.checkedAt)).toEqual(
    fixture,
  )
  for (const change of [
    (r: typeof fixture) => {
      r.protocol = 2 as 1
    },
    (r: typeof fixture) => {
      r.revision = 'other'
    },
    (r: typeof fixture) => {
      r.root = '/fixture/other'
    },
    (r: typeof fixture) => {
      r.checkedAt -= 6000
    },
    (r: typeof fixture) => {
      r.canExecute = true as false
    },
    (r: typeof fixture) => {
      r.visited = 10001
    },
    (r: typeof fixture) => {
      r.skipped.links = 1
    },
    (r: typeof fixture) => {
      r.items[0].path = '/fixture/downloads-old/Tool.dmg'
    },
    (r: typeof fixture) => {
      r.items[0].path = '/fixture/downloads/a/b/Tool.dmg'
    },
    (r: typeof fixture) => {
      r.items[0].sizeBytes = Number.MAX_SAFE_INTEGER + 1
    },
    (r: typeof fixture) => {
      r.items[0].measurement = 'unavailable'
    },
    (r: typeof fixture) => {
      r.items[0].usage = 'safe' as 'notChecked'
    },
    (r: typeof fixture) => {
      r.items[0].canExecute = true as false
    },
    (r: typeof fixture) => {
      r.items[1].classification = 'nameOnly'
    },
    (r: typeof fixture) => {
      r.items[1].sizeBytes = 0
    },
    (r: typeof fixture) => {
      r.items.push(r.items[0])
    },
  ]) {
    const r = structuredClone(fixture)
    change(r)
    expect(() => readInstallers(r, fixture.root, fixture.checkedAt, fixture.checkedAt)).toThrow()
  }
})
it('preserves zero-size files, unknown sizes and unverified zip content as different observations', () => {
  const r = installersReport()
  r.items[0].sizeBytes = 0
  const value = readInstallers(r, r.root, r.checkedAt, r.checkedAt)
  expect(value.items[0].sizeBytes).toBe(0)
  expect(value.items[1].sizeBytes).toBeNull()
  expect(value.items[1].classification).toBe('archiveUnverified')
})
it('retains empty partial inventories and explains actual missing coverage without implying absence', () => {
  const r = installersReport()
  r.items = []
  r.completion = 'partial'
  r.skipped.depth = 2
  r.skipped.unsupportedType = 1
  r.stopReason = 'entryLimit'
  expect(readInstallers(r, r.root, r.checkedAt, r.checkedAt).items).toEqual([])
  expect(installerIncompleteReason(r)).toBe(
    '达到层级限制 2 次；包目录或特殊文件未支持 1 次；达到子项数量限制',
  )
})

it('binds ZIP hints to the scope and complete first-fifty evidence without admitting execution', () => {
  const fixture = installerZipInspection()
  expect(
    readInstallerZip(fixture, fixture.root, fixture.path, fixture.checkedAt, fixture.checkedAt),
  ).toEqual(fixture)
  expect(() =>
    readInstallerZip({ ...fixture, protocol: 1 }, fixture.root, fixture.path, fixture.checkedAt),
  ).toThrow()
  expect(() =>
    readInstallerZip(
      { ...fixture, fileStamp: 'bad' },
      fixture.root,
      fixture.path,
      fixture.checkedAt,
    ),
  ).toThrow()
  for (const change of [
    (r: typeof fixture) => {
      r.path = '/fixture/downloads-old/a.zip'
    },
    (r: typeof fixture) => {
      r.root = '/fixture/other'
    },
    (r: typeof fixture) => {
      r.checkedAt -= 6000
    },
    (r: typeof fixture) => {
      r.canExecute = true as false
    },
    (r: typeof fixture) => {
      r.state = 'unknown'
    },
    (r: typeof fixture) => {
      r.checkedEntries = 49
    },
    (r: typeof fixture) => {
      r.totalEntries = null
    },
    (r: typeof fixture) => {
      r.totalEntries = 10001
    },
    (r: typeof fixture) => {
      r.matchedTypes = []
    },
    (r: typeof fixture) => {
      r.matchedTypes = ['exe']
    },
    (r: typeof fixture) => {
      r.matchedTypes = ['app', 'app']
    },
    (r: typeof fixture) => {
      r.reason = 'listingUnavailable'
    },
  ]) {
    const r = structuredClone(fixture)
    change(r)
    expect(() =>
      readInstallerZip(r, fixture.root, fixture.path, fixture.checkedAt, fixture.checkedAt),
    ).toThrow()
  }
})
it('keeps a complete no-match window distinct from unreadable or truncated ZIP listings', () => {
  const r = installerZipInspection()
  r.state = 'noHintInWindow'
  r.matchedTypes = []
  expect(zipInspectionText(readInstallerZip(r, r.root, r.path, r.checkedAt, r.checkedAt))).toBe(
    '前 50 条未见安装线索',
  )
  r.state = 'unknown'
  r.reason = 'listingLimit'
  r.checkedEntries = 0
  r.totalEntries = null
  expect(readInstallerZip(r, r.root, r.path, r.checkedAt, r.checkedAt).state).toBe('unknown')
  expect(zipInspectionText(r)).toBe('条目列表超出检查限制')
})

it('resolves all pinned Mole locations literally without probing, adding or broadening scope', () => {
  const locations = installerLocations('/Users/Fixture User')
  expect(locations).toHaveLength(12)
  expect(locations.find((l) => l.id === 'downloads')?.path).toBe('/Users/Fixture User/Downloads')
  expect(locations.find((l) => l.id === 'shared')?.path).toBe('/Users/Shared')
  expect(locations.find((l) => l.id === 'icloud')?.path).toBe(
    '/Users/Fixture User/Library/Mobile Documents/com~apple~CloudDocs/Downloads',
  )
  expect(locations.some((l) => l.path.includes('Library/Updates'))).toBe(false)
  for (const home of ['', '/', 'relative', '/Users/../tmp', '/Users/name\n'])
    expect(installerLocations(home)).toEqual([])
})
it('filters name or path and sorts known sizes above unknowns without removing protected rows or mutating inventory', () => {
  const report = installersReport()
  report.items.push({
    ...report.items[0],
    name: 'empty.PKG',
    path: '/fixture/downloads/sub/empty.PKG',
    extension: 'pkg',
    sizeBytes: 0,
  })
  const before = structuredClone(report.items)
  expect(installerView(report.items, '', '', 'size').map((i) => i.sizeBytes)).toEqual([
    1048576,
    0,
    null,
  ])
  expect(installerView(report.items, 'ARCHIVE', 'zip', 'size')[0].protection).toBe('matched')
  expect(installerView(report.items, '/SUB/', 'pkg', 'name').map((i) => i.name)).toEqual([
    'empty.PKG',
  ])
  expect(installerView(report.items, 'missing', '', 'size')).toEqual([])
  expect(report.items).toEqual(before)
})

it('connection observations bind exact scope and time and never infer safe removal from an unlisted path', () => {
  const fixture = installerConnectionInspection()
  const read = (r: typeof fixture) =>
    readInstallerConnection(r, fixture.root, fixture.path, fixture.checkedAt, fixture.checkedAt)
  expect(read(fixture)).toEqual(fixture)
  expect(connectionInspectionText(fixture)).toContain('请先保留')
  for (const patch of [
    { protocol: 2 },
    { revision: 'other' },
    { root: '/else' },
    { path: '/else/Tool.dmg' },
    { checkedAt: fixture.checkedAt - 6000 },
    { checkedAt: NaN },
    { canExecute: true },
    { state: 'safe' },
    { state: 'unknown' },
    { reason: 'listingLimit' },
  ])
    expect(() => read({ ...fixture, ...patch } as typeof fixture)).toThrow()
  const negative = { ...fixture, state: 'pathNotListed' as const }
  expect(connectionInspectionText(read(negative))).toBe('本次连接列表未见此路径；文件占用仍未确认')
  for (const reason of ['queryUnavailable', 'listingUncertain', 'listingLimit'] as const) {
    const r = { ...fixture, state: 'unknown' as const, reason }
    expect(connectionInspectionText(read(r))).toContain('无法确认')
  }
  for (const path of ['/fixture/downloads/a/b/Tool.dmg', '/fixture/downloads/Tool.zip']) {
    expect(() =>
      readInstallerConnection({ ...fixture, path }, fixture.root, path, fixture.checkedAt),
    ).toThrow()
  }
})

it('usage evidence admits only bound busy or explicit unknown and never accepts idle claims', () => {
  const fixture = installerUsageInspection()
  const read = (r: typeof fixture) =>
    readInstallerUsage(r, fixture.root, fixture.path, fixture.checkedAt, fixture.checkedAt)
  expect(read(fixture)).toEqual(fixture)
  expect(usageInspectionText(fixture)).toContain('请先保留')
  for (const patch of [
    { protocol: 2 },
    { revision: 'other' },
    { root: '/other' },
    { path: '/other/Tool.dmg' },
    { checkedAt: fixture.checkedAt - 6000 },
    { checkedAt: Infinity },
    { canExecute: true },
    { state: 'idle' },
    { state: 'noOpenHandles' },
    { state: 'unknown' },
    { reason: 'limitedView' },
  ])
    expect(() => read({ ...fixture, ...patch } as typeof fixture)).toThrow()
  for (const reason of [
    'limitedView',
    'queryUnavailable',
    'listingUncertain',
    'listingLimit',
  ] as const) {
    expect(usageInspectionText(read({ ...fixture, state: 'unknown', reason }))).toContain('确认')
  }
  expect(usageInspectionText({ ...fixture, state: 'unknown', reason: 'limitedView' })).toContain(
    '视图有限，占用仍未确认',
  )
  for (const path of ['/fixture/downloads/a/b/Tool.dmg', '/fixture/downloads/Tool.txt']) {
    expect(() =>
      readInstallerUsage({ ...fixture, path }, fixture.root, path, fixture.checkedAt),
    ).toThrow()
  }
})

it('ZIP data tests bind scope and time, distinguish passed from unknown, and never authorize execution', () => {
  const fixture = installerZipDataInspection()
  const read = (r: typeof fixture) =>
    readInstallerZipData(
      r,
      fixture.root,
      fixture.path,
      fixture.checkedAt,
      fixture.fileStamp,
      fixture.checkedAt,
    )
  expect(read(fixture)).toEqual(fixture)
  expect(zipDataInspectionText(fixture)).toBe('ZIP 数据测试通过；签名与用途未确认')
  for (const patch of [
    { protocol: 2 },
    { revision: 'other' },
    { fileStamp: 'b'.repeat(64) },
    { root: '/other' },
    { path: '/other/a.zip' },
    { checkedAt: fixture.checkedAt - 6000 },
    { checkedAt: Infinity },
    { canExecute: true },
    { state: 'safe' },
    { state: 'corrupt' },
    { state: 'unknown' },
    { reason: 'testUnavailable' },
  ])
    expect(() => read({ ...fixture, ...patch } as typeof fixture)).toThrow()
  for (const reason of ['fileSizeLimit', 'testUnavailable', 'unexpectedOutput'] as const) {
    const r = read({ ...fixture, state: 'unknown', reason })
    expect(r.canExecute).toBe(false)
    expect(zipDataInspectionText(r)).not.toContain('测试通过')
  }
  for (const path of ['/fixture/downloads/a/b/test.zip', '/fixture/downloads/test.dmg'])
    expect(() =>
      readInstallerZipData(
        { ...fixture, path },
        fixture.root,
        path,
        fixture.checkedAt,
        fixture.fileStamp,
      ),
    ).toThrow()
})

it('DMG checksum observations bind scope and time without admitting trust, execution or unrecognized reports', () => {
  const fixture = installerImageInspection()
  const read = (r: typeof fixture) =>
    readInstallerImage(r, fixture.root, fixture.path, fixture.checkedAt, fixture.checkedAt)
  expect(read(fixture)).toEqual(fixture)
  expect(imageInspectionText(fixture)).toBe('DMG 内部校验和匹配；签名与用途未确认')
  for (const patch of [
    { protocol: 2 },
    { revision: 'other' },
    { root: '/other' },
    { path: '/other/a.dmg' },
    { fileStamp: 'bad' },
    { checkedAt: fixture.checkedAt - 6000 },
    { checkedAt: NaN },
    { canExecute: true },
    { state: 'safe' },
    { state: 'corrupt' },
    { state: 'unknown' },
    { reason: 'checksumUnconfirmed' },
  ])
    expect(() => read({ ...fixture, ...patch } as typeof fixture)).toThrow()
  for (const reason of [
    'fileSizeLimit',
    'verificationUnavailable',
    'checksumUnconfirmed',
    'reportUncertain',
  ] as const) {
    const r = read({ ...fixture, state: 'unknown', reason })
    expect(r.canExecute).toBe(false)
    expect(imageInspectionText(r)).not.toContain('匹配；签名')
  }
  for (const path of ['/fixture/downloads/a/b/test.dmg', '/fixture/downloads/test.iso'])
    expect(() =>
      readInstallerImage({ ...fixture, path }, fixture.root, path, fixture.checkedAt),
    ).toThrow()
})
