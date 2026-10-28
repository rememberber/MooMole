import { getLocale } from '../i18n/locale'
import type { AppEntry } from '../types'

// The pinned Mole bytes_to_human uses decimal units and rounded display values.
// These estimates are for sorting/comparison only, never execution decisions.
export function appSizeEstimate(value: unknown): number | null {
  if (typeof value !== 'string') return null
  const match = /^(\d+(?:\.\d+)?)(B|KB|MB|GB)$/.exec(value.trim())
  if (!match) return null
  const size = Number(match[1]) * 1000 ** ['B', 'KB', 'MB', 'GB'].indexOf(match[2])
  return Number.isFinite(size) && size <= Number.MAX_SAFE_INTEGER ? size : null
}

export function appEstimateLabel(size: number): string {
  const unit = Math.min(Math.floor(Math.log10(Math.max(1, size)) / 3), 3)
  return `${(size / 1000 ** unit).toLocaleString(getLocale(), { maximumFractionDigits: 2 })} ${['B', 'KB', 'MB', 'GB'][unit]}`
}

export const appSource = (value: string) =>
  value === 'Homebrew' ? 'Homebrew' : value === 'App' ? '普通应用' : '来源待确认'

export function compareApps(a: AppEntry, b: AppEntry, sort: 'size' | 'name') {
  if (sort === 'size') {
    const left = appSizeEstimate(a.size)
    const right = appSizeEstimate(b.size)
    if (left == null && right != null) return 1
    if (right == null && left != null) return -1
    if (left != null && right != null && left !== right) return right - left
  }
  return a.name.localeCompare(b.name, getLocale()) || a.path.localeCompare(b.path, getLocale())
}
