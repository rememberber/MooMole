import { useEffect, useState } from 'react'
import { readMenuBar, type MenuBarTransport, type MenuBarView } from '../lib/menu-bar'
export function MenuBarSettings({
  enabled,
  transport,
}: {
  enabled: boolean
  transport: MenuBarTransport
}) {
  const [view, setView] = useState<MenuBarView | null>(null),
    [checked, setChecked] = useState(false),
    [pending, setPending] = useState(false),
    [error, setError] = useState('')
  useEffect(() => {
    if (!enabled) return
    let live = true
    void transport
      .read()
      .then(readMenuBar)
      .then((v) => {
        if (live) {
          setView(v)
          setChecked(v.config.enabled)
        }
      })
      .catch((e) => {
        if (live) setError(String(e))
      })
    return () => {
      live = false
    }
  }, [enabled, transport])
  async function save() {
    if (!enabled || pending || !view) return
    setPending(true)
    setError('')
    try {
      const v = readMenuBar(await transport.configure(checked))
      setView(v)
      setChecked(v.config.enabled)
    } catch (e) {
      setError(String(e))
    } finally {
      setPending(false)
    }
  }
  return (
    <section className="card" aria-label="菜单栏持续管理">
      <label className="confirm-check">
        <input
          type="checkbox"
          checked={checked}
          disabled={!enabled || !view || pending}
          onChange={(e) => setChecked(e.target.checked)}
        />
        在菜单栏保留 MooMole，关闭窗口后继续运行
      </label>
      <p className="muted">
        菜单栏可查看容量、打开空间趋势或单次记录。是否定期记录仍由观察设置决定；退出 MooMole
        后停止，不添加登录启动项、不自动处理文件。
      </p>
      <button
        className="button secondary compact"
        disabled={!enabled || !view || pending}
        onClick={() => void save()}
      >
        {pending ? '正在保存…' : '保存菜单栏设置'}
      </button>
      {view && (
        <p role="status">
          {view.visible
            ? '菜单栏已显示，关闭窗口后继续运行；可从菜单栏退出。'
            : '菜单栏未显示，关闭窗口按普通应用行为处理。'}
        </p>
      )}
      {(error || view?.error) && (
        <p className="notice warning" role="alert">
          {error || view?.error}
        </p>
      )}
    </section>
  )
}
