import { expect, it } from 'vitest'
import {
  installersReport,
  installerZipInspection,
  installerConnectionInspection,
} from '../../tests/e2e/fixtures/installers'
import {
  readInstallers,
  installerLocations,
  installerView,
  installerIncompleteReason,
  readInstallerZip,
  zipInspectionText,
  readInstallerConnection,
  connectionInspectionText,
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
