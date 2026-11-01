import { describe, expect, it } from 'vitest'
import {
  browserHistoryPath,
  defaultBrowserProfile,
  readBrowserProfile,
  readBrowserProfiles,
  sameBrowserProfile,
} from './browser-downloads'
import { readPersonalSnapshot } from './personal-files'
const home = '/Users/demo'
const selection = { browser: 'edge', profile: 'Profile 2' } as const
const profile = {
  selection,
  historyPath: `${home}/Library/Application Support/Microsoft Edge/Profile 2/History`,
}
describe('browser download source boundaries', () => {
  it('binds Firefox Places paths to one explicit profile without relaxing Chromium names', () => {
    const selection = { browser: 'firefox', profile: 'abcd1234.default-release' } as const
    const historyPath = `${home}/Library/Application Support/Firefox/Profiles/abcd1234.default-release/places.sqlite`
    expect(browserHistoryPath(home, selection)).toBe(historyPath)
    expect(
      readBrowserProfiles(
        { browser: 'firefox', profiles: [{ selection, historyPath }], complete: true, skipped: 0 },
        'firefox',
        home,
      ).profiles,
    ).toEqual([{ selection, historyPath }])
    expect(readBrowserProfile({ browser: 'firefox', profile: '私人 工作' }).profile).toBe(
      '私人 工作',
    )
    for (const profile of [
      '',
      '.hidden',
      'bad.',
      '../other',
      'a/b',
      'a\\b',
      'a:b',
      ' a',
      'a\n',
      '界'.repeat(43),
    ])
      expect(() => readBrowserProfile({ browser: 'firefox', profile })).toThrow()
    expect(() => readBrowserProfile({ browser: 'chrome', profile: selection.profile })).toThrow()
    expect(() =>
      readBrowserProfiles(
        {
          browser: 'firefox',
          profiles: [{ selection, historyPath: historyPath.replace('places.sqlite', 'History') }],
          complete: true,
          skipped: 0,
        },
        'firefox',
        home,
      ),
    ).toThrow()
    expect(sameBrowserProfile(selection, { browser: 'firefox', profile: 'efgh5678.work' })).toBe(
      false,
    )
  })
  it('accepts only exact supported profile names and directories within the selected browser', () => {
    expect(
      readBrowserProfiles(
        { browser: 'edge', profiles: [profile], complete: false, skipped: 1 },
        'edge',
        home,
      ).profiles,
    ).toEqual([profile])
    for (const malformed of [
      { ...selection, profile: '../Default' },
      { ...selection, profile: 'Profile 01' },
      { ...selection, profile: 'Guest Profile' },
      { ...selection, profile: 'Profile 2\n' },
      { ...selection, browser: 'safari' },
      { ...selection, root: '/other' },
    ])
      expect(() => readBrowserProfile(malformed)).toThrow()
    for (const malformed of [
      { browser: 'chrome', profiles: [profile] },
      { profiles: [profile, profile] },
      {
        profiles: [
          { ...profile, historyPath: profile.historyPath.replace('Profile 2', 'Default') },
        ],
      },
      { profiles: [{ ...profile, selection: { browser: 'brave', profile: 'Profile 2' } }] },
      { skipped: -1 },
    ]) {
      expect(() =>
        readBrowserProfiles(
          { browser: 'edge', profiles: [profile], complete: true, skipped: 0, ...malformed },
          'edge',
          home,
        ),
      ).toThrow()
    }
    expect(browserHistoryPath(home, defaultBrowserProfile)).toBe(
      `${home}/Library/Application Support/Google/Chrome/Default/History`,
    )
    expect(sameBrowserProfile(null, selection)).toBe(false)
    expect(sameBrowserProfile(selection, { ...selection, browser: 'chrome' })).toBe(false)
  })
  it('binds browser profiles to download reports while allowing historical default-source reports', () => {
    const now = Date.now(),
      id = '12345678-1234-4123-8123-123456789abc'
    const report = {
      id,
      root: home,
      createdAt: now,
      expiresAt: now + 900000,
      complete: true,
      cancelled: false,
      skipped: 0,
      visited: 0,
      items: [],
      softwareContent: {
        source: 'chromeDownloads',
        minimumAgeDays: 0,
        minimumDirectoryBytes: 0,
        minimumSizeMet: true,
        executionSupported: true,
        browserProfile: selection,
      },
    }
    const snapshot = { phase: 'ready', requestId: id, runId: null, report, error: null }
    expect(readPersonalSnapshot(snapshot).report?.softwareContent?.browserProfile).toEqual(
      selection,
    )
    const legacy = structuredClone(snapshot)
    delete (legacy.report.softwareContent as any).browserProfile
    expect(readPersonalSnapshot(legacy).report?.softwareContent?.browserProfile).toBeUndefined()
    for (const browserProfile of [
      null,
      { browser: 'edge', profile: '../Default' },
      { browser: 'edge', profile: 'Default', path: '/untrusted' },
    ]) {
      const changed = structuredClone(snapshot)
      ;(changed.report.softwareContent as any).browserProfile = browserProfile
      expect(() => readPersonalSnapshot(changed)).toThrow()
    }
  })
})
