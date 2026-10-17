import { useEffect, useRef, useState, type CSSProperties } from 'react'
import {
  Activity,
  ArrowRight,
  ArrowUpRight,
  Box,
  Check,
  ChevronRight,
  Clock3,
  Code2,
  Cpu,
  FolderOpen,
  HardDrive,
  History,
  Info,
  LayoutGrid,
  LoaderCircle,
  MemoryStick,
  RefreshCw,
  Search,
  Settings2,
  ShieldCheck,
  Sparkles,
  Square,
  X,
} from 'lucide-react'
import * as api from './lib/api'
import { demoAnalysis, demoCleanup, demoCleanPreview, demoSnapshot } from './lib/demo'
import {
  bytes,
  dateTime,
  kindNames,
  measuredBytes,
  percent,
  shortPath,
  stateNames,
} from './lib/format'
import { Modal } from './components/Modal'
import { SettingsModal } from './components/SettingsModal'
import { CleanReview } from './components/CleanReview'
import { ReviewPlanDialog } from './components/ReviewPlanDialog'
import { EngineSummary } from './components/EngineSummary'
import { ExecutionHistory } from './components/ExecutionHistory'
import { ExecutionRecoveryNotice } from './components/ExecutionRecoveryNotice'
import { MoleHistoryView } from './components/MoleHistoryView'
import { ApplicationLibrary } from './components/ApplicationLibrary'
import { AnalysisExplorer } from './components/AnalysisExplorer'
import { AnalysisNavigation } from './components/AnalysisNavigation'
import { AnalysisContexts, type AnalysisExplorerHandle } from './lib/analysis-context'
import {
  recordAnalysisVisit,
  type AnalysisHistoryTarget,
  type AnalysisNavigationHistory,
} from './lib/analysis-navigation'
import type {
  Analysis,
  AppEntry,
  Bootstrap,
  Cleanup,
  CleanPreview,
  Page,
  Settings,
  Snapshot,
  TaskKind,
  TaskRecord,
  TaskResults,
  ReviewPlan,
  MoleHistory,
} from './types'

const navigation = [
  ['overview', LayoutGrid, '概览'],
  ['analyze', HardDrive, '空间分析'],
  ['cleanup', Sparkles, '清理'],
  ['apps', Box, '应用列表'],
  ['status', Activity, '系统状态'],
  ['history', History, '操作历史'],
] as const

function Empty({
  icon: Icon = FolderOpen,
  title,
  children,
}: {
  icon?: typeof FolderOpen
  title: string
  children: React.ReactNode
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon">
        <Icon size={20} strokeWidth={1.5} />
      </div>
      <h3>{title}</h3>
      <div className="muted">{children}</div>
    </div>
  )
}

function Metric({
  label,
  value,
  detail,
  icon: Icon,
  usage,
}: {
  label: string
  value: string
  detail: string
  icon: typeof Cpu
  usage?: number
}) {
  return (
    <div className="metric-card">
      <div className="metric-label">
        <Icon size={16} />
        {label}
      </div>
      <strong>{value}</strong>
      <span>{detail}</span>
      {usage != null && (
        <div className="meter">
          <div style={{ width: `${Math.max(0, Math.min(usage, 100))}%` }} />
        </div>
      )}
    </div>
  )
}

