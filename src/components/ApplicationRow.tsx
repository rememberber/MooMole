import { useState } from 'react'
import { ExternalLink } from 'lucide-react'
import { ApplicationIcon } from './ApplicationIcon'
import { ApplicationInspection } from './ApplicationInspection'
import type { AppEntry } from '../types'
import { shortPath } from '../lib/format'
import { appSizeEstimate, appSource } from '../lib/application-library'
import type { UninstallTarget } from '../lib/app-uninstall'

export function ApplicationRow({
  app,
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
}: {
  app: AppEntry
  taskId?: string | null
  protectedPaths: string[]
  index: number
  home: string
  selected: boolean
  onSelect: (checked: boolean) => void
  canReveal: boolean
  onReveal: (path: string) => void
  canUninstall?: boolean
  onUninstall?: (target: UninstallTarget) => void
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
          <strong>{app.name}</strong>
          <small className="mono">{shortPath(app.path, home)}</small>
        </div>
        <span className="tag">{appSource(app.source)}</span>
        <span className="app-size">
          {appSizeEstimate(app.size) == null ? '大小未知' : app.size}
        </span>
        {onUninstall && (
          <button
            className="button secondary compact"
            disabled={!canUninstall || !taskId || app.source !== 'App'}
            title={app.source !== 'App' ? '请使用对应管理工具卸载' : '先检查卸载范围，不会直接执行'}
            onClick={() => {
              if (taskId)
                onUninstall({
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
      </details>
    </article>
  )
}
