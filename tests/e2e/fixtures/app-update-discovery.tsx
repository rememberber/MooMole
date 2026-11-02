import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ApplicationLibrary } from '../../../src/components/ApplicationLibrary'
import { ApplicationUpdateCenter } from '../../../src/components/ApplicationUpdateCenter'
import { AppUpdateDiscoveryCenter } from '../../../src/components/AppUpdateDiscoveryCenter'
import type { UninstallTarget } from '../../../src/lib/app-uninstall'
import { setLanguagePreference } from '../../../src/i18n/locale'
import { applications } from './application-library'

// Browser transport only: does not contact vendors, open a browser or change apps.
export function mount(mode = 'normal') {
  setLanguagePreference(
    mode === 'en' || mode === 'downloadEn'
      ? 'en'
      : mode === 'ja' || mode === 'downloadJa'
        ? 'ja'
        : 'zh',
  )
  const host = document.createElement('div')
  host.id = 'app-discovery-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const calls: { command: string; args: Record<string, unknown> }[] = []
  const task = '853bf826-4ae1-4695-bc66-85d49e1957b0',
    now = Date.now() - (mode === 'expired' ? 300001 : 0)
  const probe = {
    id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    taskId: task,
    index: 0,
    path: applications[0].path,
    bundleId: applications[0].bundle_id,
    currentVersion: '1.0',
    currentBuild: '10',
    feedUrl: mode === 'unsupported' ? null : 'https://updates.example.org/feed.xml',
    state: mode === 'unsupported' ? 'unsupported' : 'ready',
    reason:
      mode === 'unsupported'
        ? '未发现本轮支持的 HTTPS 更新地址，请使用应用自身更新器或已下载应用包。'
        : '更新地址来自此应用的 Info.plist，尚未访问网络或验证发布者。',
    createdAt: now,
    expiresAt: now + 300000,
    verifiedDownload: mode.startsWith('download'),
  }
  const release = {
    id: '821324f0-d589-4873-b42a-8061782d839e',
    version: '2.0',
    build: '11',
    url: 'https://updates.example.org/release.zip',
    kind: 'package',
    bytes: mode.startsWith('download') ? 1234 : null,
    downloadable: mode.startsWith('download'),
    requiresReview: true,
  }
  const draft = {
    id: '46573b46-9956-492e-b38c-101114edbeec',
    taskId: task,
    index: 0,
    name: applications[0].name,
    path: applications[0].path,
    replacement: '/Users/demo/Downloads/Example.app',
    bundleId: applications[0].bundle_id,
    oldVersion: '1.0',
    newVersion: '2.0',
    publisher: 'adHoc',
    bytes: 12345,
    createdAt: now,
    expiresAt: now + 120000,
  }
  let rejectActive: ((reason: Error) => void) | undefined
  let resolveLate: ((value: unknown) => void) | undefined
  let unmount = () => {}
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'prepare_app_update_discovery') {
          if (mode === 'late')
            return new Promise((resolve) => {
              resolveLate = resolve
            })
          return { ...probe, ...(mode === 'mismatch' ? { path: '/Applications/Other.app' } : {}) }
        }
        if (command === 'check_app_update_discovery') {
          if (mode === 'cancel')
            return new Promise((_, reject) => {
              rejectActive = reject
            })
          const empty = ['unknown', 'noNewer'].includes(mode)
          return {
            probeId: probe.id,
            requestId: args.requestId,
            checkedAt: Date.now(),
            expiresAt: probe.expiresAt,
            state: empty ? (mode === 'unknown' ? 'unknown' : 'noNewerInFeed') : 'newerReported',
            complete: mode !== 'unknown',
            inspected: 1,
            skipped: empty ? 1 : 0,
            releases: empty ? [] : [release],
          }
        }
        if (command === 'open_app_update_candidate' || command === 'discard_app_update_discovery')
          return null
        if (command === 'cancel_task') {
          rejectActive?.(Error('应用更新来源检查已停止。'))
          return null
        }
        if (command === 'download_app_update_candidate') {
          if (mode === 'downloadCancel')
            return new Promise((_, reject) => {
              rejectActive = reject
            })
          if (mode === 'downloadBadSignature')
            throw Error('下载包签名不匹配，未保存；请使用应用自身更新器核对。')
          return {
            probeId: probe.id,
            candidateId: mode === 'downloadMismatch' ? probe.id : release.id,
            requestId: args.requestId,
            path: '/Users/demo/Downloads/MooMole-update-46573b46-9956-492e-b38c-101114edbeec.zip',
            bytes: 1234,
            sha256: 'a'.repeat(64),
            verification: 'installedKeyEd25519',
          }
        }
        if (command === 'plugin:dialog|open')
          return (args.options as { directory?: boolean })?.directory
            ? mode === 'downloadDialogCancel'
              ? null
              : '/Users/demo/Downloads'
            : draft.replacement
        if (command === 'prepare_application_update') return draft
        if (command === 'execute_application_update')
          return {
            id: draft.id,
            requestId: args.requestId,
            createdAt: Date.now(),
            finishedAt: Date.now(),
            draft,
            state: 'updated',
            phase: 'finished',
            reason: 'browser fixture only',
            backup: `/Applications/.moomole-app-update-${draft.id}/Example.app`,
            reviewedAt: null,
          }
        if (command === 'get_application_updates') return []
        if (command === 'get_application_restorations') return { records: [], activeRequests: [] }
        if (command === 'fixture_unmount') {
          unmount()
          return null
        }
        if (command === 'fixture_late') {
          resolveLate?.(probe)
          return null
        }
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [target, setTarget] = useState<UninstallTarget | null>(null),
      [manual, setManual] = useState<UninstallTarget | null>(null),
      [active, setActive] = useState(false),
      [shown, setShown] = useState(true)
    unmount = () => setShown(false)
    return (
      <>
        <ApplicationLibrary
          apps={applications.slice(0, 2)}
          taskId={task}
          home="/Users/demo"
          canControl={!active}
          onDiscoverUpdate={setTarget}
        />
        {shown && (
          <AppUpdateDiscoveryCenter
            target={target}
            onClose={() => setTarget(null)}
            onActiveChange={setActive}
            onDownloaded={(target) => {
              setTarget(null)
              setManual(target)
            }}
          />
        )}
        <ApplicationUpdateCenter
          target={manual}
          historyOpen={false}
          onClose={() => setManual(null)}
          onHistoryClose={() => {}}
          onActiveChange={setActive}
          onOutcome={() => {}}
        />
      </>
    )
  }
  host.dataset.calls = '[]'
  createRoot(host).render(<Fixture />)
}