export default function App() {
  const [data, setData] = useState<Bootstrap | null>(null)
  const [page, setPage] = useState<Page>('overview')
  const [snapshot, setSnapshot] = useState<Snapshot | null>(api.desktop ? null : demoSnapshot)
  const [analysis, setAnalysis] = useState<Analysis | null>(api.desktop ? null : demoAnalysis)
  const [analysisNavigation, setAnalysisNavigation] = useState<AnalysisNavigationHistory>(() => ({
    paths: api.desktop ? [] : [demoAnalysis.path],
    index: api.desktop ? -1 : 0,
  }))
  const analysisContexts = useRef(new AnalysisContexts())
  const analysisExplorer = useRef<AnalysisExplorerHandle>(null)
  useEffect(() => {
    analysisContexts.current.retain(analysisNavigation.paths)
  }, [analysisNavigation])
  const [cleanup, setCleanup] = useState<Cleanup | null>(api.desktop ? null : demoCleanup)
  const [cleanPreview, setCleanPreview] = useState<CleanPreview | null>(
    api.desktop ? null : demoCleanPreview,
  )
  const [apps, setApps] = useState<AppEntry[] | null>(null)
  const [moleHistory, setMoleHistory] = useState<MoleHistory | null>(null)
  const [busy, setBusy] = useState<{ id: string; kind: TaskKind } | null>(null)
  const busyRef = useRef(false)
  const autoStatus = useRef(false)
  const observation = useRef(new AbortController())
  const historyRequest = useRef(0)
  const [settingsTab, setSettingsTab] = useState<'general' | 'protected' | 'about' | null>(null)
  const [error, setError] = useState('')
  const [accepted, setAccepted] = useState(false)
  const [saving, setSaving] = useState(false)
  const [path, setPath] = useState('')
  const [cancelPending, setCancelPending] = useState(false)
  const [plan, setPlan] = useState<ReviewPlan | null>(null)
  const [planLoading, setPlanLoading] = useState(false)
  const [recoveryNavigation, setRecoveryNavigation] = useState(0)
  const handledRecoveryNavigation = useRef(0)

  useEffect(() => {
    if (page !== 'history' || recoveryNavigation === handledRecoveryNavigation.current) return
    const section = document.getElementById('execution-records')
    if (!section) return
    handledRecoveryNavigation.current = recoveryNavigation
    section?.focus({ preventScroll: true })
    section?.scrollIntoView({ block: 'start' })
  }, [page, recoveryNavigation])

  async function load() {
    try {
      const initial = await api.bootstrap()
      if (observation.current.signal.aborted) return
      const active = initial.history.filter(
        (task) => task.state === 'running' && Object.hasOwn(kindNames, task.kind),
      )
      if (api.desktop && active.length) autoStatus.current = true
      setData(initial)
      if (api.desktop && initial.latestCleanPreview) setCleanPreview(initial.latestCleanPreview)
      setPath(initial.home)
      setError('')
      if (api.desktop && active.length) void followTasks(active)
    } catch (error) {
      setError(String(error))
    }
  }
  useEffect(() => {
    void load()
    const stop = () => observation.current.abort()
    window.addEventListener('pagehide', stop)
    return () => {
      window.removeEventListener('pagehide', stop)
      stop()
    }
  }, [])
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if (event.metaKey && event.key === ',' && data?.settings.acknowledged) {
        event.preventDefault()
        setSettingsTab('general')
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [data?.settings.acknowledged])
  useEffect(() => {
    const preference = data?.settings.theme ?? 'system'
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const apply = () => {
      document.documentElement.dataset.theme =
        preference === 'system' ? (media.matches ? 'dark' : 'light') : preference
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [data?.settings.theme])
  useEffect(() => {
    if (
      api.desktop &&
      data?.settings.acknowledged &&
      data.engine.available &&
      !autoStatus.current
    ) {
      autoStatus.current = true
      void execute('status')
    }
  }, [data?.settings.acknowledged, data?.engine.available])

  async function execute<K extends TaskKind>(
    kind: K,
    target?: string,
    analysisTarget?: AnalysisHistoryTarget,
  ) {
    if (busyRef.current) return
    busyRef.current = true
    setError('')
    setCancelPending(false)
    try {
      const result = await api.runTask(
        kind,
        target,
        (id) => {
          setBusy({ id, kind })
          void refreshRecords().catch((error) => setError(`历史记录无法刷新：${String(error)}`))
        },
        observation.current.signal,
      )
      applyTaskResult(kind, result, analysisTarget)
    } catch (error) {
      if (!observation.current.signal.aborted) setError(String(error).replace(/^Error: /, ''))
    } finally {
      if (observation.current.signal.aborted) return
      setBusy(null)
      busyRef.current = false
      setCancelPending(false)
      try {
        await refreshRecords(kind === 'clean')
      } catch (error) {
        setError(`历史记录无法刷新：${String(error)}`)
      }
    }
  }

  function applyTaskResult(
    kind: TaskKind,
    result: TaskResults[TaskKind],
    analysisTarget?: AnalysisHistoryTarget,
  ) {
    if (kind === 'status') setSnapshot(result as Snapshot)
    if (kind === 'analyze') {
      const value = result as Analysis
      rememberAnalysisContext()
      setAnalysis({ ...value })
      setPath(value.path)
      setAnalysisNavigation((current) => recordAnalysisVisit(current, value.path, analysisTarget))
    }
    if (kind === 'apps') setApps([...(result as AppEntry[])])
    if (kind === 'cleanup') setCleanup(result as Cleanup)
    if (kind === 'clean') setCleanPreview(result as CleanPreview)
    if (kind === 'history') setMoleHistory(result as MoleHistory)
  }

  async function refreshRecords(includePlans = false) {
    const request = ++historyRequest.current
    const history = await api.history()
    if (!observation.current.signal.aborted && request === historyRequest.current)
      setData((current) => (current ? { ...current, history } : current))
    if (includePlans) {
      const reviewPlans = await api.reviewPlans()
      if (!observation.current.signal.aborted)
        setData((current) => (current ? { ...current, reviewPlans } : current))
    }
  }

  async function followTasks(tasks: TaskRecord[]) {
    if (busyRef.current || !tasks.length) return
    busyRef.current = true
    setCancelPending(false)
    setError('')
    const pending = new Map(tasks.map((task) => [task.id, task]))
    setBusy({ id: tasks[0].id, kind: tasks[0].kind })
    await Promise.all(
      tasks.map(async (task) => {
        try {
          const result = await api.resumeTask(task.id, task.kind, observation.current.signal)
          applyTaskResult(task.kind, result)
        } catch (error) {
          if (!observation.current.signal.aborted) setError(String(error).replace(/^Error: /, ''))
        } finally {
          pending.delete(task.id)
          if (!observation.current.signal.aborted) {
            const next = pending.values().next().value
            setBusy(next ? { id: next.id, kind: next.kind } : null)
            setCancelPending(false)
            try {
              await refreshRecords(task.kind === 'clean')
            } catch (error) {
              setError(`历史记录无法刷新：${String(error)}`)
            }
          }
        }
      }),
    )
    busyRef.current = false
  }

  async function chooseAndAnalyze() {
    try {
      const selected = await api.chooseDirectory()
      if (selected) {
        setPage('analyze')
        setPath(selected)
        void execute('analyze', selected)
      }
    } catch (error) {
      setError(String(error))
    }
  }
  function visitAnalysis(index: number) {
    const target = analysisNavigation.paths[index]
    if (target == null || index < 0 || !Number.isInteger(index)) return
    void execute('analyze', target, { index, path: target })
  }
  async function updateSettings(settings: Settings) {
    const saved = await api.saveSettings(settings)
    setData((current) => (current ? { ...current, settings: saved } : current))
    setCleanup((current) =>
      current
        ? {
            ...current,
            items: current.items.map((item) => ({
              ...item,
              protected: saved.protectedPaths.some(
                (path) =>
                  path === '/' ||
                  item.path === path ||
                  item.path.startsWith(`${path}/`) ||
                  path.startsWith(`${item.path}/`),
              ),
            })),
          }
        : null,
    )
    await refreshRecords()
  }
  async function acknowledge() {
    if (!data) return
    setSaving(true)
    try {
      await updateSettings({ ...data.settings, acknowledged: true })
      setError('')
    } catch (error) {
      setError(String(error))
    } finally {
      setSaving(false)
    }
  }
  async function protect(target: string) {
    if (!data) return
    try {
      await updateSettings({
        ...data.settings,
        protectedPaths: [...new Set([...data.settings.protectedPaths, target])],
      })
    } catch (error) {
      setError(String(error))
    }
  }
  async function createPlan(scanId: string, ids: string[]) {
    const saved = await api.createReviewPlan(scanId, ids)
    setPlan(saved)
    const reviewPlans = await api.reviewPlans()
    setData((current) => (current ? { ...current, reviewPlans } : current))
  }
  async function openPlan(id: string) {
    setPlanLoading(true)
    try {
      setPlan(await api.reviewPlan(id))
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      setPlanLoading(false)
    }
  }
  function reveal(target: string) {
    void api.reveal(target).catch((error) => setError(String(error).replace(/^Error: /, '')))
  }
  function official() {
    void api.official().catch((error) => setError(String(error)))
  }
  function changePage(next: Page) {
    rememberAnalysisContext()
    setPage(next)
  }
  function rememberAnalysisContext() {
    const current = analysisExplorer.current?.capture()
    if (current) analysisContexts.current.remember(current.path, current.context)
  }

  if (!data)
    return (
      <div className="startup">
        <div className="brand-mark">
          m<span>·</span>
        </div>
        <h1>MooMole</h1>
        {error ? (
          <>
            <p role="alert">{error}</p>
            <button className="button primary" onClick={() => void load()}>
              重新加载
            </button>
          </>
        ) : (
          <p>
            <LoaderCircle className="spin" size={16} />
            正在打开本地工作区…
          </p>
        )}
      </div>
    )

  const canRun = api.desktop && data.engine.available && !busy
  const disk =
    snapshot?.disks?.find((disk) => disk.mount === '/') ??
    snapshot?.disks?.find((disk) => !disk.external)
  const diskPercent =
    disk && disk.total > 0 ? Math.max(0, Math.min(100, (disk.used / disk.total) * 100)) : 0
  const pageName = navigation.find(([id]) => id === page)![2]
  const lastSnapshot = snapshot ? dateTime(snapshot.collected_at) : null
  const totalCandidates = cleanup?.items.reduce((sum, item) => sum + (item.size ?? 0), 0)

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-drag" data-tauri-drag-region />
        <div className="brand">
          <div className="brand-mark">
            m<span>·</span>
          </div>
          <div>MooMole</div>
        </div>
        <nav className="main-nav" aria-label="主导航">
          {navigation.map(([id, Icon, name]) => (
            <button
              key={id}
              className={page === id ? 'active' : ''}
              onClick={() => changePage(id)}
              aria-current={page === id ? 'page' : undefined}
            >
              <Icon size={18} strokeWidth={1.7} />
              <span>{name}</span>
              {page === id && <span className="active-dot" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <button className="sidebar-settings" onClick={() => setSettingsTab('general')}>
            <Settings2 size={17} />
            设置<span>⌘ ,</span>
          </button>
          <button
            className="sidebar-about"
            aria-label="关于与使用说明"
            onClick={() => setSettingsTab('about')}
          >
            <Info size={17} />
            关于 MooMole <span>v0.1.0</span>
          </button>
          <button className="sidebar-official" onClick={official}>
            Mole 官方产品
            <ArrowUpRight size={12} />
          </button>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar" data-tauri-drag-region aria-label={`${pageName}工具栏`}>
          <div className="toolbar-actions">
            {page === 'analyze' ? (
              <>
                {analysis && (
                  <AnalysisNavigation
                    path={analysis.path}
                    history={analysisNavigation}
                    disabled={!canRun}
                    onVisit={visitAnalysis}
                    onAnalyze={(target) => void execute('analyze', target)}
                  />
                )}
                <form
                  className="path-bar"
                  onSubmit={(event) => {
                    event.preventDefault()
                    if (canRun) void execute('analyze', path)
                  }}
                >
                  <input
                    aria-label="扫描目录"
                    value={path}
                    onChange={(event) => setPath(event.target.value)}
                    placeholder="输入绝对路径或 ~/目录"
                  />
                  <button
                    className="button secondary compact"
                    disabled={!canRun || !path.trim()}
                    type="submit"
                  >
                    {busy?.kind === 'analyze' ? (
                      <LoaderCircle size={14} className="spin" />
                    ) : (
                      <Search size={14} />
                    )}
                    开始分析
                  </button>
                </form>
                <button
                  className="icon-button"
                  disabled={!canRun}
                  onClick={() => void chooseAndAnalyze()}
                  aria-label="选择文件夹"
                  title="选择文件夹"
                >
                  <FolderOpen size={17} />
                </button>
              </>
            ) : (
              <>
                {page === 'overview' && (
                  <button
                    className="button primary compact"
                    disabled={!canRun}
                    onClick={() => void chooseAndAnalyze()}
                  >
                    <FolderOpen size={15} />
                    选择文件夹
                  </button>
                )}
                {(page === 'overview' || page === 'status') && (
                  <button
                    className="button secondary compact"
                    disabled={!canRun}
                    onClick={() => void execute('status')}
                  >
                    <RefreshCw size={15} className={busy?.kind === 'status' ? 'spin' : ''} />
                    {page === 'overview' ? '刷新状态' : '刷新快照'}
                  </button>
                )}
                {page === 'cleanup' && (
                  <button
                    className="button primary compact"
                    disabled={!canRun}
                    onClick={() => void execute('clean')}
                  >
                    <Search size={15} />
                    {cleanPreview ? '重新扫描' : '扫描清理预览'}
                  </button>
                )}
                {page === 'apps' && (
                  <button
                    className="button secondary compact"
                    disabled={!canRun}
                    onClick={() => void execute('apps')}
                  >
                    <RefreshCw size={15} />
                    {apps ? '刷新列表' : '读取应用'}
                  </button>
                )}
                {page === 'history' && (
                  <span className="toolbar-note">
                    客户端任务保留 {data.settings.retentionDays} 天
                  </span>
                )}
              </>
            )}
          </div>
          <span className="read-only-tag">
            <ShieldCheck size={13} />
            只读检查
          </span>
        </header>
        <main className="main-content" aria-label={`${pageName}内容`}>
          <ExecutionRecoveryNotice
            records={data.executions}
            onReview={() => {
              changePage('history')
              setRecoveryNavigation((value) => value + 1)
            }}
          />
          {!api.desktop && (
            <div className="preview-banner">
              <Info size={15} />
              <span>
                <strong>浏览器预览</strong> · 以下为示例数据，不代表本机状态。真实检查请启动桌面
                App。
              </span>
            </div>
          )}
          {api.desktop && !data.engine.available && (
            <div className="notice warning">
              <Info size={17} />
              <span>{data.engine.reason}</span>
            </div>
          )}
          {error && (
            <div className="notice error" role="alert">
              <Info size={17} />
              <span>{error}</span>
              <button className="icon-button" aria-label="关闭提示" onClick={() => setError('')}>
                <X size={15} />
              </button>
            </div>
          )}

          {page === 'overview' && (
            <>
              <EngineSummary engine={data.engine} desktop={api.desktop} />
              <section className="storage-card card">
                <div className="storage-main">
                  <div className="section-top">
                    <div>
                      <span className="section-label">磁盘空间</span>
                      <h2>{snapshot?.host ?? '这台 Mac'}</h2>
                    </div>
                    <span className="tag">{disk ? '启动磁盘' : '等待快照'}</span>
                  </div>
                  <div className="storage-detail">
                    <div
                      className="storage-ring"
                      style={{ '--used': `${diskPercent}%` } as CSSProperties}
                    >
                      <div>
                        <strong>{disk ? `${diskPercent.toFixed(0)}%` : '—'}</strong>
                        <span>已使用</span>
                      </div>
                    </div>
                    <div className="storage-numbers">
                      <div className="disk-free">
                        <strong>{disk ? bytes(disk.total - disk.used) : '—'}</strong>
                        <span>可用空间</span>
                      </div>
                      <p>
                        共 {disk ? bytes(disk.total) : '—'} ·{' '}
                        {snapshot?.hardware?.cpu_model ?? '读取后显示设备信息'}
                      </p>
                      <div className="disk-legend">
                        <span>
                          <i />
                          已使用 {bytes(disk?.used)}
                        </span>
                        <span>
                          <i />
                          可用
                        </span>
                      </div>
                    </div>
                  </div>
                </div>
              </section>
              <div className="metrics-grid">
                <Metric
                  label="CPU 使用率"
                  value={percent(snapshot?.cpu?.usage)}
                  detail={
                    snapshot?.cpu?.logical_cpu
                      ? `${snapshot.cpu.logical_cpu} 个逻辑核心`
                      : '等待系统快照'
                  }
                  icon={Cpu}
                  usage={snapshot?.cpu?.usage}
                />
                <Metric
                  label="内存占用"
                  value={bytes(snapshot?.memory?.used)}
                  detail={`总内存 ${bytes(snapshot?.memory?.total)}`}
                  icon={MemoryStick}
                  usage={snapshot?.memory?.used_percent}
                />
                <Metric
                  label="运行时间"
                  value={snapshot?.uptime ?? '—'}
                  detail={lastSnapshot ? `快照更新于 ${lastSnapshot}` : '刷新后查看当前状态'}
                  icon={Clock3}
                />
              </div>
            </>
          )}

          {page === 'analyze' && (
            <>
              {analysis ? (
                <AnalysisExplorer
                  key={`${analysisNavigation.index}:${analysis.path}`}
                  ref={analysisExplorer}
                  initialContext={analysisContexts.current.get(analysis.path)}
                  analysis={analysis}
                  home={data.home}
                  canAnalyze={canRun}
                  canReveal={api.desktop}
                  refreshing={busy?.kind === 'analyze'}
                  onAnalyze={(target) => void execute('analyze', target)}
                  onReveal={reveal}
                />
              ) : (
                <section className="card">
                  <Empty title="先选择一个文件夹">
                    <p>
                      可以从下载目录或某个项目开始。
                      <br />
                      分析过程不会删除或修改目标文件。
                    </p>
                    <button
                      className="button secondary"
                      disabled={!canRun}
                      onClick={() => void execute('analyze', data.home)}
                    >
                      分析个人目录
                      <ArrowRight size={14} />
                    </button>
                  </Empty>
                </section>
              )}
            </>
          )}

          {page === 'cleanup' && (
            <>
              <div className="notice">
                <ShieldCheck size={18} />
                <span>当前只提供扫描与审阅，不会删除文件；需要管理员权限的项目会跳过。</span>
              </div>
              {cleanPreview ? (
                <CleanReview
                  preview={cleanPreview}
                  protectedPaths={data.settings.protectedPaths}
                  home={data.home}
                  desktop={api.desktop}
                  onProtect={protect}
                  onReveal={reveal}
                  onCreatePlan={createPlan}
                />
              ) : (
                <section className="card">
                  <Empty icon={Sparkles} title="尚未扫描">
                    <p>
                      点击“扫描清理预览”，读取 Mole 的候选项目。
                      <br />
                      按分类筛选、展开路径、勾选审阅，并保护想保留的目录。
                    </p>
                  </Empty>
                </section>
              )}
              <div className="section-heading cache-section-heading">
                <h2>开发缓存详情</h2>
                <button
                  className="button secondary compact"
                  disabled={!canRun}
                  onClick={() => void execute('cleanup')}
                >
                  <Search size={13} />
                  检查常见开发缓存
                </button>
              </div>
              {cleanup ? (
                <>
                  <div className="cleanup-total">
                    <span className="section-label">本次已测量缓存大小</span>
                    <strong>
                      {bytes(totalCandidates)}
                      {cleanup.items.some((item) => item.scanStatus === 'partial') ? '+' : ''}
                    </strong>
                    <p>目录占用不等于实际可释放空间，未测量的项目未计入。</p>
                  </div>
                  <div className="cache-list">
                    {cleanup.items.map((item) => (
                      <article className="card cache-card" key={item.id}>
                        <div
                          className={`action-icon ${item.id === 'xcode' ? 'lavender' : item.id === 'npm' ? 'peach' : 'mint'}`}
                        >
                          <Code2 size={22} />
                        </div>
                        <div className="cache-description">
                          <h3>
                            {item.name}
                            {item.protected && (
                              <span className="tag">
                                <ShieldCheck size={11} />
                                已保护
                              </span>
                            )}
                          </h3>
                          <p>{item.impact}</p>
                          <span className="mono path-caption" title={item.path}>
                            {shortPath(item.path, data.home)}
                          </span>
                          {item.reason && <small className="muted">{item.reason}</small>}
                        </div>
                        <div className="cache-actions">
                          <strong>{measuredBytes(item.size, item.scanStatus)}</strong>
                          <div>
                            {item.size != null && (
                              <button
                                className="button text"
                                disabled={!api.desktop}
                                onClick={() => reveal(item.path)}
                              >
                                Finder
                                <ArrowUpRight size={12} />
                              </button>
                            )}
                            {!item.protected && item.size != null && (
                              <button
                                className="icon-button"
                                disabled={!api.desktop}
                                onClick={() => void protect(item.path)}
                                aria-label={`保护 ${item.name}`}
                              >
                                <ShieldCheck size={15} />
                              </button>
                            )}
                          </div>
                        </div>
                      </article>
                    ))}
                  </div>
                </>
              ) : (
                <section className="card">
                  <Empty icon={Sparkles} title="查看常见开发缓存的占用">
                    <p>
                      检查 Xcode、npm 和 Homebrew 的缓存占用。
                      <br />
                      运行中或身份不明确的目录应保留，不能仅凭大小判断可删除。
                    </p>
                  </Empty>
                </section>
              )}
              <button
                className="button text protected-link"
                onClick={() => setSettingsTab('protected')}
              >
                <ShieldCheck size={14} />
                管理保护列表
                <ChevronRight size={13} />
              </button>
            </>
          )}

          {page === 'apps' && (
            <>
              <div className="inline-note">
                <Info size={14} />
                列表来自 Mole CLI；仅供查看和对比，不会卸载。首次读取可能需要较长时间。
              </div>
              {apps ? (
                <ApplicationLibrary
                  apps={apps}
                  home={data.home}
                  refreshing={busy?.kind === 'apps'}
                  canReveal={api.desktop}
                  onReveal={reveal}
                />
              ) : (
                <section className="card">
                  <Empty icon={Box} title="尚未读取应用">
                    <p>
                      点击“读取应用”获取 Mole CLI 的应用清单。
                      <br />
                      未知大小会保留原始标记，不计为零。
                    </p>
                  </Empty>
                </section>
              )}
            </>
          )}

          {page === 'status' && (
            <>
              {snapshot ? (
                <>
                  <div className="device-card card">
                    <span className="device-icon">
                      <Cpu size={28} strokeWidth={1.4} />
                    </span>
                    <div>
                      <h2>{snapshot.host ?? '这台 Mac'}</h2>
                      <p>
                        {snapshot.hardware?.cpu_model ?? '硬件信息不可用'} ·{' '}
                        {snapshot.hardware?.total_ram ?? '内存未知'} ·{' '}
                        {snapshot.hardware?.os_version ?? snapshot.platform ?? 'macOS'}
                      </p>
                    </div>
                    <span className="tag">{api.desktop ? '本机快照' : '示例快照'}</span>
                  </div>
                  <div className="metrics-grid">
                    <Metric
                      label="CPU 使用率"
                      value={percent(snapshot.cpu?.usage)}
                      detail={`${snapshot.cpu?.logical_cpu ?? '—'} 个逻辑核心`}
                      icon={Cpu}
                      usage={snapshot.cpu?.usage}
                    />
                    <Metric
                      label="内存占用"
                      value={bytes(snapshot.memory?.used)}
                      detail={`总内存 ${bytes(snapshot.memory?.total)}`}
                      icon={MemoryStick}
                      usage={snapshot.memory?.used_percent}
                    />
                    <Metric
                      label="运行时间"
                      value={snapshot.uptime ?? '—'}
                      detail={`采集时间 ${lastSnapshot}`}
                      icon={Clock3}
                    />
                  </div>
                  <section className="card">
                    <div className="list-toolbar">
                      <h2>进程快照</h2>
                      <span className="muted">
                        {snapshot.process_stale ? '复用的较早样本' : '按 CPU 占用展示'}
                      </span>
                    </div>
                    {snapshot.top_processes?.length ? (
                      snapshot.top_processes.slice(0, 12).map((process) => (
                        <div className="process-row" key={process.pid}>
                          <span className="process-icon">
                            <Activity size={15} />
                          </span>
                          <strong>{process.name}</strong>
                          <span className="mono">PID {process.pid}</span>
                          <span>{percent(process.cpu)}</span>
                          <span>{bytes(process.memory_bytes)}</span>
                        </div>
                      ))
                    ) : (
                      <div className="empty-inline">当前没有可用的进程数据</div>
                    )}
                  </section>
                </>
              ) : (
                <section className="card">
                  <Empty icon={Activity} title="读取一份系统快照">
                    <p>
                      CPU、内存和进程信息通过 Mole 采集。
                      <br />
                      缺失指标会显示为空缺，不代入模拟数值。
                    </p>
                  </Empty>
                </section>
              )}
            </>
          )}

          {page === 'history' && (
            <>
              <MoleHistoryView
                report={moleHistory}
                disabled={!!busy || !api.desktop || !data.engine.available}
                reading={busy?.kind === 'history'}
                onRead={() => void execute('history')}
              />
              {data.reviewPlans.length > 0 && (
                <section className="card saved-plans" aria-label="已保存的审阅计划">
                  <div className="list-toolbar">
                    <h2>审阅计划</h2>
                    <span className="muted">仅保存选中范围，尚未执行</span>
                  </div>
                  {data.reviewPlans.map((item) => (
                    <button
                      className="saved-plan-row"
                      key={item.id}
                      disabled={planLoading}
                      onClick={() => void openPlan(item.id)}
                    >
                      <ShieldCheck size={17} />
                      <span>
                        <strong>
                          {item.itemCount} 项 · 计划 {item.id.slice(0, 8)}
                        </strong>
                        <small>
                          保存于 {dateTime(item.createdAt)} ·{' '}
                          {Date.now() >= item.expiresAt
                            ? '已过期，需重新扫描'
                            : `到期 ${dateTime(item.expiresAt)}`}
                        </small>
                      </span>
                      <span>查看并复核</span>
                      <ChevronRight size={14} />
                    </button>
                  ))}
                </section>
              )}
              <ExecutionHistory
                initialRecords={data.executions}
                loadRecords={api.executions}
                loadDetail={api.execution}
                onRecordsChange={(executions) =>
                  setData((current) => (current ? { ...current, executions } : current))
                }
              />
              <section className="card">
                <div className="list-toolbar">
                  <h2>客户端检查任务</h2>
                </div>
                {data.history.length ? (
                  <>
                    {data.history.map((task) => (
                      <div className="history-row" key={task.id}>
                        <span className="history-icon">
                          {task.state === 'succeeded' ? <Check size={17} /> : <Clock3 size={17} />}
                        </span>
                        <div className="history-info">
                          <strong>{kindNames[task.kind] ?? task.kind}</strong>
                          <span title={task.path ?? task.message ?? ''}>
                            {task.path
                              ? shortPath(task.path, data.home)
                              : (task.message ?? '只读任务')}
                          </span>
                          {task.state !== 'succeeded' && task.path && <small>{task.message}</small>}
                        </div>
                        <time>{dateTime(task.createdAt)}</time>
                        <span className={`state-tag ${task.state}`}>
                          {stateNames[task.state] ?? task.state}
                        </span>
                        {api.desktop &&
                          task.state === 'running' &&
                          Object.hasOwn(kindNames, task.kind) && (
                            <button
                              className="button secondary"
                              disabled={!!busy}
                              onClick={() => void followTasks([task])}
                            >
                              继续跟踪
                            </button>
                          )}
                      </div>
                    ))}
                  </>
                ) : (
                  <Empty icon={History} title="从第一次检查开始">
                    <p>
                      完成空间分析、缓存检查或读取系统状态后，
                      <br />
                      任务会记录在这里。
                    </p>
                  </Empty>
                )}
              </section>
            </>
          )}
        </main>
      </div>
      {plan && (
        <ReviewPlanDialog
          key={plan.id}
          initialPlan={plan}
          onRefresh={api.reviewPlan}
          onCheckRules={api.checkReviewRules}
          onClose={() => setPlan(null)}
        />
      )}
      {busy && (
        <div className="task-toast" role="status">
          <LoaderCircle size={18} className="spin" />
          <div>
            <strong>{kindNames[busy.kind]}进行中</strong>
            <span>
              {cancelPending ? '正在停止任务进程…' : '检查在本机运行，你可以继续浏览其他页面。'}
            </span>
          </div>
          <button
            className="button secondary compact"
            disabled={cancelPending}
            onClick={() => {
              setCancelPending(true)
              void api.cancelTask(busy.id).catch((error) => {
                setCancelPending(false)
                setError(String(error))
              })
            }}
          >
            <Square size={11} />
            取消
          </button>
        </div>
      )}
      {settingsTab && (
        <SettingsModal
          key={settingsTab}
          initialTab={settingsTab}
          settings={data.settings}
          revision={data.engine.revision}
          version={data.engine.version}
          onSave={updateSettings}
          onClose={() => setSettingsTab(null)}
        />
      )}
      {!data.settings.acknowledged && (
        <Modal title="欢迎使用 MooMole" className="welcome-modal">
          <div className="welcome-symbol">
            <ShieldCheck size={30} strokeWidth={1.4} />
          </div>
          <p>
            MooMole
            是独立的非官方学习研究项目。使用前请理解操作影响并备份重要数据，由此产生的问题与使用风险由使用者自行承担。
          </p>
          <p>
            部分能力基于 Mole CLI，感谢原作者及贡献者。为尊重和支持 Mole
            的开发成果，日常使用建议选择官方产品。
          </p>
          <div className="welcome-note">
            <ShieldCheck size={16} />
            当前版本仅进行只读检查，不提供删除或系统维护。
          </div>
          <label className="acceptance">
            <input
              type="checkbox"
              checked={accepted}
              onChange={(event) => setAccepted(event.target.checked)}
            />
            我已了解项目用途及使用风险
          </label>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <div className="welcome-actions">
            <button className="button text" onClick={official}>
              Mole 官方产品
              <ArrowUpRight size={14} />
            </button>
            <button
              className="button primary"
              disabled={!accepted || saving}
              onClick={() => void acknowledge()}
            >
              {saving ? <LoaderCircle className="spin" size={15} /> : null}开始使用
              <ArrowRight size={15} />
            </button>
          </div>
        </Modal>
      )}
    </div>
  )
}
