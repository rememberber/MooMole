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
  engine: { available: boolean; revision: string; reason: string | null }
  home: string
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
}
