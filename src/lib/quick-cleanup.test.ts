import { describe, expect, it } from 'vitest'
import type { CleanCandidate, ReviewPlan } from '../types'
import {
  quickGroup,
  quickInspectionIds,
  quickSupported,
  recommendedQuickIds,
} from './quick-cleanup'

function cache(name = 'com.apple.helpd'): CleanCandidate {
  const path = `/Users/demo/Library/Caches/${name}`
  return {
    id: name,
    path,
    upstreamIdentity: name,
    section: '',
    size: 10,
    itemCount: 1,
    coveredBy: null,
    resolvedPath: path,
    isDirectory: true,
    impact: {
      version: 1,
      kind: 'appCache',
      title: '',
      purpose: '',
      consequence: '',
      recovery: '',
      requiresReview: true,
    },
    provenance: {
      version: 1,
      observations: [
        {
          path,
          functions: [
            'record_dry_run_cleanup_target',
            '_safe_clean_impl',
            'safe_clean_guarded',
            'clean_user_essentials',
          ],
          deleteGuard: '_user_cache_deno_delete_guard',
          finalGuard: '_user_cache_deno_delete_guard',
        },
      ],
    },
  }
}
function report(item: CleanCandidate): ReviewPlan {
  return {
    id: 'plan',
    scanId: 'scan',
    createdAt: 1000,
    checkedAt: 1000,
    expiresAt: 900_000,
    ruleCheckedAt: 1000,
    itemCount: 1,
    readOnly: true,
    requiresMoleRecheck: true,
    items: [
      {
        id: item.id,
        path: item.path,
        size: item.size,
        section: '',
        state: 'unchanged',
        reason: '',
        impact: item.impact,
        inventoryEntries: 1,
        descendantRules: { version: 1, code: 'checked', checkedEntries: 1, reason: 'fixture' },
        permissions: { version: 1, code: 'noRestrictionObserved', reason: '', findings: [] },
        moleRule: {
          code: 'passed',
          source: '',
          reason: '',
          module: {
            version: 2,
            binding: 'userCache',
            scope: 'userCache',
            code: 'checked',
            source: '',
            reason: '',
          },
          activity: { version: 1, code: 'noOpenHandles', source: '', reason: '' },
        },
      },
    ],
    cleanupPreparation: {
      version: 2,
      planId: 'plan',
      scope: 'userCachesAndLogs',
      method: 'trash',
      readOnly: true,
      checkedAt: 1000,
      expiresAt: 31_000,
      observedCount: 1,
      authorizationCount: 0,
      items: [{ id: item.id, code: 'observed', reason: 'fixture' }],
    },
  }
}
describe('quick cleanup safety', () => {
  it('requires exact module provenance; path or label alone cannot qualify', () => {
    const item = cache()
    expect(quickSupported(item, '/Users/demo')).toBe(true)
    item.provenance!.observations[0].finalGuard = ''
    expect(quickSupported(item, '/Users/demo')).toBe(false)
    item.provenance = null
    expect(quickSupported(item, '/Users/demo')).toBe(false)
  })
  it('only inspects rebuildable candidates and excludes protected ancestors, children and aliases', () => {
    const items = [cache(), cache('personal-session')]
    expect(quickInspectionIds(items, '/Users/demo', [])).toEqual(['com.apple.helpd'])
    expect(quickInspectionIds(items, '/Users/demo', ['/Users/demo/Library/Caches'])).toEqual([])
    expect(quickInspectionIds([items[0]], '/Users/demo', [`${items[0].path}/child`])).toEqual([])
    items[0].resolvedPath = '/protected/alias'
    expect(quickInspectionIds([items[0]], '/Users/demo', ['/protected'])).toEqual([])
  })
  it('defaults only allowlisted rebuildable caches with fresh complete backend evidence', () => {
    const plan = report(cache())
    expect(recommendedQuickIds(plan, 1001)).toEqual(['com.apple.helpd'])
    expect(recommendedQuickIds(plan, 31_000)).toEqual([])
    expect(recommendedQuickIds(plan, 999)).toEqual([])
    expect(recommendedQuickIds(report(cache('personal-session')), 1001)).toEqual([])
    plan.items[0].descendantRules!.code = 'protected'
    expect(recommendedQuickIds(plan, 1001)).toEqual([])
  })
  it('unknown occupancy is never a default recommendation', () => {
    const plan = report(cache())
    plan.items[0].state = 'activityUnknown'
    plan.items[0].moleRule!.activity!.code = 'unknown'
    plan.cleanupPreparation!.observedCount = 0
    plan.cleanupPreparation!.authorizationCount = 1
    plan.cleanupPreparation!.items[0].code = 'authorizationRequired'
    expect(recommendedQuickIds(plan, 1001)).toEqual([])
  })
  it('groups orphan candidates using recorded caller, never terminal headings', () => {
    const item = cache()
    item.section = '卸载残留'
    expect(quickGroup(item)).toBe('cache')
    item.provenance!.observations[0].functions.push('clean_orphaned_app_data')
    expect(quickGroup(item)).toBe('orphan')
  })
})
