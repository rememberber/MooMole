import { useState } from 'react'
import { bytes, dateTime } from '../lib/format'
import {
  folderScanStates,
  type FolderWatch,
  type FolderScheduleEdit,
  type FolderScanRecord,
} from '../lib/folder-schedules'
import type { FolderSchedulesController } from '../lib/use-folder-schedules'
import { Modal } from './Modal'
const empty: FolderScheduleEdit = { id: null, root: '', enabled: false, intervalMinutes: 60 }
function Result({ record }: { record: FolderScanRecord }) {
  const s = record.summary
  return (
    <article>
      <strong>{folderScanStates[record.state]}</strong>
      <small>
        {' '}
        · {dateTime(record.startedAt)} · {record.trigger === 'scheduled' ? '定期扫描' : '手动扫描'}
      </small>
      <p className="mono full-path" data-i18n="off">
        {record.root}
      </p>
      {s && (
        <>
          <p>
            {s.files} 个所见文件 · {bytes(s.logicalBytes)} · 跳过 {s.skipped} 项
          </p>
          {s.comparison === 'compared' ? (
            <p>
              新增 {s.added} · 属性变化 {s.changed} · 未再见 {s.notSeen} · 所见大小变化{' '}
              {s.logicalDeltaBytes! < 0 ? '−' : '+'}
              {bytes(Math.abs(s.logicalDeltaBytes!))}
            </p>
          ) : (
            <p>
              {s.comparison === 'first'
                ? '已保存第一份完整比较基线。'
                : s.comparison === 'reset'
                  ? '范围或保护变化，已重新建立比较基线。'
                  : '扫描不完整，不比较缺失文件，也不替换完整基线。'}
            </p>
          )}
          {!!s.changes.length && (
            <details>
              <summary>查看变化线索</summary>
              <ul>
                {s.changes.map((c) => (
                  <li key={c.path}>
                    <span>
                      {c.kind === 'added' ? '新增' : c.kind === 'changed' ? '属性变化' : '未再见'}{' '}
                      ·{' '}
                    </span>
                    <span className="mono full-path" data-i18n="off">
                      {c.path}
                    </span>
                  </li>
                ))}
              </ul>
              {s.changesTruncated && <p>仅列出前 100 条变化线索，数量汇总包含其余结果。</p>}
            </details>
          )}
        </>
      )}
      {record.error && <p className="notice warning">{record.error}</p>}
    </article>
  )
}
export function FolderSchedules({
  enabled,
  controller: c,
  chooseDirectory,
  onOrganize,
}: {
  enabled: boolean
  controller: FolderSchedulesController
  chooseDirectory: () => Promise<string | null>
  onOrganize: (root: string) => void
}) {
  const [edit, setEdit] = useState<FolderScheduleEdit | null>(null),
    [formError, setFormError] = useState('')
  const view = c.view
  const act = (action: () => Promise<unknown>) => void action().catch(() => {})
  async function save() {
    if (!edit) return
    setFormError('')
    try {
      await c.configure({ ...edit, root: edit.root.trim() })
      setEdit(null)
    } catch (e) {
      setFormError(String(e))
    }
  }
  function toggle(w: FolderWatch) {
    act(() =>
      c.configure({
        id: w.id,
        root: w.root,
        enabled: !w.enabled,
        intervalMinutes: w.intervalMinutes,
      }),
    )
  }
  return (
    <section className="card" aria-label="目录定期扫描">
      <div className="section-toolbar">
        <h3>目录定期扫描</h3>
        <button
          className="button secondary compact"
          disabled={!enabled || !view || c.pending || view.watches.length >= 8}
          onClick={() => {
            setFormError('')
            setEdit({ ...empty })
          }}
        >
          添加扫描目录…
        </button>
        <button
          className="button text compact"
          disabled={!enabled || c.pending}
          onClick={c.refresh}
        >
          刷新扫描记录
        </button>
      </div>
      <p className="muted">
        仅在 MooMole
        运行时扫描明确保存的目录；其他任务占用时延后，休眠后不补跑所有错过的周期。最多八个目录，保留最近
        64 条记录。停止或移除任务不会修改目录文件。
      </p>
      <p className="muted">
        比较所见文件的路径、大小和属性，不读取内容来判断相同，也不推断可清理或可释放空间。“未再见”可能来自移动、排除或目录变化，不代表文件已删除。
      </p>
      {c.error && (
        <p className="notice warning" role="alert">
          {c.error}
        </p>
      )}
      {view && (
        <div className="personal-list">
          {view.watches.map((w) => {
            const latest = view.records.find((r) => r.watchId === w.id)
            return (
              <article key={w.id}>
                <div>
                  <p className="mono full-path" data-i18n="off">
                    {w.root}
                  </p>
                  <p>
                    {w.enabled ? '定期目录扫描已开启' : '定期目录扫描已暂停'} · {w.intervalMinutes}{' '}
                    分钟
                  </p>
                  {w.lastAttemptAt !== null && <small>上次尝试：{dateTime(w.lastAttemptAt)}</small>}
                  {latest && <p>{folderScanStates[latest.state]}</p>}
                  <div className="inline-actions">
                    <button
                      className="button secondary compact"
                      disabled={!enabled || c.pending || c.running}
                      onClick={() => act(() => c.scan(w.id))}
                    >
                      立即扫描
                    </button>
                    <button
                      className="button secondary compact"
                      disabled={!enabled || c.pending}
                      onClick={() => toggle(w)}
                    >
                      {w.enabled ? '暂停扫描' : '启用扫描'}
                    </button>
                    <button
                      className="button text compact"
                      disabled={!enabled || c.pending}
                      onClick={() => {
                        setFormError('')
                        setEdit({
                          id: w.id,
                          root: w.root,
                          enabled: w.enabled,
                          intervalMinutes: w.intervalMinutes,
                        })
                      }}
                    >
                      编辑范围与周期
                    </button>
                    <button
                      className="button text compact"
                      disabled={!enabled || c.pending || c.running}
                      onClick={() => onOrganize(w.root)}
                    >
                      查看并整理
                    </button>
                    <button
                      className="button text compact"
                      disabled={!enabled || c.pending}
                      onClick={() => act(() => c.remove(w.id))}
                    >
                      移除扫描任务
                    </button>
                  </div>
                </div>
              </article>
            )
          })}
          {!view.watches.length && (
            <p className="muted">
              尚未配置扫描目录。添加目录并保存后，可单次扫描或明确开启定期扫描。
            </p>
          )}
        </div>
      )}
      {!!view?.activeRequests.length && (
        <div role="status">
          <p>目录扫描正在运行，可以停止；部分结果不会替换完整比较基线。</p>
          {view.activeRequests.map((id) => (
            <button
              key={id}
              className="button secondary compact"
              disabled={c.pending}
              onClick={() => act(() => c.cancel(id))}
            >
              停止目录扫描
            </button>
          ))}
        </div>
      )}
      {!!view?.records.length && (
        <details>
          <summary>目录扫描记录</summary>
          <div className="personal-list">
            {view.records.map((r) => (
              <Result key={r.id} record={r} />
            ))}
          </div>
        </details>
      )}
      <p className="muted">
        “查看并整理”只传递目录。请重新扫描、预览和选择，再按独立确认执行移动、归档或废纸篓处理；历史扫描不是文件操作许可。
      </p>
      {edit && (
        <Modal title="目录扫描范围与周期" onClose={c.pending ? undefined : () => setEdit(null)}>
          <label className="field">
            扫描目录
            <input
              aria-label="定期扫描目录"
              value={edit.root}
              disabled={c.pending}
              onChange={(e) => setEdit({ ...edit, root: e.target.value })}
            />
          </label>
          <button
            className="button secondary compact"
            disabled={c.pending}
            onClick={() =>
              void chooseDirectory()
                .then((path) => {
                  if (path) setEdit((v) => (v ? { ...v, root: path } : v))
                })
                .catch((e) => setFormError(String(e)))
            }
          >
            选择扫描目录…
          </button>
          <label className="field">
            扫描间隔
            <select
              aria-label="目录扫描间隔"
              value={edit.intervalMinutes}
              disabled={c.pending}
              onChange={(e) => setEdit({ ...edit, intervalMinutes: Number(e.target.value) })}
            >
              <option value={15}>每 15 分钟</option>
              <option value={60}>每小时</option>
              <option value={360}>每 6 小时</option>
              <option value={1440}>每天</option>
            </select>
          </label>
          <label className="confirm-check">
            <input
              type="checkbox"
              checked={edit.enabled}
              disabled={c.pending}
              onChange={(e) => setEdit({ ...edit, enabled: e.target.checked })}
            />
            启用定期目录扫描
          </label>
          <p className="notice">
            保存范围只授权扫描观察，不授权自动处理文件。暂停时仍可手动扫描；退出 App
            后停止，下次打开按已保存设置继续。
          </p>
          {formError && (
            <p className="notice warning" role="alert">
              {formError}
            </p>
          )}
          <div className="dialog-actions">
            <button
              className="button primary"
              disabled={c.pending || !edit.root.trim()}
              onClick={() => void save()}
            >
              保存扫描范围
            </button>
            <button className="button secondary" disabled={c.pending} onClick={() => setEdit(null)}>
              返回
            </button>
          </div>
        </Modal>
      )}
    </section>
  )
}
