import { describe, expect, it } from 'vitest'
import type { AppEntry, CleanCandidate } from '../types'
import { softwareCleanupGroups } from './software-cleanup'

function cache(name: string, size = 10): CleanCandidate {
  const path = `/Users/demo/Library/Caches/${name}`
  return {
    id: name,
    path,
    upstreamIdentity: name,
    section: 'fixture',
    size,
    itemCount: 1,
    coveredBy: null,
    resolvedPath: path,
    isDirectory: true,
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
const app: AppEntry = {
  name: 'My Application',
  bundle_id: 'example.application',
  source: 'App',
  uninstall_name: 'My Application',
  path: '/Applications/My Application.app',
  size: '1GB',
}

describe('software cleanup classification', () => {
  it('groups exact browser/chat/media cache identifiers and keeps source objects and IDs', () => {
    const items = [
      cache('com.google.Chrome', 30),
      cache('com.tencent.xinWeChat', 20),
      cache('com.spotify.client'),
    ]
    const groups = softwareCleanupGroups(items, '/Users/demo/')
    expect(groups.map((group) => [group.label, group.category])).toEqual([
      ['Google Chrome', 'browser'],
      ['微信', 'chat'],
      ['Spotify', 'media'],
    ])
    expect(groups[0].items[0]).toBe(items[0])
    expect(groups.flatMap((group) => group.items)).toEqual(items)
  })
  it('does not turn similar names, nested roots or missing/contradictory provenance into software scope', () => {
    const forged = cache('com.google.Chrome')
    forged.provenance = null
    const mixed = cache('com.tencent.xinWeChat')
    mixed.provenance!.observations.push({ ...mixed.provenance!.observations[0], finalGuard: '' })
    const items = [
      cache('com.google.Chrome.backup'),
      cache('com.google.Chrome/Default'),
      cache('../Caches/com.google.Chrome'),
      cache('__proto__'),
      forged,
      mixed,
    ]
    const groups = softwareCleanupGroups(items, '/Users/demo')
    expect(groups).toHaveLength(1)
    expect(groups[0].category).toBe('unclassified')
    expect(groups[0].items).toEqual(items)
    expect(softwareCleanupGroups([cache('com.google.Chrome')], '/Users/other')[0].category).toBe(
      'unclassified',
    )
  })
  it('uses an exact inventory bundle ID only as a literal display label; no fuzzy app matching', () => {
    const items = [cache(app.bundle_id), cache('My Application'), cache(`${app.bundle_id}.shared`)]
    const groups = softwareCleanupGroups(items, '/Users/demo', [app])
    expect(groups[0]).toMatchObject({
      id: app.bundle_id,
      label: app.name,
      literal: true,
      category: 'other',
    })
    expect(groups[0].items).toEqual([items[0]])
    expect(groups[1].items).toEqual(items.slice(1))
  })
  it('always retains unknown candidates and does not mutate scan order or contents', () => {
    const items = [
      cache('unknown', 1000),
      cache('com.google.Chrome', 0),
      cache('com.apple.helpd', 10),
    ]
    const original = structuredClone(items)
    const groups = softwareCleanupGroups(items, '/Users/demo')
    expect(groups.map((group) => group.id)).toEqual([
      'com.apple.helpd',
      'com.google.Chrome',
      'unclassified',
    ])
    expect(
      groups
        .flatMap((group) => group.items)
        .map((item) => item.id)
        .sort(),
    ).toEqual(items.map((item) => item.id).sort())
    expect(items).toEqual(original)
  })
})
