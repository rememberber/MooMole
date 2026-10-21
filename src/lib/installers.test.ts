import { expect, it } from 'vitest'
import { installersReport, installerZipInspection } from '../../tests/e2e/fixtures/installers'
import {
  readInstallers,
  installerIncompleteReason,
  readInstallerZip,
  zipInspectionText,
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
