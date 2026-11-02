import { useState } from 'react'
import { ExternalLink } from 'lucide-react'
import { ApplicationIcon } from './ApplicationIcon'
import { ApplicationInspection } from './ApplicationInspection'
import type { AppEntry } from '../types'
import { shortPath } from '../lib/format'
import { appSizeEstimate, appSource } from '../lib/application-library'
import type { AppControlTarget } from '../lib/app-control'
import type { UninstallTarget } from '../lib/app-uninstall'
import type { AppUsageRow } from '../lib/application-usage'
import { getLocale } from '../i18n/locale'

export function ApplicationRow({
  app,
  usage,
  taskId,
  protectedPaths,
  index,
  home,
  selected,
  onSelect,
  canReveal,
  onReveal,
  canUninstall = false,
  onUninstall,
  onManagedUninstall,
  onControl,
  canControl = false,
  onUpdate,
  onDiscoverUpdate,
}: {
  app: AppEntry
  usage?: AppUsageRow
  taskId?: string | null
  protectedPaths: string[]
  index: number
  home: string
  selected: boolean
  onSelect: (checked: boolean) => void
  canReveal: boolean
  onReveal: (path: string) => void
  canControl?: boolean
  onControl?: (target: AppControlTarget) => void
  onUpdate?: (target: UninstallTarget) => void
  onDiscoverUpdate?: (target: UninstallTarget) => void
  canUninstall?: boolean
  onUninstall?: (target: UninstallTarget) => void
  onManagedUninstall?: (target: UninstallTarget) => void
}) {
  const [open, setOpen] = useState(false)
  return (
    <article className="application-item" aria-label={`${app.name} · ${app.path}`}>
      <div className="app-row">
        <input
          type="checkbox"
          aria-label={`对比 ${app.path}`}
          checked={selected}
          onChange={(event) => onSelect(event.target.checked)}
        />
        <ApplicationIcon taskId={taskId} index={index} />
        <div className="app-info">
          <strong data-i18n="off">{app.name}</strong>
          <small className="mono">{shortPath(app.path, home)}</small>
          {usage && (
            <small>
              {usage.state === 'running'
                ? '此副本正在运行'
                : usage.state === 'protected'
                  ? '受保护，未读取使用记录'
                  : usage.state === 'observed' && usage.lastOpenedAt !== null
                    ? `系统上次打开：${new Date(usage.lastOpenedAt).toLocaleDateString(getLocale(), { year: 'numeric', month: '2-digit', day: '2-digit' })}`
                    : usage.issue === 'notReported' || usage.issue === 'notIndexed'
                      ? '系统未提供打开记录'
                      : '使用记录无法核对'}
            </small>
          )}
        </div>
        <span className="tag">{appSource(app.source)}</span>
        <span className="app-size">
          {appSizeEstimate(app.size) == null ? '大小未知' : app.size}
        </span>
        {onUpdate && (
          <button
            className="button secondary compact"
            disabled={!canControl || !taskId || app.source !== 'App'}
            onClick={() => {
              if (taskId)
                onUpdate({ taskId, index, name: app.name, path: app.path, bundleId: app.bundle_id })
            }}
          >
            从新版应用包更新…
          </button>
        )}
        {onDiscoverUpdate && (
          <button
            className="button secondary compact"
            disabled={!canControl || !taskId || app.source !== 'App'}
            onClick={() => {
              if (taskId)
                onDiscoverUpdate({
                  taskId,
                  index,
                  name: app.name,
                  path: app.path,
                  bundleId: app.bundle_id,
                })
            }}
          >
            检查更新…
          </button>
        )}
        {onControl && (
          <div className="application-control-actions">
            {(['open', 'quit'] as const).map((action) => (
              <button
                className="button secondary compact"
                key={action}
                disabled={!canControl || !taskId || app.source !== 'App'}
                onClick={() => {
                  if (taskId)
                    onControl({
                      taskId,
                      index,
                      name: app.name,
                      path: app.path,
                      bundleId: app.bundle_id,
                      action,
                    })
                }}
              >
                {action === 'open' ? '打开' : '退出…'}
              </button>
            ))}
          </div>
        )}
        {onUninstall && (
          <button
            className="button secondary compact"
            disabled={
              !canUninstall ||
              !taskId ||
              (app.source !== 'App' && !(app.source === 'Homebrew' && onManagedUninstall))
            }
            title={
              app.source !== 'App' && !(app.source === 'Homebrew' && onManagedUninstall)
                ? '请使用对应管理工具卸载'
                : '先检查卸载范围，不会直接执行'
            }
            onClick={() => {
              if (taskId)
                (app.source === 'Homebrew' ? onManagedUninstall : onUninstall)?.({
                  taskId,
                  index,
                  name: app.name,
                  path: app.path,
                  bundleId: app.bundle_id,
                })
            }}
          >
            卸载…
          </button>
        )}
      </div>
      <details
        className="application-details"
        onToggle={(event) => setOpen(event.currentTarget.open)}
      >
        <summary>位置与来源</summary>
        <dl>
          <dt>完整路径</dt>
          <dd className="mono">{app.path}</dd>
          <dt>Bundle ID</dt>
          <dd className="mono">{app.bundle_id || '未提供'}</dd>
          <dt>Mole 来源</dt>
          <dd>{app.source || '未提供'}</dd>
          {appSizeEstimate(app.size) == null && (
            <>
              <dt>Mole 大小标记</dt>
              <dd>{app.size || '未提供'}</dd>
            </>
          )}
        </dl>
        {open && (
          <ApplicationInspection
            key={`${taskId}:${index}:${JSON.stringify(protectedPaths)}`}
            taskId={taskId}
            index={index}
            expectedId={app.bundle_id}
            expectedName={app.name}
          />
        )}
        <button
          className="button secondary compact"
          disabled={!canReveal}
          onClick={() => onReveal(app.path)}
        >
          <ExternalLink size={13} />在 Finder 中显示
        </button>
        {onControl && (
          <button
            className="button secondary compact"
            disabled={!canControl || !taskId || app.source !== 'App'}
            onClick={() => {
              if (taskId)
                onControl({
                  taskId,
                  index,
                  name: app.name,
                  path: app.path,
                  bundleId: app.bundle_id,
                  action: 'register',
                })
            }}
          >
            重新注册此副本…
          </button>
        )}
      </details>
    </article>
  )
}
