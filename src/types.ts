export type Page = 'overview' | 'analyze' | 'cleanup' | 'apps' | 'status' | 'history'
export type TaskKind = 'analyze' | 'status' | 'apps' | 'cleanup' | 'clean'
export type ScanState = 'complete' | 'partial' | 'unavailable'
export interface Settings {
  theme: 'light' | 'dark' | 'system'
  retentionDays: number
  protectedPaths: string[]
  acknowledged: boolean
}
export interface TaskRecord {
  id: string
  kind: TaskKind
  path: string | null
  state: string
  createdAt: number
  finishedAt: number | null
  message: string | null
}
export interface Bootstrap {
  settings: Settings
  history: TaskRecord[]
  engine: { available: boolean; version: string | null; revision: string; reason: string | null }
  home: string
  latestCleanPreview: CleanPreview | null
  reviewPlans: ReviewPlanSummary[]
  executions: ExecutionSummary[]
}
export interface Entry {
  name: string
  path: string
  size: number
  is_dir: boolean
  scan_status: ScanState
}
export interface Analysis {
  path: string
  scan_status: ScanState
  entries: Entry[]
  total_size: number
  total_files?: number
}
export interface Snapshot {
  collected_at: string
  host?: string
  platform?: string
  uptime?: string
  health_score?: number
  hardware?: { model?: string; cpu_model?: string; total_ram?: string; os_version?: string }
  cpu?: { usage?: number; logical_cpu?: number }
  memory?: { total?: number; used?: number; used_percent?: number; pressure?: string }
  disks?: { mount: string; total: number; used: number; used_percent: number; external: boolean }[]
  top_processes?: { pid: number; name: string; cpu: number; memory_bytes?: number }[]
  process_stale?: boolean
}
export interface AppEntry {
  name: string
  bundle_id: string
  source: string
  uninstall_name: string
  path: string
  size: string
}
export interface CleanupItem {
  id: string
  name: string
  path: string
  impact: string
  size: number | null
  scanStatus: ScanState
  protected: boolean
  reason: string | null
}
export interface Cleanup {
  items: CleanupItem[]
  readOnly: true
}
export interface CleanPreview {
  output: string
  dryRun: true
  protocol: 1 | 2
  revision: string
  systemIncluded: boolean
  items: CleanCandidate[]
  scanId?: string
  scannedAt?: number
  expiresAt?: number
}
export interface ReviewPlanSummary {
  id: string
  scanId: string
  createdAt: number
  expiresAt: number
  itemCount: number
}
export interface ReviewPlan extends ReviewPlanSummary {
  checkedAt: number
  readOnly: true
  requiresMoleRecheck: true
  ruleCheckedAt?: number | null
  items: {
    id: string
    path: string
    size: number | null
    section: string
    state: string
    reason: string
    moleRule?: {
      code: string
      source: string
      reason: string
      module?: ModuleCheck
      activity?: ActivityCheck | null
    } | null
    provenance?: Provenance | null
    inventoryEntries?: number | null
    impact?: Impact | null
  }[]
}
export interface CleanCandidate {
  id: string
  upstreamIdentity: string
  path: string
  section: string
  size: number | null
  itemCount: number
  coveredBy: string | null
  isDirectory: boolean
  resolvedPath: string | null
  impact?: Impact | null
  provenance?: Provenance | null
}
export interface ActivityCheck {
  version: number
  code: string
  source: string
  reason: string
}
export interface Impact {
  version: number
  kind: 'downloadCache' | 'logs' | 'diagnostics' | 'appCache' | 'unknown'
  title: string
  purpose: string
  consequence: string
  recovery: string
  requiresReview: true
}
export interface TaskResults {
  analyze: Analysis
  status: Snapshot
  apps: AppEntry[]
  cleanup: Cleanup
  clean: CleanPreview
}
export interface TaskEvent {
  id: string
  kind: TaskKind
  state: string
  result: unknown
  error: string | null
  resultUnavailable?: string | null
}

export interface ExecutionSummary {
  id: string
  planId: string
  method: string
  state: string
  createdAt: number
  finishedAt: number | null
  itemCount: number
  movedCount: number
  uncertainCount: number
  retainedForReview: boolean
}
export interface ExecutionReceipt {
  destination: string
  device: string
  inode: string
}
export interface ExecutionDetail extends ExecutionSummary {
  revision: string
  protocol: number
  snapshotSha256: string
  items: {
    id: string
    path: string
    state: string
    reason: string
    receipt: ExecutionReceipt | null
  }[]
  events: {
    itemId: string
    state: string
    createdAt: number
    reason: string
    receipt: ExecutionReceipt | null
  }[]
}

export interface ModuleCheck {
  version: number
  binding: string
  scope: string
  code: string
  source: string
  reason: string
}

export interface Provenance {
  version: number
  observations: { path: string; functions: string[]; deleteGuard: string; finalGuard: string }[]
}
