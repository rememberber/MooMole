import { useEffect, useRef, useState } from 'react'
import * as api from '../lib/api'
import type { PersonalController } from '../lib/use-personal-files'
import type { PersonalConfirmation } from '../lib/personal-files'
import type { PreviewTarget } from '../lib/file-preview'
import { bytes, dateTime } from '../lib/format'
import { pathOverlaps } from '../lib/clean-preview'
import { useNow } from '../lib/use-now'
import { Modal } from './Modal'
import { FilePreview } from './FilePreview'
import { CleanupResultSummary } from './CleanupResultSummary'
import { ExecutionReason } from './ExecutionReason'
import { ExecutionLocation } from './ExecutionLocation'

export function LivePhotoPairs({
  controller: c,
  enabled,
  protectedPaths,
  onHistory,
}: {
  controller: PersonalController
  enabled: boolean
  protectedPaths: string[]
  onHistory: () => void
}) {
  const [selected, setSelected] = useState<string[]>([]),
    [query, setQuery] = useState(''),
    [older, setOlder] = useState(0),
    [minimum, setMinimum] = useState(0)
  const [draft, setDraft] = useState<PersonalConfirmation | null>(null),
    [ack, setAck] = useState(false),
    [pending, setPending] = useState(false),
    [error, setError] = useState(''),
    [preview, setPreview] = useState<PreviewTarget | null>(null)
  const generation = useRef(0),
    plan = useRef<string | null>(null)
  useNow()
  const now = Date.now(),
    report = c.snapshot.report,
    live = report?.livePhotos
  const fresh =
    !!live &&
    !!report &&
    report.createdAt <= now &&
    now < report.expiresAt &&
    c.root.trim().replace(/\/+$/, '') === report.root
  const entries = (live?.pairs ?? []).map((p) => ({
    pair: p,
    photo: report!.items.find((i) => i.id === p.photoId)!,
    movie: report!.items.find((i) => i.id === p.movieId)!,
  }))
  const blocked = (path: string) => protectedPaths.some((p) => pathOverlaps(p, path))
  const ids = entries
    .filter((e) => selected.includes(e.pair.id))
    .flatMap((e) => [e.photo.id, e.movie.id])
  const visible = entries.filter(
    (e) =>
      (!query ||
        [e.photo.path, e.movie.path].some((p) =>
          p.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
        )) &&
      e.photo.size + e.movie.size >= minimum &&
      (!older || Math.max(e.photo.modifiedAt, e.movie.modifiedAt) <= now - older * 86400000),
  )
  useEffect(() => {
    ++generation.current
    setSelected([])
    setDraft(null)
    setAck(false)
    setPending(false)
    setPreview(null)
    setError('')
    return () => {
      ++generation.current
      if (plan.current) {
        void c.discard(plan.current).catch(() => {})
        plan.current = null
      }
    }
  }, [report?.id, c.root])
  async function prepare() {
    if (!fresh || !ids.length || pending || c.running || !enabled) return
    const ticket = generation.current
    setPending(true)
    setError('')
    try {
      const d = await c.prepare(ids)
      if (ticket !== generation.current) {
        void c.discard(d.id).catch(() => {})
        return
      }
      if (!d.livePhotoPairs?.length) throw Error('Live Photo 确认缺少配对范围。')
      plan.current = d.id
      setDraft(d)
      setAck(false)
    } catch (e) {
      if (ticket === generation.current) setError(String(e))
    } finally {
      if (ticket === generation.current) setPending(false)
    }
  }
  function close() {
    if (plan.current) void c.discard(plan.current).catch(() => {})
    plan.current = null
    setDraft(null)
    setAck(false)
  }
  return (
    <div className="live-photo-pairs">
      <div className="personal-scope live-photo-scope">
        <label className="personal-path">
          <span>Live Photo 导出目录</span>
          <input
            aria-label="Live Photo 导出目录"
            value={c.root}
            disabled={c.running || pending}
            onChange={(e) => c.setRoot(e.target.value)}
          />
        </label>
        <button
          className="button secondary compact"
          disabled={!enabled || c.running || pending}
          onClick={() =>
            void api
              .chooseDirectory()
              .then((p) => {
                if (p) c.setRoot(p)
              })
              .catch((e) => setError(String(e)))
          }
        >
          选择文件夹
        </button>
      </div>
      <p className="notice">
        核对普通文件夹中已导出的照片与短视频。相同文件名只用于寻找候选，系统需确认两份资源可组成
        Live Photo；不进入照片图库，也不判断它们重复或可以丢弃。
      </p>
      <details className="personal-help">
        <summary>Live Photo 范围与处理方式</summary>
        <p>
          当前核对同一文件夹、同名的 JPEG/HEIC/HEIF 与
          MOV。缺失、同名多份、错配、受保护或无法确认的资源保留；改名、其他格式和图库内资源需另行检查。
        </p>
        <p>
          默认不选，每次最多 10
          对。照片和短视频一起进入确认，但系统按文件依次移入废纸篓；停止或失败可能只完成其中一份，必须查看逐文件结果。不会清空废纸篓或承诺立即释放空间。
        </p>
      </details>
      {(error || c.error) && (
        <p role="alert" className="error">
          {error || c.error}
        </p>
      )}
      {c.running && <p role="status">正在核对或处理 Live Photo 资源，可停止并核对实际结果。</p>}
      {live && report && (
        <>
          <p role="status">
            已核对 {live.checkedCandidates} 组同名候选，系统确认 {live.pairs.length} 对；未确认{' '}
            {live.rejectedCandidates} 组，缺失或歧义资源 {live.unpairedFiles} 份。
          </p>
          {!report.complete && (
            <p className="notice warning">
              本次未完整覆盖目录；未显示的资源保持原样，不能据此认定没有其他 Live Photos。
            </p>
          )}
          {!fresh && (
            <p className="notice warning">目录已改变或扫描已过期，请重新扫描 Live Photo 对。</p>
          )}
          <div className="personal-filters">
            <input
              aria-label="搜索 Live Photo"
              placeholder="名称或路径"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <label>
              两份均早于
              <select
                aria-label="Live Photo 修改时间"
                value={older}
                onChange={(e) => setOlder(Number(e.target.value))}
              >
                <option value={0}>不限</option>
                <option value={30}>30 天</option>
                <option value={180}>180 天</option>
                <option value={365}>365 天</option>
              </select>
            </label>
            <label>
              合计大小
              <select
                aria-label="Live Photo 合计大小"
                value={minimum}
                onChange={(e) => setMinimum(Number(e.target.value))}
              >
                <option value={0}>不限</option>
                <option value={10 * 1024 * 1024}>10 MiB</option>
                <option value={100 * 1024 * 1024}>100 MiB</option>
              </select>
            </label>
            <span>
              已选 {selected.length} 对 · {ids.length} 份文件
            </span>
            <button
              className="button secondary compact"
              disabled={!selected.length || c.running || pending}
              onClick={() => setSelected([])}
            >
              清空选择
            </button>
            <button
              className="button primary compact"
              disabled={!fresh || !enabled || !selected.length || c.running || pending}
              onClick={() => void prepare()}
            >
              审阅所选整对文件
            </button>
          </div>
          <section className="card personal-list" aria-label="Live Photo 配对资源">
            {visible.map(({ pair, photo, movie }) => (
              <article className="personal-row live-photo-row" key={pair.id}>
                <label>
                  <input
                    type="checkbox"
                    aria-label={`选择 Live Photo ${photo.name}`}
                    checked={selected.includes(pair.id)}
                    disabled={
                      !fresh ||
                      !enabled ||
                      c.running ||
                      pending ||
                      blocked(photo.path) ||
                      blocked(movie.path) ||
                      (!selected.includes(pair.id) && selected.length >= 10)
                    }
                    onChange={() =>
                      setSelected((s) =>
                        s.includes(pair.id) ? s.filter((i) => i !== pair.id) : [...s, pair.id],
                      )
                    }
                  />
                  <strong data-i18n="off">{photo.name}</strong>
                </label>
                <span>{bytes(photo.size + movie.size)}</span>
                <div className="live-photo-resources">
                  {[photo, movie].map((item, index) => (
                    <div key={item.id}>
                      <p className="mono full-path" data-i18n="off">
                        {item.path}
                      </p>
                      <p>
                        {bytes(item.size)} · {dateTime(item.modifiedAt)}
                      </p>
                      <button
                        className="button secondary compact"
                        disabled={!fresh || !enabled || c.running || pending || blocked(item.path)}
                        onClick={() =>
                          setPreview({
                            domain: 'personalFiles',
                            scanId: report.id,
                            itemId: item.id,
                            path: item.path,
                          })
                        }
                      >
                        {index === 0 ? '预览照片' : '预览短视频'}
                      </button>
                    </div>
                  ))}
                </div>
              </article>
            ))}
            {!visible.length && (
              <p className="empty-inline">
                没有匹配的已确认 Live Photo 对，可调整筛选或检查导出资源。
              </p>
            )}
          </section>
        </>
      )}
      {c.result?.revision === 'MooMole.personal-live-photos.v1' && (
        <section className="card personal-result" aria-label="Live Photo 逐文件结果">
          <strong>
            已移入 {c.result.movedCount} / {c.result.itemCount} 份文件 · 待核实{' '}
            {c.result.uncertainCount} 份
          </strong>
          <p>请核对照片和短视频各自的结果；一份完成不代表整对完成。</p>
          <CleanupResultSummary detail={c.result} />
          {c.result.items.map((i) => (
            <article key={i.id}>
              <p data-i18n="off" className="mono full-path">
                {i.path}
              </p>
              <p>
                {i.state === 'moved'
                  ? '已移入废纸篓'
                  : i.state === 'uncertain'
                    ? '结果待核实'
                    : '本项未确认完成'}
              </p>
              <ExecutionReason reason={i.reason} />
              {i.receipt && (
                <ExecutionLocation
                  runId={c.result!.id}
                  itemId={i.id}
                  onReveal={api.revealExecutionItem}
                  method={c.result!.method}
                />
              )}
            </article>
          ))}
          <button className="button secondary" onClick={onHistory}>
            查看操作历史
          </button>
        </section>
      )}
      {draft && (
        <Modal title="确认处理 Live Photo 整对文件" onClose={close} className="review-plan-modal">
          <h2>确认处理 Live Photo 整对文件</h2>
          <p>
            将所选照片及对应短视频依次移入废纸篓。动态、声音与照片可能各含独有内容；停止或失败可能只完成一部分，请按收据核对。
          </p>
          <p>
            本次 {draft.livePhotoPairs!.length} 对 · {draft.items.length} 份文件
          </p>
          <div className="live-photo-confirmation-items">
            {draft.items.map((i) => (
              <p className="mono full-path" data-i18n="off" key={i.id}>
                {i.path}
              </p>
            ))}
          </div>
          <label className="confirmation-ack live-photo-ack">
            <input type="checkbox" checked={ack} onChange={(e) => setAck(e.target.checked)} />
            我已核对两份资源，了解逐文件处理、部分完成及废纸篓恢复的边界。
          </label>
          {(now < draft.createdAt || now >= draft.expiresAt) && (
            <p className="notice warning">确认已过期，请关闭后重新审阅。</p>
          )}
          <button
            className="button primary"
            disabled={
              !ack ||
              !fresh ||
              !enabled ||
              c.running ||
              now < draft.createdAt ||
              now >= draft.expiresAt
            }
            onClick={() => {
              plan.current = null
              setDraft(null)
              setSelected([])
              void c.execute(draft.id)
            }}
          >
            确认移入所选整对文件
          </button>
        </Modal>
      )}
      {preview && (
        <FilePreview
          key={`${preview.scanId}:${preview.itemId}`}
          target={preview}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  )
}
