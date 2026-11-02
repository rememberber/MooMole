import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Activity,
  BrainCircuit,
  Archive,
  ArrowRight,
  ArrowUpRight,
  Box,
  ChevronRight,
  Code2,
  Cpu,
  FolderOpen,
  Copy,
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
  Zap,
  Square,
  Play,
  Pause,
  Wrench,
  X,
} from 'lucide-react'
import * as api from './lib/api'
import { useLanguage, setLanguagePreference, setSystemLanguage } from './i18n/locale'
import { useAiScan } from './lib/use-ai-scan'
import { duplicateTransport, similarTransport } from './lib/personal-files'
import {
  softwareContentRoot,
  softwareContentSources,
  softwareContentTransport,
  type SoftwareContentSource,
} from './lib/software-content'
import { usePersonalFiles } from './lib/use-personal-files'
import type { BrowserProfile } from './lib/browser-downloads'
import { ManagedAppCenter } from './components/ManagedAppCenter'
import { LivePhotoPairs } from './components/LivePhotoPairs'
import { BrowserDownloadsPicker } from './components/BrowserDownloadsPicker'
import { useStatusWatch } from './lib/use-status-watch'
import { statusIntervals, type StatusInterval } from './lib/status-watch'
import { statusPercent } from './lib/status-metrics'
import { StorageOverview } from './components/StorageOverview'
import { preserveExecutionProgress, watchExecution } from './lib/execution-watcher'
import { ApplicationControlCenter } from './components/ApplicationControlCenter'
import { ApplicationUpdateCenter } from './components/ApplicationUpdateCenter'
import { AppUpdateDiscoveryCenter } from './components/AppUpdateDiscoveryCenter'
import { DeviceBackups } from './components/DeviceBackups'
import { SpaceExplanation } from './components/SpaceExplanation'
import type { AppControlTarget } from './lib/app-control'
import { SearchRepairCenter } from './components/SearchRepairCenter'
import { ProblemRepairs } from './components/ProblemRepairs'
import { searchRepairTransport } from './lib/search-repair'
import { FileRestorationCenter } from './components/FileRestorationCenter'
import { FileArchiveCenter, FileArchiveHistory } from './components/FileArchiveCenter'
import { FolderArchiveCenter, FolderArchiveHistory } from './components/FolderArchiveCenter'
import { ZipExtractionCenter, ZipExtractionHistory } from './components/ZipExtractionCenter'
import { StartupManagement } from './components/StartupManagement'
import { FileTransferCenter, FileTransferHistory } from './components/FileTransferCenter'
import {
  TransferRestorationCenter,
  TransferRestorationHistory,
} from './components/TransferRestorationCenter'
import type { TransferRestoreTarget } from './lib/transfer-restoration'
import type { TransferTarget } from './lib/file-transfer'
import type { FileArchiveTarget } from './lib/file-archive'
import { restorationTransport, type RestorationTarget } from './lib/file-restoration'
import { FileRestorationBatchCenter } from './components/FileRestorationBatchCenter'
import { MountedVolumePicker } from './components/MountedVolumePicker'
import type { RestorationBatchTarget } from './lib/file-restoration-batch'
import { storageMonitorTransport } from './lib/storage-monitor'
import { menuBarTransport, listenMenuBar } from './lib/menu-bar'
import { MenuBarSettings } from './components/MenuBarSettings'
import { FolderSchedules } from './components/FolderSchedules'
import { folderScheduleTransport } from './lib/folder-schedules'
import { useFolderSchedules } from './lib/use-folder-schedules'
import { useScheduledFiles } from './lib/use-scheduled-files'
import { scheduledFilesTransport } from './lib/scheduled-files'
import { ScheduledFiles } from './components/ScheduledFiles'
import type { UninstallTarget } from './lib/app-uninstall'
import { demoAnalysis, demoCleanup, demoCleanPreview, demoSnapshot } from './lib/demo'
import { bytes, dateTime, kindNames, measuredBytes, percent, shortPath } from './lib/format'
import { deferredView } from './components/DeferredView'
import { Modal } from './components/Modal'
import { focusReturnTarget, restoreFocus } from './lib/focus-return'
import { ExecutionRecoveryNotice } from './components/ExecutionRecoveryNotice'
import { MoleCleanupCenter } from './components/MoleCleanupCenter'
import { ProcessSnapshot } from './components/ProcessSnapshot'
import { useMaintenance } from './lib/use-maintenance'
import { useProjectArtifacts } from './lib/use-project-artifacts'
import { useInstallers } from './lib/use-installers'
import { AnalysisNavigation } from './components/AnalysisNavigation'
import { recentAnalysisPaths } from './lib/recent-analysis'
import { AnalysisTaskNotice } from './components/AnalysisTaskNotice'
import { useMotionVisibility, WorkspaceAtmosphere } from './components/TaskMotion'
import { useWindowDrag } from './lib/use-window-drag'
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
const AiDeepScan = deferredView(async () => ({
  default: (await import('./components/AiDeepScan')).AiDeepScan,
}))
const PersonalFiles = deferredView(async () => ({
  default: (await import('./components/PersonalFiles')).PersonalFiles,
}))
const QuickCleanup = deferredView(async () => ({
  default: (await import('./components/QuickCleanup')).QuickCleanup,
}))
const CleanReview = deferredView(async () => ({
  default: (await import('./components/CleanReview')).CleanReview,
}))
const ReviewPlanDialog = deferredView(
  async () => ({ default: (await import('./components/ReviewPlanDialog')).ReviewPlanDialog }),
  { onClose: (props) => props.onClose() },
)
const ExecutionArchiveCenter = deferredView(async () => ({
  default: (await import('./components/ExecutionArchiveCenter')).ExecutionArchiveCenter,
}))
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

const StorageMonitor = deferredView(async () => ({
  default: (await import('./components/StorageMonitor')).StorageMonitor,
}))

