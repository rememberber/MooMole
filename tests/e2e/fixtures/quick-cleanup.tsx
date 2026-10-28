import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { QuickCleanup } from '../../../src/components/QuickCleanup'
import type { CleanCandidate, CleanPreview, ReviewPlan } from '../../../src/types'

export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'quick-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:10000;overflow:auto;background:var(--surface);padding:20px'
  document.body.append(host)
  const root = createRoot(host)
  const item = (
    id: string,
    path: string,
    kind: 'logs' | 'appCache',
    isDirectory = false,
  ): CleanCandidate => ({
    id,
    upstreamIdentity: id,
    path,
    section: kind,
    size: 1024,
    itemCount: 1,
    coveredBy: null,
    isDirectory,
    resolvedPath: path,
    impact: {
      version: 1,
      kind,
      title: '',
      purpose: '',
      consequence: '清理后可能需要重新生成。',
      recovery: '移入废纸篓后可在 Finder 中核对。',
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
            ...(kind === 'logs' ? ['_clean_user_log_directory'] : []),
            'clean_user_essentials',
          ],
          deleteGuard: kind === 'logs' ? '_user_log_delete_guard' : '_user_cache_deno_delete_guard',
          finalGuard: kind === 'logs' ? '_user_log_delete_guard' : '_user_cache_deno_delete_guard',
        },
      ],
    },
  })
  const preview: CleanPreview = {
    output: '',
    dryRun: true,
    protocol: 2,
    revision: 'fixture',
    systemIncluded: false,
    scanId: 'scan',
    scannedAt: Date.now() - 1000,
    expiresAt: Date.now() + 10_000,
    items: [
      item('log', '/Users/demo/Library/Logs/example.log', 'logs'),
      item('protected', '/Users/demo/Library/Logs/protected.log', 'logs'),
      item('dir', '/Users/demo/Library/Logs/directory', 'logs', true),
      item('cache', '/Users/demo/Library/Caches/com.apple.helpd', 'appCache', true),
      item('unknown', '/Users/demo/Library/Caches/example', 'appCache', true),
      item('protected-cache', '/Users/demo/Library/Caches/Quick Look', 'appCache', true),
    ],
  }
  function Fixture() {
    const [protectedPaths, setProtectedPaths] = useState([
      '/Users/demo/Library/Logs/protected.log',
      ...(mode === 'one-failure' ? [] : ['/Users/demo/Library/Caches/Quick Look']),
    ])
    return (
      <QuickCleanup
        preview={preview}
        desktop
        enabled
        home="/Users/demo"
        protectedPaths={protectedPaths}
        apps={[
          {
            name: 'Large App',
            bundle_id: 'example.app',
            source: 'App',
            uninstall_name: 'Large App',
            path: '/Applications/Large App.app',
            size: '2GB',
          },
        ]}
        appsTaskId="apps-task"
        analysis={{
          path: '/Users/demo',
          scan_status: 'partial',
          total_size: 2048,
          entries: [],
          large_files: [{ name: 'movie.mp4', path: '/Users/demo/movie.mp4', size: 2048 }],
        }}
        onUninstall={(target) => sessionStorage.setItem('quick-uninstall', JSON.stringify(target))}
        onProtect={async (path) => setProtectedPaths((paths) => [...paths, path])}
        onInspect={async (_scanId, ids) => {
          if (mode === 'one-failure' && ids.includes('cache'))
            throw new Error('此样例包含链接，保留。')
          const now = Date.now()
          const items = preview.items.filter((entry) => ids.includes(entry.id))
          return {
            id: 'inspection',
            scanId: 'scan',
            createdAt: now - 100,
            expiresAt: now + 10_000,
            checkedAt: now,
            ruleCheckedAt: now,
            itemCount: items.length,
            readOnly: true,
            requiresMoleRecheck: true,
            items: items.map((entry) => ({
              id: entry.id,
              path: entry.path,
              size: entry.size,
              section: entry.section,
              state: 'unchanged',
              reason: 'fixture',
              impact: entry.impact,
              inventoryEntries: entry.isDirectory ? 1 : null,
              descendantRules: entry.isDirectory
                ? { version: 1, code: 'checked', checkedEntries: 1, reason: 'fixture' }
                : null,
              permissions: {
                version: 1,
                code: 'noRestrictionObserved',
                reason: 'fixture',
                findings: [],
              },
              moleRule: {
                code: 'passed',
                source: 'fixture',
                reason: 'fixture',
                module: {
                  version: 2,
                  binding: entry.impact?.kind === 'appCache' ? 'userCache' : 'userLogs',
                  scope: entry.impact?.kind === 'appCache' ? 'userCache' : 'userLogs',
                  code: 'checked',
                  source: 'fixture',
                  reason: 'fixture',
                },
                activity: {
                  version: 1,
                  code: 'noOpenHandles',
                  source: 'fixture',
                  reason: 'fixture',
                },
              },
            })),
            cleanupPreparation: {
              version: 2,
              planId: 'inspection',
              scope: 'userCachesAndLogs',
              method: 'trash',
              readOnly: true,
              checkedAt: now,
              expiresAt: now + 10_000,
              observedCount: items.length,
              authorizationCount: 0,
              items: items.map((entry) => ({ id: entry.id, code: 'observed', reason: 'fixture' })),
            },
          } satisfies ReviewPlan
        }}
        onCreatePlan={async (scanId, ids) => {
          sessionStorage.setItem('quick-plan', JSON.stringify({ scanId, ids }))
        }}
        onNavigate={() => {}}
      />
    )
  }
  root.render(<Fixture />)
}
