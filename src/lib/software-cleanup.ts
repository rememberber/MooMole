import type { AppEntry, CleanCandidate } from '../types'
import { quickSupported } from './quick-cleanup'

export const softwareCategories = [
  ['all', '全部软件'],
  ['browser', '浏览器'],
  ['chat', '聊天与协作'],
  ['media', '音乐与视频'],
  ['mail', '邮件'],
  ['system', '系统缓存'],
  ['other', '其他软件'],
  ['unclassified', '未归组候选'],
] as const
export type SoftwareCategory = (typeof softwareCategories)[number][0]
type Family = Exclude<SoftwareCategory, 'all' | 'unclassified'>

// Exact cache-root names are presentation hints, not evidence of exclusive
// ownership or permission. They never add candidates or grant execution.
const products: Record<string, readonly [string, Family]> = {
  'com.apple.Safari': ['Safari', 'browser'],
  'com.google.Chrome': ['Google Chrome', 'browser'],
  'com.google.Chrome.beta': ['Google Chrome Beta', 'browser'],
  'com.microsoft.edgemac': ['Microsoft Edge', 'browser'],
  'org.mozilla.firefox': ['Firefox', 'browser'],
  'com.brave.Browser': ['Brave', 'browser'],
  'company.thebrowser.Browser': ['Arc', 'browser'],
  'com.tencent.xinWeChat': ['微信', 'chat'],
  'com.tencent.qq': ['QQ', 'chat'],
  'com.tencent.WeWorkMac': ['企业微信', 'chat'],
  'com.tinyspeck.slackmacgap': ['Slack', 'chat'],
  'com.microsoft.teams2': ['Microsoft Teams', 'chat'],
  'ru.keepcoder.Telegram': ['Telegram', 'chat'],
  'com.spotify.client': ['Spotify', 'media'],
  'com.netease.163music': ['网易云音乐', 'media'],
  'com.apple.Music': ['音乐', 'media'],
  'com.apple.mail': ['邮件', 'mail'],
  'com.microsoft.Outlook': ['Microsoft Outlook', 'mail'],
  'com.apple.helpd': ['系统帮助', 'system'],
  'com.apple.iconservices': ['系统图标', 'system'],
  'com.apple.QuickLook.thumbnailcache': ['文件预览', 'system'],
  'Quick Look': ['文件预览', 'system'],
}

export interface SoftwareCleanupGroup {
  id: string
  label: string
  literal: boolean
  category: Exclude<SoftwareCategory, 'all'>
  items: CleanCandidate[]
}

export function softwareCleanupGroups(
  items: CleanCandidate[],
  home: string,
  apps: AppEntry[] | null = null,
): SoftwareCleanupGroup[] {
  const root = `${home.replace(/\/+$/, '')}/Library/Caches/`
  const groups = new Map<string, SoftwareCleanupGroup>()
  for (const item of items) {
    const identifier = item.path.startsWith(root) ? item.path.slice(root.length) : ''
    const product = Object.hasOwn(products, identifier) ? products[identifier] : undefined
    const app = apps?.find((entry) => entry.bundle_id === identifier && entry.name.trim())
    // Provenance and exact root binding are required even for classification.
    // A familiar filename, partial prefix, label or orphan scan is insufficient.
    const matched = quickSupported(item, home) && identifier && !identifier.includes('/')
    const id = matched && (product || app) ? identifier : 'unclassified'
    let group = groups.get(id)
    if (!group) {
      group = {
        id,
        label: id === 'unclassified' ? '未归组候选' : (product?.[0] ?? app!.name),
        literal: id !== 'unclassified' && !product,
        category: id === 'unclassified' ? 'unclassified' : (product?.[1] ?? 'other'),
        items: [],
      }
      groups.set(id, group)
    }
    group.items.push(item)
  }
  return [...groups.values()].sort(
    (a, b) =>
      Number(a.id === 'unclassified') - Number(b.id === 'unclassified') ||
      b.items.reduce((size, item) => size + (item.size ?? 0), 0) -
        a.items.reduce((size, item) => size + (item.size ?? 0), 0) ||
      a.id.localeCompare(b.id, 'en'),
  )
}
