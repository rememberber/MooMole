import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import {
  Activity,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUpRight,
  Box,
  Check,
  ChevronRight,
  Clock3,
  Code2,
  Cpu,
  ExternalLink,
  File,
  Folder,
  FolderOpen,
  HardDrive,
  History,
  Home,
  Info,
  LayoutGrid,
  LoaderCircle,
  LockKeyhole,
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
  ReviewPlan,
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
        <Icon size={27} strokeWidth={1.5} />
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
  const [cleanup, setCleanup] = useState<Cleanup | null>(api.desktop ? null : demoCleanup)
  const [cleanPreview, setCleanPreview] = useState<CleanPreview | null>(
    api.desktop ? null : demoCleanPreview,
  )
  const [apps, setApps] = useState<AppEntry[] | null>(null)
  const [busy, setBusy] = useState<{ id: string; kind: TaskKind } | null>(null)
  const busyRef = useRef(false)
  const autoStatus = useRef(false)
  const [settingsTab, setSettingsTab] = useState<'general' | 'protected' | 'about' | null>(null)
  const [error, setError] = useState('')
  const [accepted, setAccepted] = useState(false)
  const [saving, setSaving] = useState(false)
  const [path, setPath] = useState('')
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<'size' | 'name'>('size')
  const [cancelPending, setCancelPending] = useState(false)
  const [plan, setPlan] = useState<ReviewPlan | null>(null)
  const [planLoading, setPlanLoading] = useState(false)

  async function load() {
    try {
      const initial = await api.bootstrap()
      setData(initial)
      if (api.desktop && initial.latestCleanPreview) setCleanPreview(initial.latestCleanPreview)
      setPath(initial.home)
      setError('')
    } catch (error) {
      setError(String(error))
    }
  }
  useEffect(() => {
    void load()
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

  async function execute<K extends TaskKind>(kind: K, target?: string) {
    if (busyRef.current) return
    busyRef.current = true
    setError('')
    setCancelPending(false)
    try {
      const result = await api.runTask(kind, target, (id) => setBusy({ id, kind }))
      if (kind === 'status') setSnapshot(result as Snapshot)
      if (kind === 'analyze') {
        const value = result as Analysis
        setAnalysis(value)
        setPath(value.path)
        setQuery('')
      }
      if (kind === 'apps') setApps(result as AppEntry[])
      if (kind === 'cleanup') setCleanup(result as Cleanup)
      if (kind === 'clean') setCleanPreview(result as CleanPreview)
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      setBusy(null)
      busyRef.current = false
      setCancelPending(false)
      try {
        const history = await api.history()
        const reviewPlans = kind === 'clean' ? await api.reviewPlans() : null
        setData((current) =>
          current ? { ...current, history, ...(reviewPlans ? { reviewPlans } : {}) } : current,
        )
      } catch (error) {
        setError(`历史记录无法刷新：${String(error)}`)
      }
    }
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
    const history = await api.history()
    setData((current) => (current ? { ...current, history } : current))
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
    setPage(next)
    setQuery('')
  }

  const entries = useMemo(
    () =>
      [...(analysis?.entries ?? [])]
        .filter((item) => item.name.toLowerCase().includes(query.toLowerCase()))
        .sort((a, b) =>
          sort === 'size' ? b.size - a.size : a.name.localeCompare(b.name, 'zh-CN'),
        ),
    [analysis, query, sort],
  )
  const filteredApps = useMemo(
    () =>
      (apps ?? []).filter((app) =>
        `${app.name} ${app.bundle_id}`.toLowerCase().includes(query.toLowerCase()),
      ),
    [apps, query],
  )

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
  const actionCards = [
    {
      title: '看见每一份占用',
      text: '逐层浏览目录，找到空间的去向。',
      icon: HardDrive,
      className: 'lavender',
      page: 'analyze' as Page,
      caption: '空间分析',
    },
    {
      title: '先预览，再清理',
      text: '把 Mole 的清理结果，放到眼前审阅。',
      icon: Sparkles,
      className: 'peach',
      page: 'cleanup' as Page,
      caption: '清理预览',
    },
    {
      title: '整理应用清单',
      text: '查看已安装应用和它们的来源。',
      icon: Box,
      className: 'mint',
      page: 'apps' as Page,
      caption: '应用列表',
    },
  ]

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-drag" data-tauri-drag-region />
        <div className="brand">
          <div className="brand-mark">
            m<span>·</span>
          </div>
          <div>
            MooMole<small>Mole CLI 桌面客户端</small>
          </div>
        </div>
        <div className="nav-label">工作空间</div>
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
          <div className="local-card">
            <span className="local-dot" />
            <div>
              本地运行<small>你的文件留在你的 Mac</small>
            </div>
            <ShieldCheck size={15} />
          </div>
          <button className="sidebar-settings" onClick={() => setSettingsTab('general')}>
            <Settings2 size={17} />
            设置<span>⌘ ,</span>
          </button>
          <button className="sidebar-about" onClick={() => setSettingsTab('about')}>
            学习研究版 <span>v0.1.0</span>
          </button>
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar" data-tauri-drag-region>
          <div className="breadcrumb">
            <Home size={14} />
            <ChevronRight size={12} />
            <span>{pageName}</span>
          </div>
          <div className="topbar-right">
            <span className="read-only-tag">
              <ShieldCheck size={13} />
              只读检查
            </span>
            <button
              className="icon-button"
              onClick={() => setSettingsTab('about')}
              aria-label="关于与使用说明"
            >
              <Info size={17} />
            </button>
          </div>
        </header>
        <main className="main-content">
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
              <div className="page-heading overview-heading">
                <div>
                  <div className="eyebrow">A LITTLE SPACE. A LITTLE CLARITY.</div>
                  <h1>把空间，还给重要的事。</h1>
                  <p>从看清占用开始，让每一次整理都有据可循。</p>
                </div>
                <button
                  className="button secondary"
                  disabled={!canRun}
                  onClick={() => void execute('status')}
                >
                  <RefreshCw size={15} className={busy?.kind === 'status' ? 'spin' : ''} />
                  刷新状态
                </button>
              </div>
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
                <div className="storage-aside">
                  <div className="folder-art">
                    <FolderOpen size={42} strokeWidth={1.2} />
                    <span className="art-spark">
                      <Sparkles size={14} />
                    </span>
                  </div>
                  <h3>从一个文件夹开始</h3>
                  <p>
                    选择你想了解的目录，
                    <br />
                    看看空间都用在了哪里。
                  </p>
                  <button
                    className="button primary"
                    disabled={!canRun}
                    onClick={() => void chooseAndAnalyze()}
                  >
                    选择文件夹
                    <ArrowUpRight size={15} />
                  </button>
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
              <div className="section-heading">
                <h2>按你的节奏，慢慢整理</h2>
                <span>先了解，再决定</span>
              </div>
              <div className="action-grid">
                {actionCards.map(({ title, text, icon: Icon, className, page, caption }) => (
                  <button className="action-card" key={page} onClick={() => changePage(page)}>
                    <div className={`action-icon ${className}`}>
                      <Icon size={23} strokeWidth={1.5} />
                    </div>
                    <h3>{title}</h3>
                    <p>{text}</p>
                    <span>
                      {caption}
                      <ArrowRight size={15} />
                    </span>
                  </button>
                ))}
              </div>
              <div className="quiet-note">
                <LockKeyhole size={14} />
                <span>当前版本仅检查与展示，不删除文件，不执行系统维护。</span>
              </div>
            </>
          )}

          {page === 'analyze' && (
            <>
              <div className="page-heading">
                <div>
                  <div className="eyebrow">DISK EXPLORER</div>
                  <h1>空间分析</h1>
                  <p>沿着目录慢慢下钻，让每一份占用都清晰可见。</p>
                </div>
                <button
                  className="button primary"
                  disabled={!canRun}
                  onClick={() => void chooseAndAnalyze()}
                >
                  <FolderOpen size={15} />
                  选择文件夹
                </button>
              </div>
              <form
                className="path-bar"
                onSubmit={(event) => {
                  event.preventDefault()
                  if (canRun) void execute('analyze', path)
                }}
              >
                <Folder size={17} />
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
              {analysis ? (
                <>
                  <div className="analysis-summary">
                    <div>
                      <span className="section-label">已测量占用</span>
                      <strong>{measuredBytes(analysis.total_size, analysis.scan_status)}</strong>
                    </div>
                    <div>
                      <span>{shortPath(analysis.path, data.home)}</span>
                      <small>
                        {analysis.entries.length} 个直接子项 ·{' '}
                        {analysis.scan_status === 'complete'
                          ? '扫描完成'
                          : analysis.scan_status === 'partial'
                            ? '部分目录未完整测量'
                            : '无法测量'}
                      </small>
                    </div>
                    <button
                      className="button text"
                      disabled={!canRun || analysis.path === '/'}
                      onClick={() =>
                        void execute(
                          'analyze',
                          analysis.path.substring(0, analysis.path.lastIndexOf('/')) || '/',
                        )
                      }
                    >
                      <ArrowLeft size={14} />
                      上一级
                    </button>
                  </div>
                  {analysis.scan_status !== 'complete' && (
                    <div className="inline-note">
                      <Info size={14} />带 “+” 的数值只包含已测量部分；无法读取不代表目录为空。
                    </div>
                  )}
                  <section className="card file-list">
                    <div className="list-toolbar">
                      <h2>目录内容</h2>
                      <label className="search-field">
                        <Search size={14} />
                        <input
                          aria-label="筛选目录内容"
                          placeholder="筛选名称…"
                          value={query}
                          onChange={(event) => setQuery(event.target.value)}
                        />
                      </label>
                    </div>
                    <div className="table-header">
                      <button onClick={() => setSort('name')}>名称</button>
                      <button onClick={() => setSort('size')}>
                        占用空间
                        <ArrowDown size={12} />
                      </button>
                      <span>操作</span>
                    </div>
                    <div className="scroll-list">
                      {entries.length ? (
                        entries.slice(0, 200).map((entry, index) => (
                          <div className="file-row" key={entry.path}>
                            <button
                              className="file-name"
                              disabled={!canRun || !entry.is_dir}
                              onClick={() => void execute('analyze', entry.path)}
                              title={entry.path}
                            >
                              <span className={`file-icon color-${index % 4}`}>
                                {entry.is_dir ? <Folder size={19} /> : <File size={18} />}
                              </span>
                              <span className="truncate">
                                {entry.name}
                                <small>
                                  {entry.scan_status === 'unavailable'
                                    ? '无法访问'
                                    : entry.is_dir
                                      ? '文件夹'
                                      : '文件'}
                                </small>
                              </span>
                            </button>
                            <div className="file-size">
                              <strong>{measuredBytes(entry.size, entry.scan_status)}</strong>
                              <div className="meter">
                                <div
                                  className={`color-${index % 4}`}
                                  style={{
                                    width: `${analysis.total_size ? Math.min(100, (entry.size / analysis.total_size) * 100) : 0}%`,
                                  }}
                                />
                              </div>
                            </div>
                            <button
                              className="icon-button"
                              disabled={!api.desktop}
                              onClick={() => reveal(entry.path)}
                              aria-label={`在 Finder 中显示 ${entry.name}`}
                            >
                              <ExternalLink size={15} />
                            </button>
                          </div>
                        ))
                      ) : (
                        <Empty title={query ? '没有匹配的项目' : '没有可展示的子项'}>
                          <p>
                            {query
                              ? '试试其他关键词。'
                              : analysis.scan_status === 'complete'
                                ? '此范围内没有扫描到子项。'
                                : '部分内容无法读取，请检查目录权限。'}
                          </p>
                        </Empty>
                      )}
                    </div>
                    {entries.length > 200 && (
                      <div className="list-limit">
                        显示前 200 项，共 {entries.length} 项。请按名称筛选或缩小目录范围。
                      </div>
                    )}
                  </section>
                </>
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
              <div className="page-heading">
                <div>
                  <div className="eyebrow">MOLE CLEAN</div>
                  <h1>清理</h1>
                  <p>查看 Mole CLI 的清理预览，了解每一项处理的内容。</p>
                </div>
                <button
                  className="button primary"
                  disabled={!canRun}
                  onClick={() => void execute('clean')}
                >
                  <Search size={15} />
                  {cleanPreview ? '重新扫描' : '扫描清理预览'}
                </button>
              </div>
              <div className="notice">
                <ShieldCheck size={18} />
                <span>
                  通过 Mole CLI 的 dry-run
                  模式扫描。当前只提供扫描与审阅，不会删除文件；需要管理员权限的项目会跳过。
                </span>
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
                  <Empty icon={Sparkles} title="让命令行能力，更容易使用">
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
              <div className="page-heading">
                <div>
                  <div className="eyebrow">APPLICATION LIBRARY</div>
                  <h1>应用列表</h1>
                  <p>查看这台 Mac 上的应用、安装来源和已知占用。</p>
                </div>
                <button
                  className="button primary"
                  disabled={!canRun}
                  onClick={() => void execute('apps')}
                >
                  <RefreshCw size={15} />
                  {apps ? '刷新列表' : '读取应用'}
                </button>
              </div>
              <div className="inline-note">
                <Info size={14} />
                当前仅展示应用信息，未提供卸载。首次读取可能需要较长时间。
              </div>
              {apps ? (
                <section className="card">
                  <div className="list-toolbar">
                    <h2>{apps.length} 个应用</h2>
                    <label className="search-field">
                      <Search size={14} />
                      <input
                        aria-label="搜索应用"
                        placeholder="搜索应用或 Bundle ID…"
                        value={query}
                        onChange={(event) => setQuery(event.target.value)}
                      />
                    </label>
                  </div>
                  <div className="scroll-list">
                    {filteredApps.length ? (
                      filteredApps.map((app) => (
                        <div className="app-row" key={app.path}>
                          <span className="app-icon">
                            <Box size={20} strokeWidth={1.4} />
                          </span>
                          <div className="app-info">
                            <strong>{app.name}</strong>
                            <small className="mono truncate">{app.bundle_id || app.path}</small>
                          </div>
                          <span className="tag">{app.source}</span>
                          <span className="app-size">{app.size || '未知'}</span>
                          <button
                            className="icon-button"
                            onClick={() => reveal(app.path)}
                            aria-label={`显示 ${app.name}`}
                          >
                            <ExternalLink size={15} />
                          </button>
                        </div>
                      ))
                    ) : (
                      <Empty icon={Box} title="未找到应用">
                        <p>{query ? '试试其他关键词。' : '没有返回可展示的应用。'}</p>
                      </Empty>
                    )}
                  </div>
                </section>
              ) : (
                <section className="card">
                  <Empty icon={Box} title="你的应用，一目了然">
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
              <div className="page-heading">
                <div>
                  <div className="eyebrow">SYSTEM SNAPSHOT</div>
                  <h1>系统状态</h1>
                  <p>记录此刻的状态，帮助你了解 Mac 正在忙什么。</p>
                </div>
                <button
                  className="button primary"
                  disabled={!canRun}
                  onClick={() => void execute('status')}
                >
                  <RefreshCw size={15} />
                  刷新快照
                </button>
              </div>
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
              <div className="page-heading">
                <div>
                  <div className="eyebrow">ACTIVITY LOG</div>
                  <h1>操作历史</h1>
                  <p>每一次检查都有记录，任务中断也会如实保留。</p>
                </div>
                <span className="tag">本地保留 {data.settings.retentionDays} 天</span>
              </div>
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
              <section className="card">
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

          <footer className="content-footer">
            <span>MooMole · 学习研究项目</span>
            <button onClick={official}>
              支持 Mole 官方产品
              <ArrowUpRight size={12} />
            </button>
          </footer>
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
          onSave={updateSettings}
          onClose={() => setSettingsTab(null)}
        />
      )}
      {!data.settings.acknowledged && (
        <Modal title="欢迎使用 MooMole" className="welcome-modal">
          <div className="welcome-symbol">
            <ShieldCheck size={30} strokeWidth={1.4} />
          </div>
          <div className="eyebrow">BEFORE WE BEGIN</div>
          <h2>先了解，再开始。</h2>
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
