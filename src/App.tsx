import { useEffect, useRef, useState, type CSSProperties } from 'react'
import {
  Activity,
  Archive,
  ArrowRight,
  ArrowUpRight,
  Box,
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
  Play,
  Pause,
  Wrench,
  X,
} from 'lucide-react'
import * as api from './lib/api'
import { useStatusWatch } from './lib/use-status-watch'
import { statusIntervals, type StatusInterval } from './lib/status-watch'
import { statusPercent } from './lib/status-metrics'
import { diskBytes, systemDisk } from './lib/status-disks'
import { preserveExecutionProgress, watchExecution } from './lib/execution-watcher'
import type { UninstallTarget } from './lib/app-uninstall'
import { demoAnalysis, demoCleanup, demoCleanPreview, demoSnapshot } from './lib/demo'
import { bytes, dateTime, kindNames, measuredBytes, percent, shortPath } from './lib/format'
import { deferredView } from './components/DeferredView'
import { Modal } from './components/Modal'
import { focusReturnTarget, restoreFocus } from './lib/focus-return'
import { EngineSummary } from './components/EngineSummary'
import { ExecutionRecoveryNotice } from './components/ExecutionRecoveryNotice'
import { ProcessSnapshot } from './components/ProcessSnapshot'
import { useMaintenance } from './lib/use-maintenance'
import { useProjectArtifacts } from './lib/use-project-artifacts'
import { useInstallers } from './lib/use-installers'
import { AnalysisNavigation } from './components/AnalysisNavigation'
import { recentAnalysisPaths } from './lib/recent-analysis'
import { AnalysisTaskNotice } from './components/AnalysisTaskNotice'
import { analysisTaskActive, failedAnalysisTask, type AnalysisTask } from './lib/analysis-task'
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

const SettingsModal = deferredView(
  async () => ({ default: (await import('./components/SettingsModal')).SettingsModal }),
  { onClose: (props) => props.onClose() },
)
const CleanReview = deferredView(async () => ({
  default: (await import('./components/CleanReview')).CleanReview,
}))
const ReviewPlanDialog = deferredView(
  async () => ({ default: (await import('./components/ReviewPlanDialog')).ReviewPlanDialog }),
  { onClose: (props) => props.onClose() },
)
const ExecutionHistory = deferredView(async () => ({
  default: (await import('./components/ExecutionHistory')).ExecutionHistory,
}))
const MoleHistoryView = deferredView(async () => ({
  default: (await import('./components/MoleHistoryView')).MoleHistoryView,
}))
const ApplicationLibrary = deferredView(async () => ({
  default: (await import('./components/ApplicationLibrary')).ApplicationLibrary,
}))
const ApplicationUninstallCenter = deferredView(async () => ({
  default: (await import('./components/ApplicationUninstallCenter')).ApplicationUninstallCenter,
}))
const MaintenanceCatalog = deferredView(async () => ({
  default: (await import('./components/MaintenanceCatalog')).MaintenanceCatalog,
}))
const QuickLookRefreshCenter = deferredView(async () => ({
  default: (await import('./components/QuickLookRefreshCenter')).QuickLookRefreshCenter,
}))
const Installers = deferredView(async () => ({
  default: (await import('./components/Installers')).Installers,
}))
const ProjectArtifacts = deferredView(async () => ({
  default: (await import('./components/ProjectArtifacts')).ProjectArtifacts,
}))
const MaintenanceReviewDialog = deferredView(
  async () => ({
    default: (await import('./components/MaintenanceReviewDialog')).MaintenanceReviewDialog,
  }),
  { onClose: (props) => props.onClose() },
)
const TaskHistory = deferredView(async () => ({
  default: (await import('./components/TaskHistory')).TaskHistory,
}))
const AnalysisExplorer = deferredView(async () => ({
  default: (await import('./components/AnalysisExplorer')).AnalysisExplorer,
}))
const StatusTrend = deferredView(async () => ({
  default: (await import('./components/StatusTrend')).StatusTrend,
}))
const StatusRates = deferredView(async () => ({
  default: (await import('./components/StatusRates')).StatusRates,
}))
const StatusMemory = deferredView(async () => ({
  default: (await import('./components/StatusMemory')).StatusMemory,
}))
const StatusHardware = deferredView(async () => ({
  default: (await import('./components/StatusHardware')).StatusHardware,
}))
const StatusDisks = deferredView(async () => ({
  default: (await import('./components/StatusDisks')).StatusDisks,
}))

