import { invoke } from '@tauri-apps/api/core'

export const downloadBrowsers = [
  { id: 'chrome', label: 'Google Chrome', directory: 'Google/Chrome' },
  { id: 'chromium', label: 'Chromium', directory: 'Chromium' },
  { id: 'edge', label: 'Microsoft Edge', directory: 'Microsoft Edge' },
  { id: 'brave', label: 'Brave', directory: 'BraveSoftware/Brave-Browser' },
  { id: 'firefox', label: 'Firefox', directory: 'Firefox/Profiles' },
  { id: 'safari', label: 'Safari', directory: '' },
] as const
export type DownloadBrowser = (typeof downloadBrowsers)[number]['id']
export interface BrowserProfile {
  browser: DownloadBrowser
  profile: string
}
export const defaultBrowserProfile: BrowserProfile = { browser: 'chrome', profile: 'Default' }
export function readBrowserProfile(value: unknown): BrowserProfile {
  if (!value || typeof value !== 'object') throw Error('浏览器配置返回格式无法核对。')
  const v = value as Record<string, unknown>
  if (
    Object.keys(v).some((k) => !['browser', 'profile'].includes(k)) ||
    !downloadBrowsers.some((b) => b.id === v.browser) ||
    typeof v.profile !== 'string' ||
    !(v.browser === 'safari'
      ? ['Standard', 'Sandbox'].includes(v.profile)
      : v.browser === 'firefox'
        ? v.profile.length > 0 &&
          new TextEncoder().encode(v.profile).length <= 128 &&
          !v.profile.startsWith('.') &&
          !v.profile.endsWith('.') &&
          v.profile.trim() === v.profile &&
          !/[\p{Cc}/\\:]/u.test(v.profile)
        : /^(Default|Profile (0|[1-9][0-9]{0,9}))$/.test(v.profile))
  )
    throw Error('浏览器配置返回格式无法核对。')
  return { browser: v.browser as DownloadBrowser, profile: v.profile }
}
export function browserHistoryPath(home: string, selection: BrowserProfile) {
  const checked = readBrowserProfile(selection)
  if (checked.browser === 'safari')
    return checked.profile === 'Standard'
      ? `${home}/Library/Safari/Downloads.plist`
      : `${home}/Library/Containers/com.apple.Safari/Data/Library/Safari/Downloads.plist`
  return `${home}/Library/Application Support/${downloadBrowsers.find((b) => b.id === checked.browser)!.directory}/${checked.profile}/${checked.browser === 'firefox' ? 'places.sqlite' : 'History'}`
}
export function sameBrowserProfile(a: BrowserProfile | null, b: BrowserProfile | null) {
  return !!a && !!b && a.browser === b.browser && a.profile === b.profile
}
export function readBrowserProfiles(value: unknown, browser: DownloadBrowser, home: string) {
  const fail = () => {
    throw Error('浏览器配置列表与当前选择不匹配。')
  }
  if (!value || typeof value !== 'object') return fail()
  const v = value as Record<string, unknown>
  if (
    v.browser !== browser ||
    typeof v.complete !== 'boolean' ||
    !Number.isSafeInteger(v.skipped) ||
    (v.skipped as number) < 0 ||
    (v.skipped as number) > 2048 ||
    !Array.isArray(v.profiles) ||
    v.profiles.length > 64
  )
    return fail()
  const seen = new Set<string>()
  const profiles = v.profiles.map((p: unknown) => {
    if (!p || typeof p !== 'object') return fail()
    const item = p as Record<string, unknown>,
      selection = readBrowserProfile(item.selection)
    if (
      selection.browser !== browser ||
      seen.has(selection.profile) ||
      item.historyPath !== browserHistoryPath(home, selection)
    )
      return fail()
    seen.add(selection.profile)
    return { selection, historyPath: item.historyPath as string }
  })
  return { profiles, complete: v.complete, skipped: v.skipped as number }
}
export async function listBrowserProfiles(browser: DownloadBrowser, home: string) {
  return readBrowserProfiles(
    await invoke('list_browser_download_profiles', { browser }),
    browser,
    home,
  )
}
