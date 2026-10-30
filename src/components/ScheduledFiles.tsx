import { useEffect, useRef, useState } from 'react'
import { bytes, dateTime } from '../lib/format'
import { Modal } from './Modal'
import { useNow } from '../lib/use-now'
import type {
  ScheduledFileRule,
  ScheduledFilePolicy,
  ScheduledFileDraft,
} from '../lib/scheduled-files'
import type { ScheduledFilesController } from '../lib/use-scheduled-files'
const empty: ScheduledFileRule = {
  root: '',
  action: 'move',
  destination: '',
  extensions: ['zip'],
  minimumAgeDays: 30,
  maximumFiles: 10,
  maximumBytes: 64 * 1024 * 1024,
  intervalMinutes: 1440,
}
export function ScheduledRuleSummary({ rule: r }: { rule: ScheduledFileRule }) {
  return (
    <>
      <p className="mono full-path" data-i18n="off">
        {r.root}
      </p>
      <p>
        {r.action === 'move'
          ? '同卷移动到所选目录，不覆盖同名文件。'
          : '移入 macOS 废纸篓，不清空废纸篓。'}
      </p>
      {r.destination && (
        <p className="mono full-path" data-i18n="off">
          → {r.destination}
        </p>
      )}
      <p>
        {r.minimumAgeDays} 天未修改 ·{' '}
        <span data-i18n="off">{r.extensions.map((s) => `.${s}`).join(' / ')}</span> · 每次最多{' '}
        {r.maximumFiles} 项 / {bytes(r.maximumBytes)} · 每 {r.intervalMinutes} 分钟检查
      </p>
    </>
  )
}
export function ScheduledFiles({
  enabled,
  controller: c,
  chooseDirectory,
  onHistory,
}: {
  enabled: boolean
  controller: ScheduledFilesController
  chooseDirectory: () => Promise<string | null>
  onHistory: () => void
}) {
  const [edit, setEdit] = useState<{ id: string | null; rule: ScheduledFileRule } | null>(null)
  const act = (f: () => Promise<unknown>) => void f().catch(() => {})
  async function revise(p: ScheduledFilePolicy) {
    await c.pause(p.id)
    setEdit({ id: p.id, rule: { ...p.rule, extensions: [...p.rule.extensions] } })
  }
  return (
    <section className="card" aria-label="定期文件处理">
      <div className="section-toolbar">
        <h3>定期文件处理</h3>
        <button
          className="button secondary compact"
          disabled={
            !enabled ||
            !c.view ||
            c.pending ||
            c.running ||
            c.view.policies.length >= 8 ||
            !!c.error
          }
          onClick={() => setEdit({ id: null, rule: { ...empty } })}
        >
          添加处理规则…
        </button>
        <button
          className="button secondary compact"
          disabled={!enabled || c.pending}
          onClick={c.refresh}
        >
          刷新定期处理
        </button>
      </div>
      <p className="muted">
        默认不启用。仅在 App
        运行期间按明确规则处理；退出重开后暂停，不补跑。重新启用须再次预览确认，扫描任务不会自动变成处理规则。
      </p>
      {c.error && (
        <p role="alert" className="notice warning">
          {c.error}
        </p>
      )}
      {c.view?.policies.map((p) => (
        <article className="scheduled-file-policy" key={p.id}>
          <strong>{p.enabled ? '本次运行已启用' : '已暂停，需重新预览启用'}</strong>
          <ScheduledRuleSummary rule={p.rule} />
          <div className="inline-actions">
            <button
              className="button secondary compact"
              disabled={!enabled || c.pending}
              onClick={() => act(() => revise(p))}
            >
              预览并重新启用…
            </button>
            {p.enabled && (
              <>
                <button
                  className="button secondary compact"
                  disabled={c.pending}
                  onClick={() => act(() => c.pause(p.id))}
                >
                  暂停此处理规则
                </button>
                <button
                  className="button secondary compact"
                  disabled={c.pending || c.running || !!c.error}
                  onClick={() => act(() => c.run(p.id))}
                >
                  按规则处理一次
                </button>
              </>
            )}
            <button
              className="button secondary compact"
              disabled={c.pending}
              onClick={() => act(() => c.remove(p.id))}
            >
              移除此处理规则
            </button>
          </div>
        </article>
      ))}
      {c.view && !c.view.policies.length && (
        <p className="muted">尚未配置定期处理，不会自动移动或删除文件。</p>
      )}
      {c.view?.activeRequests.map((id) => (
        <div key={id} role="status">
          <span>定期处理正在收集实际结果。</span>
          <button
            className="button secondary compact"
            disabled={c.pending}
            onClick={() => act(() => c.cancel(id))}
          >
            停止本次处理
          </button>
        </div>
      ))}
      {!!c.view?.records.length && (
        <details>
          <summary>最近定期处理结果</summary>
          {c.view.records.map((r) => (
            <article className="scheduled-file-policy" key={r.id}>
              <strong>
                {r.state === 'running'
                  ? '正在处理'
                  : r.state === 'completed'
                    ? '本次处理已结束'
                    : r.state === 'interrupted'
                      ? '已中断，未自动重放'
                      : '未全部完成，规则已暂停'}
              </strong>
              <small> · {dateTime(r.startedAt)}</small>
              <p className="mono full-path" data-i18n="off">
                {r.rule.root}
              </p>
              <p>
                符合 {r.matchedFiles} 项 · 本次选择 {r.selectedFiles} 项 · 上限保留 {r.heldByLimits}{' '}
                项
              </p>
              <p>{r.reason}</p>
              {r.executionId && (
                <button className="button secondary compact" onClick={onHistory}>
                  查看执行历史
                </button>
              )}
            </article>
          ))}
        </details>
      )}
      {edit && (
        <RuleForm
          key={edit.id ?? 'new'}
          id={edit.id}
          initial={edit.rule}
          c={c}
          chooseDirectory={chooseDirectory}
          onClose={() => setEdit(null)}
        />
      )}
    </section>
  )
}
function RuleForm({
  id,
  initial,
  c,
  chooseDirectory,
  onClose,
}: {
  id: string | null
  initial: ScheduledFileRule
  c: ScheduledFilesController
  chooseDirectory: () => Promise<string | null>
  onClose: () => void
}) {
  const [rule, setRule] = useState(initial),
    [extensions, setExtensions] = useState(initial.extensions.join(', ')),
    [draft, setDraft] = useState<ScheduledFileDraft | null>(null),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  const now = useNow(),
    current = useRef<ScheduledFileDraft | null>(null),
    live = useRef(true),
    submitted = useRef(false)
  useEffect(() => {
    live.current = true
    return () => {
      live.current = false
      if (current.current) void c.discard(current.current.id).catch(() => {})
    }
  }, [c.discard])
  function update(next: ScheduledFileRule) {
    if (draft) void c.discard(draft.id).catch(() => {})
    setRule(next)
    setDraft(null)
    current.current = null
    setAck(false)
    setError('')
  }
  async function choose(target: boolean) {
    try {
      const path = await chooseDirectory()
      if (path) update({ ...rule, ...(target ? { destination: path } : { root: path }) })
    } catch (e) {
      setError(String(e))
    }
  }
  async function preview() {
    setError('')
    submitted.current = false
    setAck(false)
    const old = draft
    setDraft(null)
    current.current = null
    const r = {
      ...rule,
      root: rule.root.trim(),
      destination: rule.action === 'trash' ? null : (rule.destination?.trim() ?? ''),
      extensions: [
        ...new Set(
          extensions
            .split(',')
            .map((s) => s.trim().toLowerCase())
            .filter(Boolean),
        ),
      ].sort(),
    }
    try {
      if (old) await c.discard(old.id)
      const d = await c.preview(id, r)
      if (!live.current) {
        void c.discard(d.id).catch(() => {})
        return
      }
      setRule(d.rule)
      setDraft(d)
      current.current = d
    } catch (e) {
      if (live.current) setError(String(e))
    }
  }
  async function save() {
    if (!draft || !ack || submitted.current) return
    submitted.current = true
    try {
      await c.enable(draft)
      onClose()
    } catch (e) {
      setError(String(e))
    }
  }
  const fresh = draft && now >= draft.createdAt && now < draft.expiresAt
  return (
    <Modal
      title="预览并启用定期处理"
      className="scheduled-file-modal"
      onClose={c.pending ? undefined : onClose}
    >
      <h2>预览并启用定期处理</h2>
      <p className="notice warning">
        启用后，本次 App
        运行期间，后续符合条件的文件也会按规则处理，无需逐次确认。只按修改日期和扩展名判断，不代表文件近期没用或可以丢弃；新复制但保留旧修改日期的文件也可能命中。请使用用途明确的具体目录。
      </p>
      <details open={!draft}>
        <summary>编辑处理条件</summary>
        <div className="scheduled-file-form">
          <div className="scheduled-file-path">
            <label>
              处理源目录
              <input
                value={rule.root}
                disabled={c.pending}
                onChange={(e) => update({ ...rule, root: e.target.value })}
              />
            </label>
            <button
              className="button secondary compact"
              disabled={c.pending}
              onClick={() => void choose(false)}
            >
              选择源目录…
            </button>
          </div>
          <label className="scheduled-wide">
            定期操作
            <select
              aria-label="定期操作"
              value={rule.action}
              disabled={c.pending}
              onChange={(e) =>
                update({
                  ...rule,
                  action: e.target.value as 'move' | 'trash',
                  destination: e.target.value === 'trash' ? null : '',
                })
              }
            >
              <option value="move">移动到独立目录</option>
              <option value="trash">移入废纸篓</option>
            </select>
          </label>
          {rule.action === 'move' && (
            <div className="scheduled-file-path">
              <label>
                处理目标目录
                <input
                  value={rule.destination ?? ''}
                  disabled={c.pending}
                  onChange={(e) => update({ ...rule, destination: e.target.value })}
                />
              </label>
              <button
                className="button secondary compact"
                disabled={c.pending}
                onClick={() => void choose(true)}
              >
                选择目标目录…
              </button>
            </div>
          )}
          <label className="scheduled-wide">
            确切扩展名（逗号分隔，不含点）
            <input
              value={extensions}
              disabled={c.pending}
              onChange={(e) => {
                setExtensions(e.target.value)
                update({ ...rule })
              }}
            />
          </label>
          <label>
            至少多少天未修改
            <input
              type="number"
              min={30}
              max={3650}
              value={rule.minimumAgeDays}
              disabled={c.pending}
              onChange={(e) => update({ ...rule, minimumAgeDays: Number(e.target.value) })}
            />
          </label>
          <label>
            每次最多文件数
            <input
              type="number"
              min={1}
              max={20}
              value={rule.maximumFiles}
              disabled={c.pending}
              onChange={(e) => update({ ...rule, maximumFiles: Number(e.target.value) })}
            />
          </label>
          <label>
            每次大小上限（MiB）
            <input
              type="number"
              min={1}
              max={1024}
              value={rule.maximumBytes / 1048576}
              disabled={c.pending}
              onChange={(e) => update({ ...rule, maximumBytes: Number(e.target.value) * 1048576 })}
            />
          </label>
          <label>
            处理检查间隔
            <select
              aria-label="处理检查间隔"
              value={rule.intervalMinutes}
              disabled={c.pending}
              onChange={(e) => update({ ...rule, intervalMinutes: Number(e.target.value) })}
            >
              <option value={60}>每小时</option>
              <option value={360}>每六小时</option>
              <option value={1440}>每天</option>
            </select>
          </label>
        </div>
      </details>
      <button className="button secondary" disabled={c.pending} onClick={() => void preview()}>
        只读预览规则
      </button>
      {c.pending && <p role="status">正在检查或保存定期规则…</p>}
      {draft && (
        <section aria-label="定期处理预览">
          <ScheduledRuleSummary rule={draft.rule} />
          <p>
            当前符合 {draft.matchedFiles} 项 · 按上限选择 {draft.selected.length} 项 · 保留{' '}
            {draft.heldByLimits} 项
          </p>
          <ul>
            {draft.selected.map((i) => (
              <li key={i.id} className="mono full-path" data-i18n="off">
                {i.path}
              </li>
            ))}
          </ul>
          {!draft.selected.length && <p>本次没有符合项；启用后仍会处理后续符合规则的文件。</p>}
          {!fresh && <p role="status">规则预览已过期，请重新检查。</p>}
          <label className="confirm-check">
            <input
              type="checkbox"
              checked={ack}
              disabled={c.pending || !fresh || submitted.current}
              onChange={(e) => setAck(e.target.checked)}
            />
            我已核对规则和目录，允许本次 App 运行期间自动处理后续符合条件的文件。
          </label>
        </section>
      )}
      {error && (
        <p role="alert" className="notice warning">
          {error}
        </p>
      )}
      <p className="muted">
        仅处理本账户拥有的本地普通文件；目录、点开头名称、链接、硬链接文件、云占位和其他卷不纳入。
      </p>
      <p className="muted">
        保存不会立即处理文件。移动仅支持同卷且不覆盖；废纸篓不会自动清空。失败、未知、中断或时间基准变化时暂停，已经完成的项目不会自动撤销。
      </p>
      <div className="modal-actions">
        <button className="button secondary" disabled={c.pending} onClick={onClose}>
          返回持续管理
        </button>
        <button
          className="button primary"
          disabled={c.pending || !fresh || !ack || submitted.current}
          onClick={() => void save()}
        >
          确认启用此处理规则
        </button>
      </div>
    </Modal>
  )
}