const navigation = [
  ['overview', LayoutGrid, '概览'],
  ['analyze', HardDrive, '空间分析'],
  ['cleanup', Sparkles, '清理'],
  ['apps', Box, '应用列表'],
  ['status', Activity, '系统状态'],
  ['maintenance', Wrench, '系统维护'],
  ['projects', Code2, '工程产物'],
  ['installers', Archive, '安装包'],
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
  children,
}: {
  label: string
  value: string
  detail: string
  icon: typeof Cpu
  usage?: number
  children?: React.ReactNode
}) {
  return (
    <div className="metric-card">
      <div className="metric-label">
        <Icon size={16} />
        {label}
      </div>
      <strong>{value}</strong>
      <span>{detail}</span>
      {usage != null && !children && (
        <div className="meter">
          <div style={{ width: `${Math.max(0, Math.min(usage, 100))}%` }} />
        </div>
      )}
      {children}
    </div>
  )
}

export default function App() {
  const [data, setData] = useState<Bootstrap | null>(null)
  const [page, setPage] = useState<Page>('overview')
  const [maintenanceQuery, setMaintenanceQuery] = useState('')
  const maintenance = useMaintenance(page === 'maintenance')
  const projects = useProjectArtifacts(page === 'projects', data?.home ?? '')
  const installers = useInstallers(page === 'installers', data?.home ?? '')
  const [toolbarScrolled, setToolbarScrolled] = useState(() => window.scrollY > 0)
  useEffect(() => {
    const update = () => setToolbarScrolled(window.scrollY > 0)
    update()
    window.addEventListener('scroll', update, { passive: true })
    return () => window.removeEventListener('scroll', update)
  }, [])
  const [snapshot, setSnapshot] = useState<Snapshot | null>(api.desktop ? null : demoSnapshot)
  const statusWatch = useStatusWatch(page === 'status', setSnapshot)
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
  const [appsTaskId, setAppsTaskId] = useState<string | null>(null)
  const [uninstallTarget, setUninstallTarget] = useState<UninstallTarget | null>(null)
  const [uninstallActive, setUninstallActive] = useState(false)
  const [refreshActive, setRefreshActive] = useState(false)
  const [refreshOpenSignal, setRefreshOpenSignal] = useState(0)
  const [appsStale, setAppsStale] = useState(false)
  const [moleHistory, setMoleHistory] = useState<MoleHistory | null>(null)
  const [busy, setBusy] = useState<{ id: string; kind: TaskKind } | null>(null)
  const [analysisTask, setAnalysisTask] = useState<AnalysisTask | null>(null)
  const cancelRequest = useRef(0)
  const busyRef = useRef(false)
  const autoStatus = useRef(false)
  const observation = useRef(new AbortController())
  const historyRequest = useRef(0)
  const [settingsTab, setSettingsTab] = useState<'general' | 'protected' | 'about' | null>(null)
  const settingsButton = useRef<HTMLButtonElement>(null)
  const settingsOpener = useRef<HTMLElement | null>(null)
  const settingsOpen = useRef(false)
  const [error, setError] = useState('')
  const [accepted, setAccepted] = useState(false)
  const [saving, setSaving] = useState(false)
  const [path, setPath] = useState('')
  const analysisPathInput = useRef<HTMLInputElement>(null)
  const recentPaths = recentAnalysisPaths(data?.history ?? [])
  const [cancelPending, setCancelPending] = useState(false)
  const [plan, setPlan] = useState<ReviewPlan | null>(null)
  const [planLoading, setPlanLoading] = useState(false)
  const [recoveryNavigation, setRecoveryNavigation] = useState(0)
  const [executionPending, setExecutionPending] = useState(false)
  const [executionTrackingError, setExecutionTrackingError] = useState('')
  const executionActive =
    api.desktop && (executionPending || !!data?.executions.some((item) => item.state === 'running'))
  function openSettings(tab: 'general' | 'protected' | 'about', opener?: HTMLElement) {
    if (!settingsOpen.current) {
      settingsOpener.current =
        opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null)
      settingsOpen.current = true
    }
    setSettingsTab(tab)
  }
  function closeSettings() {
    settingsOpen.current = false
    setSettingsTab(null)
    // Also covers closing a pending or failed lazy view before Modal mounts.
    requestAnimationFrame(() => {
      if (document.activeElement === document.body)
        restoreFocus(settingsOpener.current, settingsButton.current)
    })
  }
  useEffect(() => {
    if (!executionActive) return
    const controller = new AbortController()
    const stop = watchExecution({
      load: api.executions,
      onValue: (executions) =>
        setData((current) =>
          current
            ? { ...current, executions: preserveExecutionProgress(executions, current.executions) }
            : current,
        ),
      onError: setExecutionTrackingError,
      signal: controller.signal,
    })
    const hide = () => controller.abort()
    window.addEventListener('pagehide', hide)
    return () => {
      controller.abort()
      stop()
      window.removeEventListener('pagehide', hide)
    }
  }, [executionActive])
  const handledRecoveryNavigation = useRef(0)

  function focusExecutionRecords(section: HTMLElement | null) {
    if (!section || page !== 'history' || recoveryNavigation === handledRecoveryNavigation.current)
      return
    handledRecoveryNavigation.current = recoveryNavigation
    section.focus({ preventScroll: true })
    section.scrollIntoView({ block: 'start' })
  }

  async function load() {
    try {
      const initial = await api.bootstrap()
      if (observation.current.signal.aborted) return
      const oldStreams = initial.history.filter(
        (task) => task.kind === 'statuswatch' && task.state === 'running',
      )
      let streamWarning = ''
      if (oldStreams.length) {
        autoStatus.current = true
        const stops = await Promise.allSettled(
          oldStreams.map((task) => api.statusTransport.stop(task.id)),
        )
        if (stops.some((result) => result.status === 'rejected'))
          streamWarning = '旧采集仍在停止，请稍后在系统状态中重新开始。'
        initial.history = await api.history()
        if (observation.current.signal.aborted) return
      }
      const active = initial.history.filter(
        (task) => task.state === 'running' && Object.hasOwn(kindNames, task.kind),
      )
      if (api.desktop && active.length) autoStatus.current = true
      setData(initial)
      if (api.desktop && initial.latestCleanPreview) setCleanPreview(initial.latestCleanPreview)
      setPath(initial.home)
      setError(streamWarning)
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
        openSettings('general')
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
    ++cancelRequest.current
    setError('')
    setCancelPending(false)
    let trackedAnalysis: AnalysisTask = {
      id: null,
      path: target ?? null,
      historyTarget: analysisTarget,
      state: 'starting',
    }
    if (kind === 'analyze') setAnalysisTask(trackedAnalysis)
    let resultTaskId: string | null = null
    try {
      const result = await api.runTask(
        kind,
        target,
        (id) => {
          resultTaskId = id
          setBusy({ id, kind })
          if (kind === 'analyze') {
            trackedAnalysis = { ...trackedAnalysis, id, state: 'running' }
            setAnalysisTask(trackedAnalysis)
          }
          void refreshRecords().catch((error) => setError(`历史记录无法刷新：${String(error)}`))
        },
        observation.current.signal,
      )
      applyTaskResult(kind, result, analysisTarget, resultTaskId)
    } catch (error) {
      if (!observation.current.signal.aborted) {
        if (kind === 'analyze') setAnalysisTask(failedAnalysisTask(trackedAnalysis, error))
        else setError(String(error).replace(/^(?:TaskResultError|Error): /, ''))
      }
    } finally {
      if (observation.current.signal.aborted) return
      setBusy(null)
      ++cancelRequest.current
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
    taskId?: string | null,
  ) {
    if (kind === 'status') {
      setSnapshot(result as Snapshot)
      statusWatch.clearHistory()
    }
    if (kind === 'analyze') {
      setAnalysisTask(null)
      const value = result as Analysis
      rememberAnalysisContext()
      setAnalysis({ ...value })
      setPath(value.path)
      setAnalysisNavigation((current) => recordAnalysisVisit(current, value.path, analysisTarget))
    }
    if (kind === 'apps') {
      setAppsStale(false)
      setApps([...(result as AppEntry[])])
      setAppsTaskId(taskId ?? null)
    }
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

  async function followTasks(records: TaskRecord[]) {
    const tasks = records.filter((task): task is TaskRecord & { kind: TaskKind } =>
      Object.hasOwn(kindNames, task.kind),
    )
    if (busyRef.current || !tasks.length) return
    busyRef.current = true
    setCancelPending(false)
    setError('')
    ++cancelRequest.current
    const analysisRecord = tasks.find((task) => task.kind === 'analyze')
    if (analysisRecord)
      setAnalysisTask({ id: analysisRecord.id, path: analysisRecord.path, state: 'running' })
    const pending = new Map(tasks.map((task) => [task.id, task]))
    setBusy({ id: tasks[0].id, kind: tasks[0].kind })
    await Promise.all(
      tasks.map(async (task) => {
        try {
          const result = await api.resumeTask(task.id, task.kind, observation.current.signal)
          applyTaskResult(task.kind, result, undefined, task.id)
        } catch (error) {
          if (!observation.current.signal.aborted) {
            if (task.kind === 'analyze')
              setAnalysisTask(
                failedAnalysisTask({ id: task.id, path: task.path, state: 'running' }, error),
              )
            else setError(String(error).replace(/^(?:TaskResultError|Error): /, ''))
          }
        } finally {
          pending.delete(task.id)
          if (!observation.current.signal.aborted) {
            const next = pending.values().next().value
            setBusy(next ? { id: next.id, kind: next.kind } : null)
            setCancelPending(false)
            ++cancelRequest.current
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

  function cancelCurrentTask() {
    if (!busy || cancelPending) return
    const request = ++cancelRequest.current
    const id = busy.id
    setCancelPending(true)
    if (busy.kind === 'analyze')
      setAnalysisTask((current) =>
        current?.id === id ? { ...current, state: 'cancelling', cancelError: undefined } : current,
      )
    void api.cancelTask(id).catch((error) => {
      if (request !== cancelRequest.current || observation.current.signal.aborted) return
      setCancelPending(false)
      if (busy.kind === 'analyze')
        setAnalysisTask((current) =>
          current?.id === id && current.state === 'cancelling'
            ? { ...current, state: 'running', cancelError: String(error) }
            : current,
        )
      else setError(String(error))
    })
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
    try {
      await refreshRecords(true)
    } catch (error) {
      // The preference transaction has already committed. A failed readback
      // must not be reported as a failed save or invite a duplicate save.
      setError(`设置已保存，但历史记录无法刷新：${String(error)}`)
    }
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

  const canRun =
    api.desktop &&
    data.engine.available &&
    !busy &&
    !uninstallActive &&
    !refreshActive &&
    analysisTask?.state !== 'starting'
  const disk = systemDisk(snapshot?.disks)
  const diskPercent = disk?.percent ?? null
  const pageName = navigation.find(([id]) => id === page)![2]
  const lastSnapshot = snapshot ? dateTime(snapshot.collected_at) : null
  const cpuUsage = statusPercent(snapshot?.cpu?.usage) ?? undefined
  const memoryUsage = statusPercent(snapshot?.memory?.used_percent) ?? undefined
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
          <button
            ref={settingsButton}
            className="sidebar-settings"
            onClick={(event) => openSettings('general', event.currentTarget)}
          >
            <Settings2 size={17} />
            设置
          </button>
          <button className="sidebar-official" onClick={official}>
            请使用Mole官方产品
            <ArrowUpRight size={12} />
          </button>
        </div>
      </aside>
      <div className="workspace">
        <header
          className="topbar"
          data-scrolled={toolbarScrolled}
          data-tauri-drag-region
          aria-label={`${pageName}工具栏`}
        >
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
                    ref={analysisPathInput}
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
                {data && recentPaths.length > 0 && (
                  <select
                    className="analysis-recent"
                    aria-label="最近分析目录"
                    title="选入路径后点击开始分析，重新读取当前目录；历史记录不代表目录仍可访问。"
                    value=""
                    disabled={!canRun}
                    onChange={(event) => {
                      const selected = event.target.value
                      if (!canRun || !recentPaths.includes(selected)) return
                      setPath(selected)
                      analysisPathInput.current?.focus({ preventScroll: true })
                    }}
                  >
                    <option value="" disabled>
                      最近目录
                    </option>
                    {recentPaths.map((recent) => (
                      <option key={recent} value={recent}>
                        {shortPath(recent, data.home)}
                      </option>
                    ))}
                  </select>
                )}
                <button
                  className="icon-button"
                  disabled={!canRun}
                  onClick={() => void chooseAndAnalyze()}
                  aria-label="选择文件夹"
                  title="选择文件夹"
                >
                  <FolderOpen size={17} />
                </button>
                {busy?.kind === 'analyze' && (
                  <button
                    className="button secondary compact"
                    disabled={cancelPending}
                    onClick={cancelCurrentTask}
                  >
                    <Square size={11} />
                    {cancelPending ? '正在取消…' : '取消'}
                  </button>
                )}
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
                    disabled={!canRun || statusWatch.phase !== 'idle'}
                    onClick={() => void execute('status')}
                  >
                    <RefreshCw size={15} className={busy?.kind === 'status' ? 'spin' : ''} />
                    {page === 'overview' ? '刷新状态' : '刷新快照'}
                  </button>
                )}
                {page === 'status' && (
                  <>
                    <label
                      className="status-interval"
                      title={
                        statusWatch.phase === 'idle'
                          ? '选择下一次采集的周期'
                          : '暂停采集后可调整周期'
                      }
                    >
                      <span>采样</span>
                      <select
                        aria-label="采样周期"
                        value={statusWatch.interval}
                        disabled={!api.desktop || statusWatch.phase !== 'idle'}
                        onChange={(event) =>
                          statusWatch.chooseInterval(Number(event.target.value) as StatusInterval)
                        }
                      >
                        {statusIntervals.map((seconds) => (
                          <option key={seconds} value={seconds}>
                            {seconds} 秒{seconds === 2 ? '（默认）' : ''}
                          </option>
                        ))}
                      </select>
                    </label>
                    <button
                      className="button secondary compact"
                      disabled={
                        statusWatch.phase === 'stopping' ||
                        (statusWatch.phase === 'idle' && !canRun)
                      }
                      onClick={() =>
                        statusWatch.phase === 'idle'
                          ? void statusWatch.start()
                          : statusWatch.pause()
                      }
                    >
                      {statusWatch.phase === 'idle' ? <Play size={15} /> : <Pause size={15} />}
                      {statusWatch.phase === 'idle'
                        ? '持续采集'
                        : statusWatch.phase === 'stopping'
                          ? '正在暂停'
                          : '暂停采集'}
                    </button>
                  </>
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
                {page === 'installers' && (
                  <>
                    <select
                      className="installer-location"
                      aria-label="安装包查找位置"
                      value={installers.locationId}
                      disabled={
                        !canRun ||
                        installers.reading ||
                        installers.choosing ||
                        !!installers.checking ||
                        installers.locations.length === 0
                      }
                      onChange={(event) => installers.selectLocation(event.target.value)}
                    >
                      <option value="" disabled>
                        常用位置…
                      </option>
                      <optgroup label="常用位置">
                        {installers.locations
                          .filter((l) => l.group === 'common')
                          .map((l) => (
                            <option key={l.id} value={l.id}>
                              {l.label}
                            </option>
                          ))}
                      </optgroup>
                      <optgroup label="其他 Mole 位置">
                        {installers.locations
                          .filter((l) => l.group === 'other')
                          .map((l) => (
                            <option key={l.id} value={l.id}>
                              {l.label}
                            </option>
                          ))}
                      </optgroup>
                    </select>

                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        installers.reading ||
                        installers.choosing ||
                        !!installers.checking
                      }
                      onClick={() => void installers.choose()}
                    >
                      <FolderOpen size={15} />
                      {installers.choosing ? '正在选择…' : '选择目录'}
                    </button>
                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        installers.reading ||
                        installers.choosing ||
                        !!installers.checking ||
                        !installers.root
                      }
                      onClick={() => void installers.scan()}
                    >
                      <Search size={15} />
                      {installers.reading ? '正在扫描…' : '扫描目录'}
                    </button>
                    {installers.root && (
                      <span className="toolbar-note project-root" title={installers.root}>
                        {installers.root}
                      </span>
                    )}
                  </>
                )}
                {page === 'projects' && (
                  <>
                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        projects.reading ||
                        projects.choosing ||
                        !!projects.measuring ||
                        !!projects.checkingGit ||
                        projects.loadingRoots
                      }
                      onClick={() => void projects.loadRoots()}
                      title="只读取 Mole 已保存的项目目录，不扫描或修改配置"
                    >
                      {projects.loadingRoots ? '正在读取…' : '读取 Mole 目录'}
                    </button>
                    {projects.roots && projects.roots.paths.length > 0 && (
                      <select
                        className="project-root-select"
                        aria-label="Mole 已保存的项目目录"
                        value=""
                        disabled={
                          !canRun ||
                          projects.reading ||
                          projects.choosing ||
                          !!projects.measuring ||
                          !!projects.checkingGit ||
                          projects.loadingRoots
                        }
                        onChange={(event) => projects.selectRoot(event.target.value)}
                      >
                        <option value="" disabled>
                          选择 Mole 目录…
                        </option>
                        {projects.roots.paths.map((path) => (
                          <option value={path} key={path}>
                            {path}
                          </option>
                        ))}
                      </select>
                    )}
                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        projects.reading ||
                        projects.choosing ||
                        !!projects.measuring ||
                        !!projects.checkingGit ||
                        projects.loadingRoots
                      }
                      onClick={() => void projects.choose()}
                    >
                      <FolderOpen size={15} />
                      {projects.choosing ? '正在选择…' : '选择目录'}
                    </button>
                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        projects.loadingRoots ||
                        projects.reading ||
                        projects.choosing ||
                        !!projects.measuring ||
                        !!projects.checkingGit ||
                        !projects.root
                      }
                      onClick={() => void projects.scan()}
                    >
                      <Search size={15} />
                      {projects.reading ? '正在扫描…' : '扫描目录'}
                    </button>
                    {projects.root && (
                      <span className="toolbar-note project-root" title={projects.root}>
                        {projects.root}
                      </span>
                    )}
                  </>
                )}
                {page === 'maintenance' && (
                  <>
                    <button
                      className="button primary compact"
                      disabled={!canRun}
                      onClick={() => setRefreshOpenSignal((n) => n + 1)}
                    >
                      刷新预览…
                    </button>
                    <label className="search-field maintenance-search">
                      <Search size={15} />
                      <input
                        aria-label="搜索维护项目"
                        placeholder="搜索用途、影响或权限"
                        value={maintenanceQuery}
                        onChange={(event) => setMaintenanceQuery(event.target.value)}
                      />
                    </label>
                    <button
                      className="button secondary compact"
                      disabled={!canRun || maintenance.reading || maintenance.reviewing}
                      onClick={() => void maintenance.inspect()}
                    >
                      <Search size={15} />
                      {maintenance.reading ? '正在检查…' : '检查本机设置'}
                    </button>
                    <button
                      className="button secondary compact"
                      disabled={
                        !canRun ||
                        maintenance.reading ||
                        maintenance.reviewing ||
                        maintenance.selected.length === 0
                      }
                      onClick={() => void maintenance.prepareReview()}
                    >
                      {maintenance.reviewing
                        ? '正在审阅…'
                        : `审阅所选${maintenance.selected.length > 0 ? ` ${maintenance.selected.length} 项` : ''}`}
                    </button>
                  </>
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
            {page === 'cleanup' || page === 'apps' || page === 'maintenance'
              ? '逐项确认'
              : '只读检查'}
          </span>
        </header>
        <main className="main-content" tabIndex={-1} aria-label={`${pageName}内容`}>
          <QuickLookRefreshCenter
            openSignal={refreshOpenSignal}
            enabled={api.desktop}
            historyVisible={page === 'history'}
            transport={api.refreshTransport}
            onActiveChange={setRefreshActive}
            initialRecords={data.maintenanceRefreshes}
          />
          <ApplicationUninstallCenter
            target={uninstallTarget}
            enabled={api.desktop}
            historyVisible={page === 'history'}
            transport={api.uninstallTransport}
            onTargetClose={() => setUninstallTarget(null)}
            onActiveChange={setUninstallActive}
            onOutcome={() => setAppsStale(true)}
          />
          {(executionActive || executionTrackingError) && (
            <section className="notice execution-recovery" aria-label="日志处理状态">
              <div>
                <strong>{executionTrackingError ? '日志处理状态待确认' : '日志处理进行中'}</strong>
                <p>
                  {executionTrackingError ||
                    '逐项结果持续保存在历史中；关闭审阅框会请求停止后续项目，已发生的结果会保留。'}
                </p>
              </div>
              <button
                className="button secondary compact"
                onClick={() => {
                  changePage('history')
                  setRecoveryNavigation((value) => value + 1)
                }}
              >
                查看执行记录
              </button>
            </section>
          )}
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
          {analysisTask && (page === 'analyze' || !analysisTaskActive(analysisTask)) && (
            <AnalysisTaskNotice
              task={analysisTask}
              hasResult={!!analysis}
              canRetry={canRun}
              showResults={page !== 'analyze'}
              onRetry={() => {
                if (analysisTask.path) {
                  changePage('analyze')
                  void execute('analyze', analysisTask.path, analysisTask.historyTarget)
                }
              }}
              onHistory={() => changePage('history')}
              onResults={() => changePage('analyze')}
              onDismiss={() => setAnalysisTask(null)}
            />
          )}

          {page === 'overview' && (
            <>
              <EngineSummary engine={data.engine} desktop={api.desktop} />
              <section className="storage-card card" aria-label="系统卷空间">
                <div className="storage-main">
                  <div className="section-top">
                    <div>
                      <span className="section-label">磁盘空间</span>
                      <h2>{snapshot?.host ?? '这台 Mac'}</h2>
                    </div>
                    <span className="tag">
                      {disk ? '系统卷 /' : snapshot ? '系统卷未确认' : '等待快照'}
                    </span>
                  </div>
                  <div className="storage-detail">
                    <div
                      className="storage-ring"
                      data-unknown={diskPercent === null}
                      role="img"
                      aria-label={
                        diskPercent === null
                          ? '系统卷占用比例未知'
                          : `系统卷占用比例 ${diskPercent.toFixed(1)}%`
                      }
                      style={{ '--used': `${diskPercent ?? 0}%` } as CSSProperties}
                    >
                      <div>
                        <strong>
                          {diskPercent !== null
                            ? `${diskPercent.toFixed(0)}%`
                            : snapshot
                              ? '未知'
                              : '—'}
                        </strong>
                        <span>已使用</span>
                      </div>
                    </div>
                    <div className="storage-numbers">
                      <div className="disk-free">
                        <strong>{snapshot ? diskBytes(disk?.remaining) : '—'}</strong>
                        <span>报告余量</span>
                      </div>
                      <p>
                        共 {snapshot ? diskBytes(disk?.total) : '—'} ·{' '}
                        {snapshot?.hardware?.cpu_model ?? '读取后显示设备信息'}
                      </p>
                      <div className="disk-legend">
                        <span>
                          <i />
                          已使用 {snapshot ? diskBytes(disk?.used) : '—'}
                        </span>
                        <span>
                          <i />
                          余量（计算）
                        </span>
                      </div>
                    </div>
                  </div>
                  {snapshot && !disk && (
                    <p className="storage-scope muted">
                      未取得唯一的系统卷 / 报告；可在系统状态查看已返回的磁盘。
                    </p>
                  )}
                  {disk?.inconsistent && (
                    <p className="storage-scope muted">已用超过总量，本次余量与比例无法确认。</p>
                  )}
                  <p className="storage-scope muted">
                    按 Mole 报告总量减去已用计算，不是可清理量；容量可能来自较早缓存。
                  </p>
                </div>
              </section>
              <div className="metrics-grid">
                <Metric
                  label="CPU 使用率"
                  value={percent(cpuUsage)}
                  detail={
                    snapshot?.cpu?.logical_cpu
                      ? `${snapshot.cpu.logical_cpu} 个逻辑核心`
                      : '等待系统快照'
                  }
                  icon={Cpu}
                  usage={cpuUsage}
                />
                <Metric
                  label="内存占用"
                  value={bytes(snapshot?.memory?.used)}
                  detail={`总内存 ${bytes(snapshot?.memory?.total)}`}
                  icon={MemoryStick}
                  usage={memoryUsage}
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
              ) : analysisTaskActive(analysisTask) ? null : (
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
                <span>
                  扫描与审阅不会删除文件；符合条件的日志需单独确认才会移入废纸篓。需要管理员权限的项目会跳过。
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
                onClick={(event) => openSettings('protected', event.currentTarget)}
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
                列表来自 Mole CLI。卸载需单独检查范围并明确确认；勾选仅用于对比。
              </div>
              {appsStale && (
                <p className="notice warning" role="status">
                  卸载后清单可能已过期，请刷新应用清单后再检查；不会自动重复处理。
                </p>
              )}
              {apps ? (
                <ApplicationLibrary
                  apps={apps}
                  taskId={appsTaskId}
                  protectedPaths={data.settings.protectedPaths}
                  home={data.home}
                  refreshing={busy?.kind === 'apps'}
                  canReveal={api.desktop}
                  onReveal={reveal}
                  canUninstall={
                    canRun && !executionActive && !appsStale && data.settings.acknowledged
                  }
                  onUninstall={setUninstallTarget}
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
              <p className="muted" role={statusWatch.error ? 'alert' : 'status'}>
                {statusWatch.message}
              </p>
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
                      <p>
                        运行时间 {snapshot.uptime ?? '未知'} · 采集时间 {lastSnapshot}
                      </p>
                    </div>
                    <span className="tag">
                      {api.desktop
                        ? statusWatch.phase === 'running'
                          ? '持续采集'
                          : '本机快照'
                        : '示例快照'}
                    </span>
                  </div>
                  <div className="metrics-grid status-metrics-grid">
                    <Metric
                      label="CPU 使用率"
                      value={cpuUsage == null ? '未知' : percent(cpuUsage)}
                      detail={`${snapshot.cpu?.logical_cpu ?? '—'} 个逻辑核心${snapshot.cpu?.per_core_estimated === true ? ' · 当前为估算值' : snapshot.cpu?.per_core_estimated == null ? ' · 估算状态未报告' : ''}`}
                      icon={Cpu}
                      usage={cpuUsage}
                    >
                      <StatusTrend samples={statusWatch.samples} metric="cpu" />
                    </Metric>
                    <Metric
                      label="内存占用"
                      value={bytes(snapshot.memory?.used)}
                      detail={`总内存 ${bytes(snapshot.memory?.total)}`}
                      icon={MemoryStick}
                      usage={memoryUsage}
                    >
                      <StatusTrend samples={statusWatch.samples} metric="memory" />
                      <StatusMemory
                        memory={snapshot.memory}
                        stale={statusWatch.phase !== 'running' || statusWatch.stale}
                      />
                    </Metric>
                  </div>
                  {statusWatch.samples.length > 0 && (
                    <p className="muted status-trend-note">
                      本轮保留 {statusWatch.samples.length} 份已接收记录 · 最多 5 分钟 / 120 份 ·
                      缺失处断开显示
                    </p>
                  )}
                  <StatusRates
                    snapshot={snapshot}
                    samples={statusWatch.samples}
                    stale={statusWatch.phase !== 'running' || statusWatch.stale}
                  />
                  <ProcessSnapshot snapshot={snapshot} refreshing={busy?.kind === 'status'} />
                  <StatusDisks
                    snapshot={snapshot}
                    stale={statusWatch.phase !== 'running' || statusWatch.stale}
                  />
                  <StatusHardware
                    snapshot={snapshot}
                    stale={statusWatch.phase !== 'running' || statusWatch.stale}
                  />
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

          {page === 'maintenance' && (
            <MaintenanceCatalog
              query={maintenanceQuery}
              engine={data.engine}
              report={maintenance.report}
              reading={maintenance.reading}
              error={maintenance.error}
              selected={maintenance.selected}
              onSelect={maintenance.select}
              reviewing={maintenance.reviewing}
            />
          )}

          {page === 'installers' && (
            <Installers
              root={installers.root}
              report={installers.report}
              reading={installers.reading}
              error={installers.error}
              onReveal={reveal}
              checking={installers.checking}
              checkKind={installers.checkKind}
              usageObservations={installers.usageObservations}
              onCheckUsage={installers.checkUsage}
              zipObservations={installers.zipObservations}
              imageObservations={installers.imageObservations}
              onCheckImage={installers.checkImage}
              zipDataObservations={installers.zipDataObservations}
              onCheckZipData={installers.checkZipData}
              onCheckZip={installers.checkZip}
              connectionObservations={installers.connectionObservations}
              onCheckConnection={installers.checkConnection}
            />
          )}

          {page === 'projects' && (
            <ProjectArtifacts
              filters={projects.filters}
              onFiltersChange={projects.setFilters}
              roots={projects.roots}
              rootsError={projects.rootsError}
              loadingRoots={projects.loadingRoots}
              root={projects.root}
              report={projects.report}
              reading={projects.reading}
              error={projects.error}
              canAnalyze={canRun && !projects.loadingRoots && !projects.choosing}
              checkingGit={projects.checkingGit}
              onCheckGit={projects.checkGit}
              measuring={projects.measuring}
              measurements={projects.measurements}
              onMeasure={projects.measure}
              onAnalyze={(target) => {
                changePage('analyze')
                setPath(target)
                void execute('analyze', target)
              }}
            />
          )}
          {page === 'maintenance' && maintenance.reviewError && (
            <p className="notice" role="alert">
              {maintenance.reviewError}
            </p>
          )}
          {page === 'maintenance' && maintenance.review && (
            <MaintenanceReviewDialog
              review={maintenance.review}
              onClose={maintenance.closeReview}
              onRecheck={() => {
                maintenance.closeReview()
                void maintenance.inspect()
              }}
            />
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
                onReady={focusExecutionRecords}
                initialRecords={data.executions}
                loadRecords={api.executions}
                loadDetail={api.execution}
                onCancel={api.cancelLogCleanup}
                onAcknowledge={api.acknowledgeExecution}
                onRevealReceipt={api.revealExecutionItem}
                onRecordsChange={(executions) => {
                  setExecutionTrackingError('')
                  setData((current) =>
                    current
                      ? {
                          ...current,
                          executions: preserveExecutionProgress(executions, current.executions),
                        }
                      : current,
                  )
                }}
              />
              <TaskHistory
                records={data.history}
                home={data.home}
                canRefresh={api.desktop}
                canTrack={api.desktop && !busy}
                onRefresh={() => refreshRecords()}
                onTrack={(task) => void followTasks([task])}
              />
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
          onDiscardConfirmation={api.discardCleanupConfirmation}
          onRevealReceipt={api.revealExecutionItem}
          onExecute={async (id, signal) => {
            setExecutionPending(true)
            try {
              return await api.executeLogCleanup(id, signal)
            } finally {
              try {
                const executions = await api.executions()
                setExecutionTrackingError('')
                setData((current) =>
                  current
                    ? {
                        ...current,
                        executions: preserveExecutionProgress(executions, current.executions),
                      }
                    : current,
                )
              } catch {
                setExecutionTrackingError('执行状态读取失败，请在历史中刷新核对实际结果。')
              }
              setExecutionPending(false)
            }
          }}
          onClose={() => setPlan(null)}
        />
      )}
      {busy && !(busy.kind === 'analyze' && page === 'analyze') && (
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
            onClick={cancelCurrentTask}
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
          onClose={closeSettings}
          returnFocus={() => focusReturnTarget(settingsOpener.current, settingsButton.current)}
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
            仅符合条件的日志可单独确认移入废纸篓，其他范围保持只读。暂无应用内恢复。
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
