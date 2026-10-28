import { useLayoutEffect, useRef, useState } from 'react'
import { ArrowUpRight, Check, Info, Plus, Settings2, ShieldCheck, Trash2 } from 'lucide-react'
import type { Settings } from '../types'
import { chooseDirectory, official } from '../lib/api'
import { Modal } from './Modal'
import { appVersion } from '../lib/app-version'
import { UpdateCheck } from './UpdateCheck'
import { restoreFocus } from '../lib/focus-return'
import { CleanupVisibility } from './CleanupVisibility'

export function SettingsModal({
  settings,
  revision,
  version,
  onSave,
  onClose,
  returnFocus,
  initialTab = 'general',
}: {
  settings: Settings
  revision: string
  version: string | null
  onSave: (settings: Settings) => Promise<void>
  onClose: () => void
  returnFocus?: () => HTMLElement | null
  initialTab?: 'general' | 'protected' | 'about'
}) {
  const [tab, setTab] = useState(initialTab)
  const [pending, setPending] = useState<'choosing' | 'saving' | null>(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const active = useRef(false)
  const operation = useRef<object | null>(null)
  const addButton = useRef<HTMLButtonElement>(null)
  const returnToAdd = useRef(false)
  const latest = useRef(settings)
  latest.current = settings
  useLayoutEffect(() => {
    active.current = true
    return () => {
      active.current = false
      operation.current = null
      returnToAdd.current = false
    }
  }, [])
  useLayoutEffect(() => {
    if (pending || !returnToAdd.current) return
    returnToAdd.current = false
    restoreFocus(addButton.current)
  }, [pending])
  function begin(state: 'choosing' | 'saving') {
    if (!active.current || operation.current) return null
    const token = {}
    operation.current = token
    setPending(state)
    setError('')
    setNotice('')
    return token
  }
  function current(token: object) {
    return active.current && operation.current === token
  }
  function finish(token: object) {
    if (!current(token)) return
    operation.current = null
    setPending(null)
  }
  async function save(next: Settings) {
    const token = begin('saving')
    if (!token) return
    try {
      await onSave(next)
    } catch (error) {
      if (current(token)) setError(String(error))
    } finally {
      finish(token)
    }
  }
  async function addPath() {
    const token = begin('choosing')
    if (!token) return
    try {
      const path = await chooseDirectory('选择要保护的文件夹')
      // The native picker cannot be aborted from this component. A result
      // belongs only to the settings session that opened it.
      if (!current(token) || !path) return
      const saved = latest.current
      if (saved.protectedPaths.includes(path)) {
        setNotice('此目录已在保护列表中。')
        return
      }
      setPending('saving')
      await onSave({ ...saved, protectedPaths: [...saved.protectedPaths, path] })
    } catch (error) {
      if (current(token)) setError(String(error))
    } finally {
      if (current(token)) returnToAdd.current = true
      finish(token)
    }
  }
  function close() {
    // Invalidate synchronously, before React unmounts the dialog.
    active.current = false
    operation.current = null
    onClose()
  }
  return (
    <Modal title="设置" onClose={close} returnFocus={returnFocus} className="settings-modal">
      <nav className="settings-nav" aria-label="设置分类">
        <span className="settings-label">设置</span>
        {(
          [
            ['general', Settings2, '通用'],
            ['protected', ShieldCheck, '保护列表'],
            ['about', Info, '关于'],
          ] as const
        ).map(([id, Icon, label]) => (
          <button key={id} className={tab === id ? 'selected' : ''} onClick={() => setTab(id)}>
            <Icon size={17} />
            {label}
          </button>
        ))}
      </nav>
      <section className="settings-content">
        <h2>{tab === 'general' ? '通用' : tab === 'protected' ? '保护列表' : '关于 MooMole'}</h2>
        {tab === 'general' && (
          <>
            <p className="section-label">语言</p>
            <div className="setting-group">
              <label className="setting-row">
                <span>
                  界面语言<small>首次启动跟随系统；其他系统语言使用英语</small>
                </span>
                <select
                  aria-label="界面语言"
                  disabled={!!pending}
                  value={settings.language ?? 'system'}
                  onChange={(event) =>
                    void save({ ...settings, language: event.target.value as Settings['language'] })
                  }
                >
                  <option value="system">跟随系统</option>
                  <option value="zh" lang="zh-CN" data-i18n="off">
                    简体中文
                  </option>
                  <option value="en" lang="en" data-i18n="off">
                    English
                  </option>
                  <option value="ja" lang="ja" data-i18n="off">
                    日本語
                  </option>
                </select>
              </label>
            </div>
            <p className="section-label">外观</p>
            <div className="setting-group">
              <label className="setting-row">
                <span>
                  主题<small>选择你习惯的阅读方式</small>
                </span>
                <select
                  aria-label="主题"
                  disabled={!!pending}
                  value={settings.theme}
                  onChange={(event) =>
                    void save({ ...settings, theme: event.target.value as Settings['theme'] })
                  }
                >
                  <option value="system">跟随系统</option>
                  <option value="light">浅色</option>
                  <option value="dark">深色</option>
                </select>
              </label>
            </div>
            <p className="section-label">本地数据</p>
            <div className="setting-group">
              <label className="setting-row">
                <span>
                  历史记录保留时间<small>任务、扫描、计划等记录；部分执行凭证会保留</small>
                </span>
                <select
                  aria-label="历史记录保留时间"
                  disabled={!!pending}
                  value={settings.retentionDays}
                  onChange={(event) =>
                    void save({ ...settings, retentionDays: Number(event.target.value) })
                  }
                >
                  <option value={7}>7 天</option>
                  <option value={30}>30 天</option>
                  <option value={90}>90 天</option>
                </select>
              </label>
              <div className="setting-row">
                <span>
                  数据存储<small>设置与历史保存在这台 Mac 上</small>
                </span>
                <span className="tag">
                  <Check size={12} />
                  SQLite
                </span>
              </div>
            </div>
            <p className="muted settings-retention-note">
              启动或保存设置时回收过期记录，不删除你的文件。
            </p>
            <details className="settings-retention">
              <summary>保留范围与例外</summary>
              <p>记录可能包含路径和检查结果，不保存文件正文。</p>
              <p>扫描最多保留 10 份，关联审阅计划最多 20 份，可能早于所选期限回收。</p>
              <p>
                等待或进行中的任务会保留。已移入废纸篓、结果待确认或中断的执行凭证也会继续保留，供你核对。
              </p>
              <p>Mole 原始历史、独立备份与导出文件不在此回收范围内。</p>
            </details>
          </>
        )}
        {tab === 'protected' && (
          <>
            <p className="muted settings-description">
              保护你想保留的目录。清理审阅中，与保护目录重叠的父子项目均不可勾选；开发缓存检查会跳过这些项目。Mole
              自有白名单继续生效，客户端不改写其配置。
            </p>
            <div className="setting-group protected-list">
              {settings.protectedPaths.length ? (
                settings.protectedPaths.map((path) => (
                  <div className="setting-row" key={path}>
                    <span className="mono truncate" title={path}>
                      {path}
                    </span>
                    <button
                      className="icon-button"
                      aria-label={`移除保护 ${path}`}
                      disabled={!!pending}
                      onClick={() =>
                        void save({
                          ...settings,
                          protectedPaths: settings.protectedPaths.filter((item) => item !== path),
                        })
                      }
                    >
                      <Trash2 size={15} />
                    </button>
                  </div>
                ))
              ) : (
                <div className="empty-inline">还没有添加保护目录</div>
              )}
            </div>
            <button
              ref={addButton}
              className="button secondary"
              disabled={!!pending}
              onClick={() => void addPath()}
            >
              <Plus size={15} />
              添加目录
            </button>
          </>
        )}
        {tab === 'about' && (
          <>
            <div className="about-brand">
              <div className="brand-mark">
                m<span>·</span>
              </div>
              <div>
                <h3>MooMole</h3>
                <p className="muted">{appVersion} · 学习研究版</p>
              </div>
            </div>
            <UpdateCheck saving={!!pending} />
            <details className="settings-retention">
              <summary>清理权限检查</summary>
              <CleanupVisibility saving={!!pending} />
            </details>
            <div className="setting-group about-copy">
              <p>
                本项目仅供学习和研究使用，是独立的非官方项目。使用前请理解操作影响并备份重要数据，由此产生的问题与使用风险由使用者自行承担。
              </p>
              <p>
                部分能力基于 Mole CLI，感谢原作者及贡献者。为尊重和支持 Mole
                的开发成果，日常使用建议选择官方产品。
              </p>
              <button
                className="button primary"
                onClick={() => void official().catch((error) => setError(String(error)))}
              >
                访问 Mole 官方产品
                <ArrowUpRight size={15} />
              </button>
            </div>
            <p className="fine-print">
              Mole CLI {version ? `v${version}` : '版本待确认'} · GPL-3.0
              <br />
              源码基线 <span className="mono">{revision.slice(0, 12)}</span>
            </p>
          </>
        )}
        {pending && (
          <p className="muted" role="status">
            {pending === 'choosing' ? '请在系统窗口中选择目录…' : '正在保存…'}
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {error && (
          <p className="error-text" role="alert">
            {error}
          </p>
        )}
      </section>
    </Modal>
  )
}
