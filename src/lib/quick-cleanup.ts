import type { CleanCandidate, ReviewPlan } from '../types'
import { protectionFor } from './clean-preview'
import { cleanupPreparation } from './cleanup-preparation'

export const quickGroups = [
  ['cache', '应用缓存'],
  ['downloadCache', '下载缓存'],
  ['logs', '日志'],
  ['diagnostics', '诊断报告'],
  ['orphan', '卸载残留'],
  ['other', '其他清理候选'],
] as const
export function quickGroup(item: CleanCandidate) {
  if (
    item.provenance?.version === 1 &&
    item.provenance.observations.some(
      (origin) => origin.path === item.path && origin.functions.includes('clean_orphaned_app_data'),
    )
  )
    return 'orphan'
  const kind = item.impact?.kind
  return kind === 'appCache' ? 'cache' : kind && kind !== 'unknown' ? kind : 'other'
}

// Preliminary UI coverage only; no path or renderer decision grants execution.
export function quickSupported(item: CleanCandidate, home: string): boolean {
  const base = home.replace(/\/+$/, '')
  const parent = item.path.slice(0, item.path.lastIndexOf('/'))
  if (item.path.split('/').some((part) => part === '.' || part === '..')) return false
  if (item.path.split('/').at(-1)?.startsWith('.')) return false
  const binding =
    parent === `${base}/Library/Caches`
      ? 'userCache'
      : !item.isDirectory &&
          [`${base}/Library/Logs`, `${base}/Library/Logs/DiagnosticReports`].includes(parent)
        ? 'userLogs'
        : null
  const source = item.provenance
  if (
    !binding ||
    source?.version !== 1 ||
    !source.observations.length ||
    source.observations.length > 16
  )
    return false
  const stack =
    binding === 'userCache'
      ? [
          'record_dry_run_cleanup_target',
          '_safe_clean_impl',
          'safe_clean_guarded',
          'clean_user_essentials',
        ]
      : [
          'record_dry_run_cleanup_target',
          '_safe_clean_impl',
          'safe_clean_guarded',
          '_clean_user_log_directory',
          'clean_user_essentials',
        ]
  const guard = binding === 'userCache' ? '_user_cache_deno_delete_guard' : '_user_log_delete_guard'
  return source.observations.every(
    (origin) =>
      origin.path === item.path &&
      origin.deleteGuard === guard &&
      origin.finalGuard === guard &&
      stack.every((name, index) => origin.functions[index] === name),
  )
}

const rebuildableCacheNames = [
  'com.apple.helpd',
  'com.apple.iconservices',
  'com.apple.QuickLook.thumbnailcache',
  'Quick Look',
]
function rebuildableCache(path: string) {
  return rebuildableCacheNames.includes(path.split('/').at(-1) ?? '')
}

export function quickInspectionIds(
  items: CleanCandidate[],
  home: string,
  protectedPaths: string[],
) {
  return items
    .filter(
      (item) =>
        rebuildableCache(item.path) &&
        quickSupported(item, home) &&
        !protectionFor(item, protectedPaths).length,
    )
    .sort(
      (a, b) =>
        Number(rebuildableCache(b.path)) - Number(rebuildableCache(a.path)) ||
        Number(quickGroup(b) === 'cache') - Number(quickGroup(a) === 'cache') ||
        (b.size ?? -1) - (a.size ?? -1),
    )
    .slice(0, 20)
    .map((item) => item.id)
}

export function recommendedQuickIds(plan: ReviewPlan, now: number): string[] {
  const preparation = cleanupPreparation(plan, now)
  if (!preparation?.valid) return []
  return plan.items
    .filter(
      (item) =>
        item.impact?.kind === 'appCache' &&
        rebuildableCache(item.path) &&
        item.moleRule?.module?.binding === 'userCache' &&
        preparation.items.get(item.id)?.code === 'observed',
    )
    .map((item) => item.id)
}
