import { useEffect, useMemo, useState } from 'react'
import { ArrowUpRight, FolderOpen, LoaderCircle, ShieldCheck } from 'lucide-react'
import type { AiScanController } from '../lib/use-ai-scan'
import { aiRecommendation, aiReviewable } from '../lib/ai-scan'
import { bytes, dateTime, shortPath } from '../lib/format'
import { useNow } from '../lib/use-now'
import * as api from '../lib/api'
import skill from '../../skills/moomole-deep-scan/SKILL.md?raw'

export function AiDeepScan({
  controller: ai,
  home,
  protectedPaths,
  enabled,
  onCreatePlan,
  onReveal,
}: {
  controller: AiScanController
  home: string
  protectedPaths: string[]
  enabled: boolean
  onCreatePlan: (scanId: string, ids: string[]) => Promise<void>
  onReveal: (path: string) => void
}) {
  const now = useNow()
  const [selected, setSelected] = useState<string[]>([])
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  const snapshot = ai.snapshot
  const reviewable = useMemo(
    () => (snapshot ? aiReviewable(snapshot, protectedPaths, now) : []),
    [snapshot, protectedPaths, now],
  )
  const chosen = reviewable.filter((e) => selected.includes(e.id))
  useEffect(() => {
    void ai.detect()
  }, [ai.detect])
  useEffect(() => {
    setSelected([])
    setQuery('')
    setFilter('all')
    setError('')
  }, [snapshot?.id])
  useEffect(() => {
    setSelected((ids) => ids.filter((id) => reviewable.some((e) => e.id === id)))
  }, [reviewable])
  const findings = snapshot?.report?.findings ?? []
  const visible = findings.filter((f) => {
    const e = snapshot?.evidence.find((e) => e.id === f.evidenceId)
    return (
      (filter === 'all' || filter === f.recommendation) &&
      `${e?.path} ${f.reason}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())
    )
  })
  async function chooseDirectory() {
    try {
      const path = await api.chooseDirectory('选择 AI 扩展扫描目录')
      if (path) ai.setRoot(path)
    } catch (e) {
      setError(String(e))
    }
  }
  async function review() {
    if (!snapshot?.scanId || !chosen.length || !enabled || pending) return
    setPending(true)
    setError('')
    try {
      await onCreatePlan(
        snapshot.scanId,
        chosen.map((e) => e.candidateId!),
      )
    } catch (e) {
      setError(String(e).replace(/^Error: /, ''))
    } finally {
      setPending(false)
    }
  }
  return (
    <div className="ai-deep-scan">
      {!api.desktop && (
        <p className="notice">浏览器仅展示界面。请在桌面 App 中检测本机 Agent 并开始扫描。</p>
      )}
      <form
        id="ai-scan-form"
        className="card ai-setup"
        onSubmit={(e) => {
          e.preventDefault()
          if (enabled) void ai.start()
        }}
      >
        <div className="ai-scope-row">
          <label>
            分析 Agent
            <select
              aria-label="分析 Agent"
              value={ai.agentId}
              disabled={ai.running || ai.detecting}
              onChange={(e) => ai.setAgentId(e.target.value)}
            >
              <option value="">{ai.detecting ? '正在检测…' : '选择本地 Agent'}</option>
              {ai.agents.map((a) => (
                <option key={a.id} value={a.id} disabled={!a.path || !a.supported}>
                  {a.name}
                  {!a.path ? ' · 未发现' : !a.supported ? ' · 尚未适配' : ' · 已发现'}
                </option>
              ))}
            </select>
          </label>
          <label className="ai-root">
            扩展扫描目录
            <span className="ai-path-input">
              <input
                aria-label="扩展扫描目录"
                value={ai.root}
                disabled={ai.running}
                onChange={(e) => ai.setRoot(e.target.value)}
                placeholder="选择普通文件夹"
              />
              <button
                type="button"
                className="icon-button"
                aria-label="选择扩展扫描目录"
                disabled={!api.desktop || ai.running}
                onClick={() => void chooseDirectory()}
              >
                <FolderOpen size={16} />
              </button>
            </span>
          </label>
        </div>
        <p className="muted">
          Mole 扫描常用清理位置；扩展扫描读取所选目录的有界元数据，AI
          结合模型知识解释用途、影响和保留建议。
        </p>
        <label className="ai-consent">
          <input
            type="checkbox"
            checked={ai.consent}
            disabled={ai.running}
            onChange={(e) => ai.setConsent(e.target.checked)}
          />
          <span>
            允许将本次路径、名称、类型、大小和修改时间交给所选 Agent
            的模型服务分析，可能消耗其额度。不发送文件内容。Agent 在本机运行不代表模型离线运行。
          </span>
        </label>
        <details className="ai-details">
          <summary>Agent 检测与内置 Skill</summary>
          <ul className="ai-agent-list">
            {ai.agents.length ? (
              ai.agents.map((a) => (
                <li key={a.id}>
                  <strong>
                    {a.name} ·{' '}
                    {a.path ? (a.supported ? '已发现，登录待验证' : '尚未适配') : '未发现'}
                  </strong>
                  <p>{a.note}</p>
                  {a.path && <code>{a.path}</code>}
                </li>
              ))
            ) : (
              <li>
                在桌面 App 中点击“检测 Agent”。仅检查 CLI
                常用位置，不读取登录凭据；不自动安装或修改全局 Skill。
              </li>
            )}
          </ul>
          <p className="muted">
            每次扫描在 App 私有临时目录加载内置 Skill。Codex 使用隔离配置与只读沙箱，Claude
            关闭工具和 MCP；认证仍由 Agent 管理，登录失败时返回终端处理。Codex
            隔离配置使用其默认模型；Claude 隔离 API 模式需预先配置 API 环境凭据，不复用订阅登录。
          </p>
          <pre className="ai-skill">{skill}</pre>
        </details>
      </form>
      <div className="notice">
        <ShieldCheck size={17} />
        <span>
          AI 建议可能有误，默认不勾选。只有来自本次 Mole
          清单的建议可进入现有清理审阅；执行前仍需保护、身份、权限和占用检查及单独确认。移入废纸篓不会立即释放空间。
        </span>
      </div>
      {(ai.error || error || snapshot?.error) && (
        <p className="error-message" role="alert">
          {error || ai.error || snapshot?.error}
        </p>
      )}
      {snapshot && (
        <section className="card ai-report" aria-label="AI 扫描结果">
          <div className="ai-report-heading">
            <strong>
              {ai.running && <LoaderCircle size={15} className="spin" />}
              {snapshot.stage}
            </strong>
            <span className="muted">{dateTime(snapshot.startedAt)}</span>
          </div>
          <p className="muted">
            {shortPath(snapshot.root, home)} · {snapshot.evidence.length} 条线索 ·{' '}
            {snapshot.agentId}
          </p>
          {snapshot.report && <p className="ai-summary">{snapshot.report.summary}</p>}
          {!!snapshot.limitations.length && (
            <details className="ai-details" open={!snapshot.report}>
              <summary>扫描范围与未完成项</summary>
              <ul>
                {snapshot.limitations.map((s, i) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
            </details>
          )}
          {!!snapshot.report?.limitations.length && (
            <ul className="ai-limitations">
              {snapshot.report.limitations.map((s, i) => (
                <li key={i}>{s}</li>
              ))}
            </ul>
          )}
          {snapshot.state === 'succeeded' && !reviewable.length && (
            <p className="muted">
              没有当前可提交的建议，或清单已过期/受保护。可查看下方原因，必要时缩小目录重新扫描。
            </p>
          )}
          {!!findings.length && (
            <>
              <div className="list-toolbar ai-list-toolbar">
                <input
                  aria-label="搜索 AI 建议"
                  placeholder="搜索路径或建议"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <select
                  aria-label="筛选 AI 建议"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                >
                  <option value="all">全部建议（{findings.length}）</option>
                  {Object.entries(aiRecommendation).map(([key, label]) => (
                    <option key={key} value={key}>
                      {label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="ai-findings">
                {visible.map((f) => {
                  const e = snapshot.evidence.find((e) => e.id === f.evidenceId)
                  if (!e) return null
                  const allowed = reviewable.some((row) => row.id === e.id)
                  const protectedItem = protectedPaths.some(
                    (p) => e.path === p || e.path.startsWith(`${p}/`) || p.startsWith(`${e.path}/`),
                  )
                  return (
                    <article className="ai-finding" key={e.id}>
                      <div className="ai-finding-title">
                        <input
                          type="checkbox"
                          aria-label={`选择 AI 建议 ${e.path}`}
                          disabled={!allowed || !enabled || pending}
                          checked={allowed && selected.includes(e.id)}
                          onChange={(event) =>
                            setSelected((ids) =>
                              event.target.checked
                                ? [...ids, e.id]
                                : ids.filter((id) => id !== e.id),
                            )
                          }
                        />
                        <strong title={e.path}>{shortPath(e.path, home)}</strong>
                        <span className="tag">{aiRecommendation[f.recommendation]}</span>
                        <span className="ai-size">
                          {e.isDirectory && e.size == null ? '目录大小未知' : bytes(e.size)}
                        </span>
                      </div>
                      <p>{f.reason}</p>
                      <p className="muted">影响：{f.impact}</p>
                      <div className="ai-finding-footer">
                        <span className="muted">
                          {e.source === 'mole' ? 'Mole 候选 · 需独立复查' : '扩展线索 · 仅供核实'}
                          {protectedItem ? ' · 已保护' : ''}
                          {snapshot.expiresAt != null && now >= snapshot.expiresAt
                            ? ' · 清单已过期'
                            : ''}
                        </span>
                        <button
                          className="text-button"
                          disabled={!api.desktop}
                          onClick={() => onReveal(e.path)}
                        >
                          Finder <ArrowUpRight size={12} />
                        </button>
                      </div>
                    </article>
                  )
                })}
              </div>
              {!visible.length && <p className="muted">没有匹配的建议。</p>}
              <div className="ai-review-actions">
                <span className="muted">
                  已选 {chosen.length} 项；大小可能存在父子重复，不作为释放空间估算。
                </span>
                <button
                  className="button primary compact"
                  disabled={!enabled || pending || !chosen.length}
                  onClick={() => void review()}
                >
                  {pending ? '正在准备…' : `审阅所选 ${chosen.length} 项`}
                </button>
              </div>
            </>
          )}
          {snapshot.report && !findings.length && (
            <p className="muted">模型没有提出逐项建议；这不代表扫描范围内没有可清理内容。</p>
          )}
        </section>
      )}
      {!snapshot && (
        <p className="muted ai-empty">
          选择 Agent 和目录，确认元数据使用范围后开始。不会自动执行清理；结果短期保留在当前 App
          会话，任务历史保存在本机，重启后不会自动重发模型请求。
        </p>
      )}
    </div>
  )
}