const navigation = [
  ['overview', LayoutGrid, '概览'],
  ['quickCleanup', Zap, '速战速决'],
  ['softwareCleanup', Sparkles, '软件清理'],
  ['personalFiles', FolderOpen, '文件整理'],
  ['duplicateFiles', Copy, '重复文件'],
  ['similarPhotos', Copy, '相似照片与视频'],
  ['aiScan', BrainCircuit, 'AI 深度扫描'],
  ['analyze', HardDrive, '空间分析'],
  ['cleanup', Sparkles, '清理'],
  ['apps', Box, '应用列表'],
  ['status', Activity, '系统状态'],
  ['maintenance', Wrench, '系统维护'],
  ['storageMonitor', Activity, '持续管理'],
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
  useMotionVisibility()
  const dragWindow = useWindowDrag()
  const language = useLanguage()
  useEffect(() => {
    document.documentElement.lang = language === 'zh' ? 'zh-CN' : language
    document.title = 'MooMole'
  }, [language])
  const [data, setData] = useState<Bootstrap | null>(null)
  const folderSchedules = useFolderSchedules(api.desktop && !!data, folderScheduleTransport)
  const scheduledFiles = useScheduledFiles(api.desktop && !!data, scheduledFilesTransport)
  const [page, setPage] = useState<Page>('overview')
  const [diskRefresh, setDiskRefresh] = useState(0)
  const [moleCleanSignal, setMoleCleanSignal] = useState(0)
  const [moleCleanHistorySignal, setMoleCleanHistorySignal] = useState(0)
  const [moleCleanActive, setMoleCleanActive] = useState(false)
  const [quickMode, setQuickMode] = useState<'quick' | 'essential'>('quick')
  useEffect(() => {
    if (!api.desktop) return
    let live = true
    const listening = listenMenuBar(() => {
      if (live) changePage('storageMonitor')
    })
    void listening.catch(() => {})
    return () => {
      live = false
      void listening.then((stop) => stop()).catch(() => {})
    }
  }, [])
  const [softwareTab, setSoftwareTab] = useState<'cache' | 'attachments'>('cache')
  const [softwareSource, setSoftwareSource] =
    useState<SoftwareContentSource>('mailSandboxDownloads')
  const [mediaPlaylist, setMediaPlaylist] = useState('')
  const [telegramExport, setTelegramExport] = useState('')
  const [browserProfile, setBrowserProfile] = useState<BrowserProfile | null>(null)
  const contentTransport = useMemo(
    () => softwareContentTransport(softwareSource, mediaPlaylist, telegramExport, browserProfile),
    [softwareSource, mediaPlaylist, telegramExport, browserProfile],
  )
  const softwareFiles = usePersonalFiles(
    page === 'softwareCleanup' && softwareTab === 'attachments',
    data?.home ?? '',
    contentTransport,
  )
  useEffect(() => {
    softwareFiles.setRoot(softwareContentRoot(data?.home ?? '', softwareSource))
  }, [data?.home, softwareSource])
  const aiScan = useAiScan(data?.home ?? '')
  const personalFiles = usePersonalFiles(page === 'personalFiles', data?.home ?? '')
  const [personalMode, setPersonalMode] = useState<'files' | 'livePhotos'>('files')
  const duplicates = usePersonalFiles(
    page === 'duplicateFiles',
    data?.home ?? '',
    duplicateTransport,
  )
  const similar = usePersonalFiles(
    page === 'similarPhotos',
    data?.home ?? '',
    similarTransport,
    'Pictures',
  )
  const activeFiles =
    page === 'similarPhotos' ? similar : page === 'duplicateFiles' ? duplicates : personalFiles
  useEffect(() => {
    const detail = personalFiles.result
    if (!detail) return
    setData((current) =>
      current
        ? {
            ...current,
            executions: preserveExecutionProgress(
              [detail, ...current.executions.filter((value) => value.id !== detail.id)],
              current.executions,
            ),
          }
        : current,
    )
  }, [personalFiles.result])
  useEffect(() => {
    const detail = duplicates.result
    if (!detail) return
    setData((current) =>
      current
        ? {
            ...current,
            executions: preserveExecutionProgress(
              [detail, ...current.executions.filter((value) => value.id !== detail.id)],
              current.executions,
            ),
          }
        : current,
    )
  }, [duplicates.result])
  useEffect(() => {
    const detail = similar.result
    if (!detail) return
    setData((current) =>
      current
        ? {
            ...current,
            executions: preserveExecutionProgress(
              [detail, ...current.executions.filter((value) => value.id !== detail.id)],
              current.executions,
            ),
          }
        : current,
    )
  }, [similar.result])
  useEffect(() => {
    const detail = softwareFiles.result
    if (!detail) return
    setData((current) =>
      current
        ? {
            ...current,
            executions: preserveExecutionProgress(
              [detail, ...current.executions.filter((value) => value.id !== detail.id)],
              current.executions,
            ),
          }
        : current,
    )
  }, [softwareFiles.result])
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
  const [quickScanning, setQuickScanning] = useState(false)
  const quickScanCancelled = useRef(false)
  const quickScanActive = useRef(false)
  const [quickScanStatus, setQuickScanStatus] = useState('')
  const [apps, setApps] = useState<AppEntry[] | null>(null)
  const [appsTaskId, setAppsTaskId] = useState<string | null>(null)
  const [uninstallTarget, setUninstallTarget] = useState<UninstallTarget | null>(null)
  const [uninstallActive, setUninstallActive] = useState(false)
  const [appControlTarget, setAppControlTarget] = useState<AppControlTarget | null>(null)
  const [appControlActive, setAppControlActive] = useState(false)
  const [applicationUpdateTarget, setApplicationUpdateTarget] = useState<UninstallTarget | null>(
    null,
  )
  const [applicationUpdateActive, setApplicationUpdateActive] = useState(false)
  const [appDiscoveryTarget, setAppDiscoveryTarget] = useState<UninstallTarget | null>(null)
  const [appDiscoveryActive, setAppDiscoveryActive] = useState(false)
  const [applicationUpdateHistory, setApplicationUpdateHistory] = useState(false)
  const [deviceBackupsActive, setDeviceBackupsActive] = useState(false)
  const [deviceBackupsOpenSignal, setDeviceBackupsOpenSignal] = useState(0)
  const [spaceExplanation, setSpaceExplanation] = useState<{ path: string | null } | null>(null)
  const [registrationIntent, setRegistrationIntent] = useState(false)
  const [managedTarget, setManagedTarget] = useState<
    import('./lib/app-uninstall').UninstallTarget | null
  >(null)
  const [managedActive, setManagedActive] = useState(false)
  const [appsTab, setAppsTab] = useState<'installed' | 'startup'>('installed')
  const [startupActive, setStartupActive] = useState(false)
  const [startupToolbar, setStartupToolbar] = useState<HTMLElement | null>(null)
  const [restorationTarget, setRestorationTarget] = useState<RestorationTarget | null>(null)
  const [restorationBatchTarget, setRestorationBatchTarget] =
    useState<RestorationBatchTarget | null>(null)
  const [restorationActive, setRestorationActive] = useState(false)
  const [volumePickerOpenSignal, setVolumePickerOpenSignal] = useState(0)
  const [fileArchiveActive, setFileArchiveActive] = useState(false)
  const [folderArchiveActive, setFolderArchiveActive] = useState(false)
  const [folderArchiveSignal, setFolderArchiveSignal] = useState(0)
  const [zipExtractionActive, setZipExtractionActive] = useState(false)
  const [zipExtractionSignal, setZipExtractionSignal] = useState(0)
  const [transferActive, setTransferActive] = useState(false)
  const [transferRestoreTarget, setTransferRestoreTarget] = useState<TransferRestoreTarget | null>(
    null,
  )
  const [transferRestoreActive, setTransferRestoreActive] = useState(false)
  const [transferTarget, setTransferTarget] = useState<TransferTarget | null>(null)
  const [fileArchiveTarget, setFileArchiveTarget] = useState<FileArchiveTarget | null>(null)
  const [searchRepairActive, setSearchRepairActive] = useState(false)
  const [searchRepairOpenSignal, setSearchRepairOpenSignal] = useState(0)
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
  const aiHistoryRequest = useRef('')
  useEffect(() => {
    const snapshot = aiScan.snapshot
    if (!snapshot?.finishedAt || aiHistoryRequest.current === snapshot.id) return
    aiHistoryRequest.current = snapshot.id
    void refreshRecords().catch((error) => setError(`AI 扫描历史无法刷新：${String(error)}`))
  }, [aiScan.snapshot?.id, aiScan.snapshot?.finishedAt])
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
  const [autoCheckPlanId, setAutoCheckPlanId] = useState<string | null>(null)
  const [recoveryNavigation, setRecoveryNavigation] = useState(0)
  const [archiveOpenSignal, setArchiveOpenSignal] = useState(0)
  const [historyEpoch, setHistoryEpoch] = useState(0)
  const [executionPending, setExecutionPending] = useState(false)
  const [executionTrackingError, setExecutionTrackingError] = useState('')
  const movingExecution = data?.executions.some(
    (item) => item.state === 'running' && item.method === 'move',
  )
  const executionActive =
    api.desktop &&
    (managedActive ||
      transferActive ||
      startupActive ||
      fileArchiveActive ||
      folderArchiveActive ||
      zipExtractionActive ||
      executionPending ||
      !!data?.executions.some((item) => item.state === 'running'))
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
      setSystemLanguage(initial.systemLanguage)
      setLanguagePreference(initial.settings.language)
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
    if (busyRef.current) return false
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
      return true
    } catch (error) {
      if (!observation.current.signal.aborted) {
        if (kind === 'analyze') setAnalysisTask(failedAnalysisTask(trackedAnalysis, error))
        else setError(String(error).replace(/^(?:TaskResultError|Error): /, ''))
      }
      return false
    } finally {
      if (observation.current.signal.aborted) return false
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

  async function scanQuickCleanup(scope: 'all' | 'software' | 'clean' = 'all') {
    if (!data || quickScanActive.current || busyRef.current) return
    setQuickMode(scope === 'clean' ? 'essential' : 'quick')
    quickScanActive.current = true
    quickScanCancelled.current = false
    setQuickScanning(true)
    setCleanPreview(null)
    setAppsStale(true)
    const failures: string[] = []
    try {
      for (const [kind, label] of [
        ['clean', '缓存与日志'],
        ['apps', '应用'],
        ['analyze', '大文件'],
      ] as const) {
        if ((scope === 'software' && kind === 'analyze') || (scope === 'clean' && kind !== 'clean'))
          continue
        if (quickScanCancelled.current || observation.current.signal.aborted) break
        setQuickScanStatus(`正在扫描${label}…`)
        const success = await execute(kind, kind === 'analyze' ? data.home : undefined)
        if (!success) failures.push(label)
      }
      setQuickScanStatus(
        quickScanCancelled.current
          ? '扫描已停止，已取得的结果仍可查看；其余范围未完成。'
          : failures.length
            ? `${failures.join('、')}扫描未完成，请重新扫描；较早结果仅供参考。`
            : scope === 'clean'
              ? '扫描完成，请确认本轮推荐的缓存；其他项目可展开审阅。'
              : scope === 'software'
                ? '软件清理扫描完成。按缓存目录标识归组，未发现不代表没有其他数据。'
                : '扫描完成。大文件来自主目录空间分析，应用大小为 Mole 估算；结果可能不完整。',
      )
    } finally {
      quickScanActive.current = false
      setQuickScanning(false)
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
    if (quickScanActive.current) quickScanCancelled.current = true
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
    setLanguagePreference(saved.language)
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
  async function createPlan(scanId: string, ids: string[], autoCheck = false) {
    const saved = await api.createReviewPlan(scanId, ids)
    setAutoCheckPlanId(autoCheck ? saved.id : null)
    setPlan(saved)
    const reviewPlans = await api.reviewPlans()
    setData((current) => (current ? { ...current, reviewPlans } : current))
  }
  async function openPlan(id: string) {
    setAutoCheckPlanId(null)
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

  const canRunWithoutEngine =
    api.desktop &&
    !busy &&
    !moleCleanActive &&
    !quickScanning &&
    !aiScan.running &&
    !personalFiles.running &&
    !duplicates.running &&
    !similar.running &&
    !softwareFiles.running &&
    !uninstallActive &&
    !appControlActive &&
    !applicationUpdateActive &&
    !managedActive &&
    !appDiscoveryActive &&
    !deviceBackupsActive &&
    !startupActive &&
    !searchRepairActive &&
    !restorationActive &&
    !fileArchiveActive &&
    !folderArchiveActive &&
    !zipExtractionActive &&
    !transferActive &&
    !transferRestoreActive &&
    !folderSchedules.running &&
    !scheduledFiles.running &&
    !refreshActive &&
    analysisTask?.state !== 'starting'
  const canRun = data.engine.available && canRunWithoutEngine
  const canStartAi = (aiScan.includeMole ? canRun : canRunWithoutEngine) && !executionActive
  const pageName = navigation.find(([id]) => id === page)![2]
  const lastSnapshot = snapshot ? dateTime(snapshot.collected_at) : null
  const cpuUsage = statusPercent(snapshot?.cpu?.usage) ?? undefined
  const memoryUsage = statusPercent(snapshot?.memory?.used_percent) ?? undefined
  const totalCandidates = cleanup?.items.reduce((sum, item) => sum + (item.size ?? 0), 0)
  const workspaceMotion =
    moleCleanActive ||
    (executionActive && !executionTrackingError) ||
    uninstallActive ||
    refreshActive
      ? 'working'
      : (busy && !cancelPending && busy.kind !== 'history') ||
          analysisTask?.state === 'starting' ||
          (quickScanning && !quickScanCancelled.current) ||
          (aiScan.running && !aiScan.stopping && !aiScan.error) ||
          projects.reading ||
          projects.loadingRoots ||
          !!projects.measuring ||
          !!projects.checkingGit ||
          installers.reading ||
          !!installers.checking ||
          maintenance.reading ||
          maintenance.reviewing
        ? 'scanning'
        : 'idle'

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="sidebar-drag" data-tauri-drag-region="deep" onMouseDown={dragWindow} />
        <div className="brand" data-tauri-drag-region="deep" onMouseDown={dragWindow}>
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
              title={name}
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
        <WorkspaceAtmosphere phase={workspaceMotion} />
        <header
          className="topbar"
          data-scrolled={toolbarScrolled}
          data-tauri-drag-region="deep"
          onMouseDown={dragWindow}
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
                  data-tauri-drag-region="false"
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
                <button
                  className="icon-button"
                  disabled={!api.desktop || !canRun}
                  aria-label="选择磁盘"
                  title="选择磁盘"
                  onClick={() => setVolumePickerOpenSignal((n) => n + 1)}
                >
                  <HardDrive size={17} />
                </button>
                <button
                  className="button secondary compact"
                  disabled={!api.desktop}
                  onClick={() => setDeviceBackupsOpenSignal((n) => n + 1)}
                >
                  设备备份…
                </button>
                <button
                  className="icon-button"
                  aria-label="空间占用说明"
                  title="空间占用说明"
                  onClick={() => setSpaceExplanation({ path: null })}
                >
                  <Info size={17} />
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
                {page === 'overview' && (
                  <button
                    className="button secondary compact"
                    disabled={!api.desktop}
                    onClick={() => setMoleCleanHistorySignal((value) => value + 1)}
                  >
                    <History size={15} />
                    清理记录
                  </button>
                )}
                {page === 'overview' && (
                  <button
                    className="button secondary compact"
                    disabled={!api.desktop}
                    onClick={() => setDiskRefresh((value) => value + 1)}
                  >
                    <RefreshCw size={15} />
                    刷新磁盘
                  </button>
                )}
                {page === 'status' && (
                  <button
                    className="button secondary compact"
                    disabled={!canRun || statusWatch.phase !== 'idle'}
                    onClick={() => void execute('status')}
                  >
                    <RefreshCw size={15} className={busy?.kind === 'status' ? 'spin' : ''} />
                    刷新快照
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
                {(page === 'personalFiles' ||
                  page === 'duplicateFiles' ||
                  page === 'similarPhotos') &&
                  (activeFiles.running ? (
                    <button
                      className="button secondary compact"
                      disabled={activeFiles.starting || activeFiles.stopping}
                      onClick={() => void activeFiles.cancel()}
                    >
                      <Square size={13} />
                      {activeFiles.stopping
                        ? '正在停止…'
                        : page === 'personalFiles' && personalMode === 'livePhotos'
                          ? '停止本次扫描或处理'
                          : activeFiles.snapshot.phase === 'processing'
                            ? '停止后续项目'
                            : '停止扫描'}
                    </button>
                  ) : (
                    <button
                      className="button primary compact"
                      type="button"
                      onClick={() =>
                        page === 'personalFiles' && personalMode === 'livePhotos'
                          ? void activeFiles.startLive()
                          : void activeFiles.start()
                      }
                      disabled={!canRun || executionActive || !activeFiles.root.trim()}
                    >
                      <Search size={15} />
                      {page === 'similarPhotos'
                        ? activeFiles.videoCandidates
                          ? '查找相似视频'
                          : '查找相似照片'
                        : page === 'duplicateFiles'
                          ? '查找重复文件'
                          : personalMode === 'livePhotos'
                            ? '扫描 Live Photo 对'
                            : '扫描文件'}
                    </button>
                  ))}
                {page === 'personalFiles' && (
                  <button
                    className="button secondary compact"
                    disabled={!canRun || executionActive}
                    onClick={() => setZipExtractionSignal((v) => v + 1)}
                  >
                    解压压缩包…
                  </button>
                )}
                {page === 'personalFiles' && (
                  <button
                    className="button secondary compact"
                    disabled={!canRun || executionActive}
                    onClick={() => setFolderArchiveSignal((v) => v + 1)}
                  >
                    归档文件夹…
                  </button>
                )}
                {page === 'aiScan' && (
                  <>
                    <button
                      className="button secondary compact"
                      disabled={!api.desktop || aiScan.running || aiScan.detecting}
                      onClick={() => void aiScan.detect()}
                    >
                      <RefreshCw size={14} />
                      {aiScan.detecting ? '正在检测…' : '检测 Agent'}
                    </button>
                    {aiScan.running ? (
                      <button
                        className="button secondary compact"
                        disabled={aiScan.starting || aiScan.stopping}
                        onClick={() => void aiScan.cancel()}
                      >
                        <Square size={13} />
                        {aiScan.stopping ? '正在停止…' : '停止扫描'}
                      </button>
                    ) : (
                      <button
                        type="submit"
                        form="ai-scan-form"
                        className="button primary compact"
                        disabled={
                          !canStartAi ||
                          executionActive ||
                          !aiScan.ready ||
                          !aiScan.consent ||
                          !aiScan.root.trim()
                        }
                      >
                        <BrainCircuit size={15} />
                        开始深度扫描
                      </button>
                    )}
                  </>
                )}
                {page === 'softwareCleanup' &&
                  softwareTab === 'attachments' &&
                  (softwareFiles.running ? (
                    <button
                      className="button secondary compact"
                      disabled={softwareFiles.starting || softwareFiles.stopping}
                      onClick={() => void softwareFiles.cancel()}
                    >
                      停止附件任务
                    </button>
                  ) : (
                    <button
                      className="button primary compact"
                      disabled={
                        !canRun ||
                        executionActive ||
                        (softwareSource === 'vlcPlaylist' && !mediaPlaylist) ||
                        (softwareSource === 'telegramExport' && !telegramExport) ||
                        (softwareSource === 'chromeDownloads' && !browserProfile)
                      }
                      onClick={() => void softwareFiles.start()}
                    >
                      {softwareSource === 'vlcPlaylist'
                        ? '扫描离线影音'
                        : softwareSource === 'telegramExport'
                          ? '扫描聊天导出附件'
                          : softwareSource === 'chromeDownloads'
                            ? '扫描下载记录'
                            : '扫描附件副本'}
                    </button>
                  ))}
                {(page === 'quickCleanup' ||
                  (page === 'softwareCleanup' && softwareTab === 'cache')) && (
                  <button
                    className="button primary compact"
                    disabled={!canRun || executionActive}
                    onClick={() =>
                      void scanQuickCleanup(
                        page === 'softwareCleanup'
                          ? 'software'
                          : quickMode === 'essential'
                            ? 'clean'
                            : 'all',
                      )
                    }
                  >
                    <Search size={15} />
                    {quickScanning ? '正在扫描…' : cleanPreview ? '重新扫描' : '扫描可清理项'}
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
                {page === 'apps' && appsTab === 'startup' && (
                  <span className="toolbar-actions" ref={setStartupToolbar} />
                )}
                {page === 'apps' && appsTab === 'installed' && (
                  <>
                    <button
                      className="button secondary compact"
                      disabled={!api.desktop || applicationUpdateActive}
                      onClick={() => setApplicationUpdateHistory(true)}
                    >
                      应用更新记录
                    </button>
                    <button
                      className="button secondary compact"
                      disabled={!canRun}
                      onClick={() => void execute('apps')}
                    >
                      <RefreshCw size={15} />
                      {apps ? '刷新列表' : '读取应用'}
                    </button>
                  </>
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
                  <>
                    <button
                      className="button secondary compact"
                      disabled={!api.desktop || executionActive || uninstallActive || refreshActive}
                      onClick={() => setArchiveOpenSignal((v) => v + 1)}
                    >
                      归档…
                    </button>
                    <span className="toolbar-note">
                      客户端任务保留 {data.settings.retentionDays} 天
                    </span>
                  </>
                )}
              </>
            )}
          </div>
          <span className="read-only-tag">
            <ShieldCheck size={13} />
            {page === 'quickCleanup' ||
            page === 'softwareCleanup' ||
            page === 'personalFiles' ||
            page === 'duplicateFiles' ||
            page === 'similarPhotos' ||
            page === 'cleanup' ||
            page === 'apps' ||
            page === 'maintenance'
              ? '逐项确认'
              : moleCleanActive
                ? '完整清理执行中'
                : '只读检查'}
          </span>
        </header>
        <main className="main-content" tabIndex={-1} aria-label={`${pageName}内容`}>
          {page === 'apps' && (
            <div className="software-content-modes" role="tablist" aria-label="应用管理类型">
              <button
                role="tab"
                aria-selected={appsTab === 'installed'}
                className={`button compact ${appsTab === 'installed' ? 'primary' : 'secondary'}`}
                disabled={startupActive}
                onClick={() => setAppsTab('installed')}
              >
                已安装应用
              </button>
              <button
                role="tab"
                aria-selected={appsTab === 'startup'}
                className={`button compact ${appsTab === 'startup' ? 'primary' : 'secondary'}`}
                disabled={startupActive}
                onClick={() => setAppsTab('startup')}
              >
                启动与后台
              </button>
            </div>
          )}
          <ManagedAppCenter
            desktop={api.desktop}
            target={managedTarget}
            onClose={() => setManagedTarget(null)}
            historyVisible={page === 'history'}
            enabled={canRun && !executionActive}
            onActiveChange={setManagedActive}
            onChanged={() => setAppsStale(true)}
          />
          <StartupManagement
            visible={page === 'apps' && appsTab === 'startup'}
            historyVisible={page === 'history'}
            enabled={canRun && !executionActive}
            toolbar={startupToolbar}
            onActiveChange={setStartupActive}
          />
          <ExecutionArchiveCenter
            openSignal={archiveOpenSignal}
            enabled={api.desktop}
            transport={api.archiveTransport}
            onChanged={async () => {
              const initial = await api.bootstrap()
              setSystemLanguage(initial.systemLanguage)
              setLanguagePreference(initial.settings.language)
              setData(initial)
              setExecutionTrackingError('')
              setHistoryEpoch((v) => v + 1)
            }}
          />
          <QuickLookRefreshCenter
            key={`maintenance-${historyEpoch}`}
            openSignal={refreshOpenSignal}
            enabled={api.desktop}
            historyVisible={page === 'history'}
            transport={api.refreshTransport}
            onActiveChange={setRefreshActive}
            initialRecords={data.maintenanceRefreshes}
          />
          <TransferRestorationCenter
            target={transferRestoreTarget}
            historyVisible={page === 'history'}
            onClose={() => setTransferRestoreTarget(null)}
            onActiveChange={setTransferRestoreActive}
            onOutcome={() => void refreshRecords(true)}
            onHistory={() => changePage('history')}
          />
          <FileTransferCenter
            target={transferTarget}
            onClose={() => setTransferTarget(null)}
            onActiveChange={setTransferActive}
            onOutcome={() => {
              setHistoryEpoch((v) => v + 1)
              void refreshRecords()
            }}
          />
          <FileArchiveCenter
            target={fileArchiveTarget}
            home={data.home}
            onClose={() => setFileArchiveTarget(null)}
            onActiveChange={setFileArchiveActive}
            onOutcome={() => {
              setHistoryEpoch((v) => v + 1)
              void refreshRecords()
            }}
          />
          <FolderArchiveCenter
            openSignal={folderArchiveSignal}
            home={data.home}
            onActiveChange={setFolderArchiveActive}
            onOutcome={() => {
              setHistoryEpoch((v) => v + 1)
              void refreshRecords()
            }}
            onHistory={() => changePage('history')}
          />
          <ZipExtractionCenter
            openSignal={zipExtractionSignal}
            home={data.home}
            onActiveChange={setZipExtractionActive}
            onOutcome={() => {
              setHistoryEpoch((v) => v + 1)
              void refreshRecords()
            }}
            onHistory={() => changePage('history')}
          />
          <FileRestorationCenter
            target={restorationTarget}
            transport={restorationTransport}
            onClose={() => setRestorationTarget(null)}
            onActiveChange={setRestorationActive}
            onOutcome={() => {
              void refreshRecords(true)
            }}
          />
          <FileRestorationBatchCenter
            target={restorationBatchTarget}
            onClose={() => setRestorationBatchTarget(null)}
            onActiveChange={setRestorationActive}
            onOutcome={() => {
              void refreshRecords(true)
            }}
          />
          <MountedVolumePicker
            openSignal={volumePickerOpenSignal}
            enabled={canRun}
            onAnalyze={(target) => {
              changePage('analyze')
              void execute('analyze', target)
            }}
          />
          <DeviceBackups
            openSignal={deviceBackupsOpenSignal}
            enabled={canRun}
            onActiveChange={setDeviceBackupsActive}
          />
          {spaceExplanation && (
            <SpaceExplanation
              key={spaceExplanation.path ?? 'guide'}
              path={spaceExplanation.path}
              desktop={api.desktop}
              enabled={canRun}
              onClose={() => setSpaceExplanation(null)}
              onReveal={reveal}
              onAction={(action) => {
                if (action === 'backups') setDeviceBackupsOpenSignal((n) => n + 1)
                else if (action === 'home') {
                  changePage('analyze')
                  void execute('analyze', data.home)
                } else changePage(action === 'cleanup' ? 'quickCleanup' : 'storageMonitor')
              }}
            />
          )}
          <SearchRepairCenter
            openSignal={searchRepairOpenSignal}
            transport={searchRepairTransport}
            onActiveChange={setSearchRepairActive}
            onOutcome={() => {
              void refreshRecords(true)
            }}
          />
          <ApplicationControlCenter
            target={appControlTarget}
            onClose={() => setAppControlTarget(null)}
            onActiveChange={setAppControlActive}
            onOutcome={() => {
              setAppsStale(true)
              void refreshRecords(true)
            }}
          />
          <ApplicationUpdateCenter
            canRestore={canRun && !executionActive}
            target={applicationUpdateTarget}
            historyOpen={applicationUpdateHistory}
            onClose={() => setApplicationUpdateTarget(null)}
            onHistoryClose={() => setApplicationUpdateHistory(false)}
            onActiveChange={setApplicationUpdateActive}
            onOutcome={() => {
              setAppsStale(true)
              void refreshRecords(true)
            }}
          />
          <AppUpdateDiscoveryCenter
            target={appDiscoveryTarget}
            onClose={() => setAppDiscoveryTarget(null)}
            onActiveChange={setAppDiscoveryActive}
            onDownloaded={(target) => {
              setAppDiscoveryTarget(null)
              setApplicationUpdateTarget(target)
            }}
          />
          <ApplicationUninstallCenter
            key={`uninstall-${historyEpoch}`}
            target={uninstallTarget}
            enabled={api.desktop}
            historyVisible={page === 'history'}
            transport={api.uninstallTransport}
            onTargetClose={() => setUninstallTarget(null)}
            onActiveChange={setUninstallActive}
            onOutcome={() => setAppsStale(true)}
          />
          {(executionActive || executionTrackingError) && (
            <section
              className="notice execution-recovery"
              aria-label={movingExecution ? '文件移动状态' : '清理状态'}
            >
              <div>
                <strong>
                  {movingExecution
                    ? executionTrackingError
                      ? '文件移动状态待确认'
                      : '文件移动进行中'
                    : executionTrackingError
                      ? '清理状态待确认'
                      : '清理进行中'}
                </strong>
                <p>
                  {executionTrackingError ||
                    (movingExecution
                      ? '移动结果逐项保存在历史中。停止后续项目不会移回已经完成的文件；离开页面后可从历史继续查看。'
                      : '逐项结果持续保存在历史中；关闭审阅框会请求停止后续项目，已发生的结果会保留。')}
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
          <MoleCleanupCenter
            desktop={api.desktop}
            enabled={canRun && !executionActive}
            openSignal={moleCleanSignal}
            historySignal={moleCleanHistorySignal}
            onActiveChange={setMoleCleanActive}
          />
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
            <StorageOverview
              desktop={api.desktop}
              refreshSignal={diskRefresh}
              canClean={canRun && !executionActive}
              onClean={() => setMoleCleanSignal((value) => value + 1)}
            />
          )}

          {page === 'personalFiles' && (
            <div className="software-content-modes" role="tablist" aria-label="文件整理类型">
              <button
                role="tab"
                aria-selected={personalMode === 'files'}
                className={`button compact ${personalMode === 'files' ? 'primary' : 'secondary'}`}
                disabled={personalFiles.running}
                onClick={() => setPersonalMode('files')}
              >
                普通文件
              </button>
              <button
                role="tab"
                aria-selected={personalMode === 'livePhotos'}
                className={`button compact ${personalMode === 'livePhotos' ? 'primary' : 'secondary'}`}
                disabled={personalFiles.running}
                onClick={() => setPersonalMode('livePhotos')}
              >
                Live Photos
              </button>
            </div>
          )}
          {page === 'personalFiles' && personalMode === 'livePhotos' && (
            <LivePhotoPairs
              controller={personalFiles}
              enabled={canRun && !executionActive}
              protectedPaths={data.settings.protectedPaths}
              onHistory={() => changePage('history')}
            />
          )}
          {((page === 'personalFiles' && personalMode === 'files') ||
            page === 'duplicateFiles' ||
            page === 'similarPhotos') && (
            <PersonalFiles
              key={page}
              mode={
                page === 'similarPhotos'
                  ? 'similar'
                  : page === 'duplicateFiles'
                    ? 'duplicates'
                    : 'personal'
              }
              controller={activeFiles}
              onArchive={page === 'personalFiles' ? setFileArchiveTarget : undefined}
              onTransfer={page === 'personalFiles' ? setTransferTarget : undefined}
              home={data.home}
              protectedPaths={data.settings.protectedPaths}
              enabled={canRun && !executionActive}
              onHistory={() => changePage('history')}
            />
          )}
          {page === 'aiScan' && (
            <AiDeepScan
              controller={aiScan}
              canStart={canStartAi}
              home={data.home}
              protectedPaths={data.settings.protectedPaths}
              enabled={canRun && !executionActive}
              onCreatePlan={createPlan}
              onReveal={reveal}
              onOrganize={async (snapshotId, evidenceIds) => {
                await personalFiles.fromAi(snapshotId, evidenceIds)
                changePage('personalFiles')
              }}
            />
          )}

          {page === 'softwareCleanup' && (
            <div className="software-content-modes" role="tablist" aria-label="软件内容类型">
              <button
                role="tab"
                aria-selected={softwareTab === 'cache'}
                className={`button compact ${softwareTab === 'cache' ? 'primary' : 'secondary'}`}
                disabled={quickScanning || softwareFiles.running}
                onClick={() => setSoftwareTab('cache')}
              >
                缓存
              </button>
              <button
                role="tab"
                aria-selected={softwareTab === 'attachments'}
                className={`button compact ${softwareTab === 'attachments' ? 'primary' : 'secondary'}`}
                disabled={quickScanning || softwareFiles.running}
                onClick={() => setSoftwareTab('attachments')}
              >
                附件、下载与离线
              </button>
            </div>
          )}
          {page === 'softwareCleanup' && softwareTab === 'attachments' && (
            <>
              <label className="software-source">
                软件内容来源
                <select
                  aria-label="软件内容来源"
                  value={softwareSource}
                  disabled={softwareFiles.running}
                  onChange={(e) => setSoftwareSource(e.target.value as SoftwareContentSource)}
                >
                  {softwareContentSources.map((source) => (
                    <option key={source.id} value={source.id}>
                      {source.label}
                    </option>
                  ))}
                </select>
              </label>
              {softwareSource === 'chromeDownloads' && (
                <BrowserDownloadsPicker
                  home={data.home}
                  value={browserProfile}
                  onChange={setBrowserProfile}
                  disabled={!api.desktop || softwareFiles.running || !canRun || executionActive}
                />
              )}
              {softwareSource === 'vlcPlaylist' && (
                <div className="personal-scope">
                  <label className="personal-path">
                    <span>播放列表来源</span>
                    <input aria-label="播放列表来源" value={mediaPlaylist} readOnly />
                  </label>
                  <button
                    className="button secondary compact"
                    disabled={!api.desktop || softwareFiles.running}
                    onClick={async () => {
                      try {
                        const path = await api.chooseMediaPlaylist()
                        if (path) setMediaPlaylist(path)
                      } catch (reason) {
                        setError(String(reason))
                      }
                    }}
                  >
                    <FolderOpen size={14} />
                    选择播放列表
                  </button>
                </div>
              )}
              {softwareSource === 'telegramExport' && (
                <div className="personal-scope">
                  <label className="personal-path">
                    <span>聊天导出来源</span>
                    <input aria-label="聊天导出来源" value={telegramExport} readOnly />
                  </label>
                  <button
                    className="button secondary compact"
                    disabled={!api.desktop || softwareFiles.running}
                    onClick={async () => {
                      try {
                        const path = await api.chooseTelegramExport()
                        if (path) setTelegramExport(path)
                      } catch (reason) {
                        setError(String(reason))
                      }
                    }}
                  >
                    <FolderOpen size={14} />
                    选择聊天导出清单
                  </button>
                </div>
              )}
              <PersonalFiles
                key={softwareSource}
                controller={softwareFiles}
                mode={
                  softwareSource === 'vlcPlaylist'
                    ? 'offline'
                    : softwareSource === 'telegramExport'
                      ? 'chat'
                      : softwareSource === 'chromeDownloads'
                        ? 'downloads'
                        : 'software'
                }
                softwareSource={softwareSource}
                playlistPath={mediaPlaylist}
                exportPath={telegramExport}
                browserProfile={browserProfile}
                home={data.home}
                protectedPaths={data.settings.protectedPaths}
                enabled={canRun && !executionActive}
                onHistory={() => changePage('history')}
              />
            </>
          )}
          {(page === 'quickCleanup' || (page === 'softwareCleanup' && softwareTab === 'cache')) && (
            <>
              {quickScanStatus && (
                <p className="quick-intro motion-scan" data-scanning={quickScanning} role="status">
                  <span className="motion-text" data-active={quickScanning}>
                    {quickScanStatus}
                  </span>
                </p>
              )}
              <QuickCleanup
                key={page}
                mode={page === 'softwareCleanup' ? 'software' : quickMode}
                preview={cleanPreview}
                home={data.home}
                protectedPaths={data.settings.protectedPaths}
                desktop={api.desktop}
                enabled={canRun && !executionActive}
                scanRunning={quickScanning}
                apps={apps}
                appsTaskId={appsTaskId}
                appsStale={appsStale}
                analysis={analysis}
                onUninstall={setUninstallTarget}
                onReveal={reveal}
                onProtect={protect}
                onInspect={async (scanId, ids, signal) => {
                  const saved = await api.createReviewPlan(scanId, ids)
                  try {
                    const checked = await api.checkReviewRules(saved.id, signal)
                    if (checked.cleanupConfirmation)
                      await api.discardCleanupConfirmation(checked.cleanupConfirmation.id)
                    return { ...checked, cleanupConfirmation: null }
                  } finally {
                    await refreshRecords(true)
                  }
                }}
                onCreatePlan={(scanId, ids) => createPlan(scanId, ids, true)}
                onNavigate={changePage}
              />
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
                  onExplain={(target) => setSpaceExplanation({ path: target })}
                  onOrganize={(target, mode) => {
                    if (mode === 'duplicates') duplicates.setRoot(target)
                    else personalFiles.setRoot(target)
                    changePage(mode === 'duplicates' ? 'duplicateFiles' : 'personalFiles')
                  }}
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

          {page === 'apps' && appsTab === 'installed' && (
            <>
              {registrationIntent && (
                <p className="notice">
                  选择需要修复的确切应用，展开“位置与来源”，点击“重新注册此副本…”。请确认安装路径；结果需要回到原来的图标或打开方式中验证。
                </p>
              )}
              <div className="inline-note">
                <Info size={14} />
                列表来自 Mole CLI。卸载需单独检查范围并明确确认；勾选仅用于对比。
              </div>
              {appsStale && (
                <p className="notice warning" role="status">
                  应用操作后清单可能已过期，请刷新应用清单后再检查；不会自动重复处理。
                </p>
              )}
              {apps ? (
                <ApplicationLibrary
                  onControl={setAppControlTarget}
                  onUpdate={setApplicationUpdateTarget}
                  onDiscoverUpdate={setAppDiscoveryTarget}
                  canControl={canRun && !executionActive && !appsStale}
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
                  onManagedUninstall={setManagedTarget}
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
            <>
              <ProblemRepairs
                enabled={canRun}
                onPreview={() => setRefreshOpenSignal((n) => n + 1)}
                onSearch={() => setSearchRepairOpenSignal((n) => n + 1)}
                onRegistration={() => {
                  setRegistrationIntent(true)
                  setAppsTab('installed')
                  changePage('apps')
                }}
              />
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
            </>
          )}

          {page === 'storageMonitor' && (
            <>
              <StorageMonitor
                enabled={api.desktop}
                transport={storageMonitorTransport}
                onAnalyze={() => changePage('analyze')}
              />
              <MenuBarSettings enabled={api.desktop} transport={menuBarTransport} />
              <ScheduledFiles
                enabled={api.desktop && data.engine.available}
                controller={scheduledFiles}
                chooseDirectory={() => api.chooseDirectory('选择定期处理目录')}
                onHistory={() => changePage('history')}
              />
              <FolderSchedules
                enabled={api.desktop}
                controller={folderSchedules}
                chooseDirectory={() => api.chooseDirectory('选择定期扫描目录')}
                onOrganize={(root) => {
                  personalFiles.setRoot(root)
                  changePage('personalFiles')
                }}
              />
            </>
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
              <button
                className="button secondary compact"
                disabled={!api.desktop}
                onClick={() => setMoleCleanHistorySignal((value) => value + 1)}
              >
                完整 Mole 清理记录
              </button>
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
                    <span className="muted">保存的选择范围；实际结果见执行记录</span>
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
                key={`cleanup-${historyEpoch}`}
                onReady={focusExecutionRecords}
                initialRecords={data.executions}
                loadRecords={api.executions}
                loadDetail={api.execution}
                onCancel={api.cancelLogCleanup}
                onAcknowledge={api.acknowledgeExecution}
                onRevealReceipt={api.revealExecutionItem}
                onRestore={
                  api.desktop && canRun && !executionActive ? setRestorationTarget : undefined
                }
                onRestoreBatch={
                  api.desktop && canRun && !executionActive ? setRestorationBatchTarget : undefined
                }
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
              <FileTransferHistory
                key={`transfers-${historyEpoch}`}
                canRestore={canRunWithoutEngine && !executionActive}
                onRestore={setTransferRestoreTarget}
              />
              <TransferRestorationHistory key={`transfer-restore-${historyEpoch}`} />
              <FileArchiveHistory key={`file-archives-${historyEpoch}`} />
              <FolderArchiveHistory key={`folder-archives-${historyEpoch}`} />
              <ZipExtractionHistory key={`zip-extractions-${historyEpoch}`} />
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
          executions={data.executions}
          autoCheck={autoCheckPlanId === plan.id}
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
              setCleanPreview(null)
            }
          }}
          onClose={() => setPlan(null)}
        />
      )}
      {busy && !(busy.kind === 'analyze' && page === 'analyze') && (
        <div className="task-toast motion-scan" data-scanning={!cancelPending} role="status">
          <LoaderCircle size={18} className="spin" />
          <div>
            <strong className="motion-text" data-active={!cancelPending}>
              {kindNames[busy.kind]}进行中
            </strong>
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
            清理和文件操作均需分别审阅与确认。个人文件等可靠记录可在适用范围内恢复，其他操作请按结果说明取回或重建。
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
