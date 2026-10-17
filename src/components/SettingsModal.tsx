import { useState } from 'react'
import { ArrowUpRight, Check, Info, Plus, Settings2, ShieldCheck, Trash2 } from 'lucide-react'
import type { Settings } from '../types'
import { chooseDirectory, official } from '../lib/api'
import { Modal } from './Modal'
import { appVersion } from '../lib/app-version'
import { UpdateCheck } from './UpdateCheck'

export function SettingsModal({
  settings,
  revision,
  version,
  onSave,
  onClose,
  initialTab = 'general',
}: {
  settings: Settings
  revision: string
  version: string | null
  onSave: (settings: Settings) => Promise<void>
  onClose: () => void
  initialTab?: 'general' | 'protected' | 'about'
}) {
  const [tab, setTab] = useState(initialTab)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  async function save(next: Settings) {
    setPending(true)
    setError('')
    try {
      await onSave(next)
    } catch (error) {
      setError(String(error))
    } finally {
      setPending(false)
    }
  }
  async function addPath() {
    try {
      const path = await chooseDirectory()
      if (path)
        await save({
          ...settings,
          protectedPaths: [...new Set([...settings.protectedPaths, path])],
        })
    } catch (error) {
      setError(String(error))
    }
  }
  return (
    <Modal title="设置" onClose={onClose} className="settings-modal">
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
            <p className="section-label">外观</p>
            <div className="setting-group">
              <label className="setting-row">
                <span>
                  主题<small>选择你习惯的阅读方式</small>
                </span>
                <select
                  aria-label="主题"
                  disabled={pending}
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
                  历史记录保留时间<small>只保存任务信息，不保存文件内容</small>
                </span>
                <select
                  aria-label="历史记录保留时间"
                  disabled={pending}
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
                      disabled={pending}
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
            <button className="button secondary" disabled={pending} onClick={() => void addPath()}>
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
            <UpdateCheck saving={pending} />
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
            正在保存…
          </p>
        )}
        {error && (
          <p className="error-text" role="alert">
            {error}
          </p>
        )}
      </section>
    </Modal>
  )
}
