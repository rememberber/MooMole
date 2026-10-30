import { useEffect, useRef, useState } from 'react'
import { Modal } from './Modal'
import { bytes, dateTime } from '../lib/format'
import { inspectSpaceItem, openStorageSettings, type SpaceItem } from '../lib/space-explanation'

export function SpaceExplanation({
  path,
  desktop,
  enabled,
  onClose,
  onAction,
  onReveal,
}: {
  path: string | null
  desktop: boolean
  enabled: boolean
  onClose: () => void
  onAction: (action: 'cleanup' | 'backups' | 'home' | 'monitor') => void
  onReveal: (path: string) => void
}) {
  const [item, setItem] = useState<SpaceItem | null>(null)
  const [pending, setPending] = useState(false),
    [error, setError] = useState('')
  const epoch = useRef(0),
    busy = useRef(false)
  async function read() {
    if (!path || !desktop || busy.current) return
    busy.current = true
    const token = ++epoch.current
    setPending(true)
    setItem(null)
    setError('')
    try {
      const result = await inspectSpaceItem(path)
      if (token === epoch.current) setItem(result)
    } catch (e) {
      if (token === epoch.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      if (token === epoch.current) {
        busy.current = false
        setPending(false)
      }
    }
  }
  useEffect(() => {
    void read()
    return () => {
      epoch.current++
      busy.current = false
    }
  }, [path, desktop])
  async function settings() {
    if (busy.current) return
    busy.current = true
    const token = ++epoch.current
    setPending(true)
    setError('')
    try {
      await openStorageSettings()
    } catch (e) {
      if (token === epoch.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      if (token === epoch.current) {
        busy.current = false
        setPending(false)
      }
    }
  }
  function action(value: Parameters<typeof onAction>[0]) {
    onClose()
    onAction(value)
  }
  return (
    <Modal title="空间占用说明" className="space-explanation-modal" onClose={onClose}>
      <h2>空间占用说明</h2>
      {path && (
        <section aria-label="当前项目的空间属性">
          <p className="mono full-path" data-i18n="off">
            {path}
          </p>
          <p className="muted">仅观察当前项目，不递归、不读取正文，不请求下载或移除本地副本。</p>
          {pending && <p role="status">正在读取空间属性…</p>}
          {item && (
            <>
              <dl className="space-item-values">
                <div>
                  <dt>文件逻辑大小</dt>
                  <dd>{bytes(item.logicalBytes)}</dd>
                </div>
                <div>
                  <dt>本地分配量</dt>
                  <dd>{bytes(item.allocatedBytes)}</dd>
                </div>
              </dl>
              {item.isDirectory && (
                <p>
                  这是文件夹，未汇总内部内容；以上大小保持未知。可返回目录分析查看已测量的子项。
                </p>
              )}
              <p>
                {item.dataless
                  ? '检测到无本地完整数据的占位标记；本地分配量可能仅包含少量元数据。'
                  : '未发现无本地数据的占位标记；这不能证明同步完成、云端备份存在或文件可删除。'}
              </p>
              <p>
                {item.location === 'icloudLocation'
                  ? '位置位于此账户的 iCloud 容器目录，不能据此确认具体应用的同步状态。'
                  : item.location === 'providerLocation'
                    ? '位置位于此账户的云文件提供程序目录，具体同步与下载状态请在 Finder 或对应软件中核对。'
                    : item.location === 'accountLibrary'
                      ? '位置位于账户资料库，可能包含应用设置、资料与缓存；不能整目录清理。'
                      : item.location === 'systemLocation'
                        ? '位置位于系统管理范围；本页不提供该位置的删除操作。'
                        : '未识别为固定云目录；其他位置仍可能被同步软件管理。'}
              </p>
              <p className="muted">
                本地分配量可能受稀疏文件、压缩和 APFS 共享块影响，不等于独占空间或可释放空间。
              </p>
              <p className="muted">
                属性读取时间：{dateTime(item.checkedAt)}
                ；文件仍可能继续变化，后续处理需重新扫描和确认。
              </p>
            </>
          )}
          <div className="modal-actions">
            <button
              className="button secondary compact"
              disabled={!desktop || pending}
              onClick={() => void read()}
            >
              重新读取属性
            </button>
            <button
              className="button secondary compact"
              disabled={!desktop || pending}
              onClick={() => onReveal(path)}
            >
              在 Finder 中核对
            </button>
          </div>
        </section>
      )}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      <section aria-label="云文件与本机空间">
        <h3>云文件与本机空间</h3>
        <p>
          文件显示的大小不一定都已下载到本机。占位状态、逻辑大小和本地分配量分别回答不同问题，也不能证明云端已有可恢复副本。
        </p>
        <p>
          删除 iCloud Drive 文件可能同步删除其他设备上的文件。仅想减少本机副本时，请先在 Finder
          或对应云盘软件核对同步状态，再使用其提供的移除下载功能；这里不会代为执行。
        </p>
      </section>
      <section aria-label="系统数据与下一步">
        <h3>系统数据与下一步</h3>
        <p>
          系统数据是 macOS
          的归类，包含无法归入其他类别的系统与应用数据，不对应一个可以整体清空的文件夹。
        </p>
        <p>
          这里没有测量系统数据总量、快照占用或可清除空间，也不把目录测量差额算成系统数据。APFS
          卷可能共享空间，废纸篓中的文件仍会占用空间。
        </p>
        <div className="space-next-actions">
          <button
            className="button secondary"
            disabled={!enabled || pending}
            onClick={() => action('cleanup')}
          >
            审阅常用清理候选
          </button>
          <button
            className="button secondary"
            disabled={!desktop || pending}
            onClick={() => action('backups')}
          >
            审阅整份设备备份
          </button>
          <button
            className="button secondary"
            disabled={!enabled || pending}
            onClick={() => action('home')}
          >
            分析个人目录
          </button>
          <button className="button secondary" disabled={pending} onClick={() => action('monitor')}>
            查看可用空间趋势
          </button>
          <button
            className="button secondary"
            disabled={!desktop || pending}
            onClick={() => void settings()}
          >
            打开系统设置核对分类
          </button>
        </div>
        <p className="muted">
          打开系统设置后，在通用 → 储存空间核对系统分类；入口名称可能随 macOS
          版本变化。以上入口不会自动开始清理。
        </p>
      </section>
    </Modal>
  )
}
