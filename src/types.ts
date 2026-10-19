export type Page = 'overview' | 'analyze' | 'cleanup' | 'apps' | 'status' | 'history'
export type TaskKind = 'analyze' | 'status' | 'apps' | 'cleanup' | 'clean' | 'history'
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
  large_files?: LargeFile[] | null
  mooObservation?: AnalysisObservation | null
}
export interface LargeFile {
  name: string
  path: string
  size: number
}
export interface AnalysisObservation {
  version: 1
  source: 'mole-directory-json'
  startedAt: number
  finishedAt: number
  elapsedMs: number
  engineVersion: string | null
  revision: string
  cacheUse: 'unreported'
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
  top_processes?: ProcessSample[] | null
  process_collected_at?: string | null
  process_stale?: boolean | null
}
export interface ProcessSample {
  pid: number
  name: string
  cpu?: number | null
  memory_bytes?: number | null
}
export interface AppEntry {
  name: string
  bundle_id: string
  source: string
  uninstall_name: string
  path: string
  size: string
}
export interface MolePathProtection {
  version: 2
  revision: string
  source: 'moleUninstallPathSubset'
  code: 'protected' | 'notMatched' | 'unknown'
  reason:
    | 'sharedRoot'
    | 'containerRuntime'
    | 'systemComponent'
    | 'criticalData'
    | 'endpointSecurityIdentity'
    | 'endpointSecurity'
    | null
  issue:
    | 'rulesUnavailable'
    | 'locationUnverified'
    | 'pathUnsupported'
    | 'identityUnverified'
    | 'identityLimitReached'
    | null
}
export interface ApplicationInspection {
  protocol: 21
  checkedAt: number
  bundleId: string | null
  version: string | null
  build: string | null
  identity: 'matches' | 'different' | 'unreported' | 'unavailable'
  embedded: {
    version: 1
    revision: string
    code: 'observed' | 'partial' | 'notChecked'
    issue:
      | 'identityChanged'
      | 'identityUnavailable'
      | 'rulesUnavailable'
      | 'bundleIdUnsupported'
      | 'unavailable'
      | 'unsupported'
      | 'changed'
      | 'limitReached'
      | null
    entries: number
    directories: number
    plists: number
    bundles: { relativePath: string; bundleId: string; kind: 'appex' | 'xpc' | 'loginItem' }[]
  }
  extensionData: {
    version: 3
    groups: ApplicationInspection['extensionData']['byHost'] & {
      version: 1
      source: 'clientEmbeddedIdNameBoundary'
    }
    revision: string
    code: 'observed' | 'partial' | 'notChecked' | 'unknown'
    issue:
      | 'identityChanged'
      | 'identityUnavailable'
      | 'rulesUnavailable'
      | 'bundleIdUnsupported'
      | 'homeUnavailable'
      | 'embeddedIncomplete'
      | 'embeddedChanged'
      | 'embeddedUnavailable'
      | 'limitReached'
      | 'byHostIncomplete'
      | 'groupsIncomplete'
      | null
    identifiers: string[]
    entries: {
      relativePath: string
      sources: {
        bundleId: string
        kind:
          | 'scripts'
          | 'fileProvider'
          | 'cache'
          | 'container'
          | 'httpStorage'
          | 'httpCookies'
          | 'preferences'
          | 'webKit'
          | 'byHost'
          | 'groupContainer'
      }[]
      status: 'present' | 'notFound' | 'unknown'
      issue: 'unavailable' | 'unsupported' | 'changed' | null
      protectionReasons: ('systemLocation' | 'currentApplication' | 'clientDirectory')[]
      molePathProtection: MolePathProtection
    }[]
    byHost: {
      code: 'observed' | 'partial' | 'notChecked'
      issue: 'unavailable' | 'unsupported' | 'changed' | 'limitReached' | null
      entries: number
      candidates: number
    }
  }
  installations: {
    version: 5
    source: 'moleListRegistryRootsVolumesAndReceipts'
    revision: string
    code: 'observed' | 'partial' | 'unknown'
    issue:
      | 'identityChanged'
      | 'identityUnavailable'
      | 'rulesUnavailable'
      | 'registryUnavailable'
      | 'listUnavailable'
      | 'rootsIncomplete'
      | 'volumesIncomplete'
      | 'receiptsIncomplete'
      | 'limitReached'
      | 'candidateUnavailable'
      | 'candidatesChanged'
      | null
    roots: {
      code: 'observed' | 'partial' | 'notChecked'
      issue: 'unavailable' | 'changed' | 'limitReached' | null
      rootCount: number
      entries: number
      candidates: number
      firstUncertainPath: string | null
    }
    volumes: {
      code: 'observed' | 'partial' | 'notChecked'
      issue: 'unavailable' | 'changed' | 'limitReached' | 'unsupportedVolume' | null
      rootCount: number
      entries: number
      candidates: number
      firstUncertainPath: string | null
    }
    receipts: {
      code: 'observed' | 'partial' | 'notChecked'
      issue: 'unavailable' | 'changed' | 'limitReached' | 'unsupportedReceipt' | null
      packageCount: number
      checkedCount: number
      entries: number
      candidates: number
    }
    copies: {
      path: string
      bundleId: string
      version: string | null
      build: string | null
      relation: 'sameIdentifier' | 'relatedIdentifier' | 'channelName'
    }[]
  }
  associations: {
    version: 7
    names: {
      version: 2
      source: 'moleLiteralAppNameLibrary'
      code: 'observed' | 'notChecked' | 'unknown'
      name: string | null
      candidates: number
      issue:
        | 'identityChanged'
        | 'identityUnavailable'
        | 'rulesUnavailable'
        | 'bundleIdUnsupported'
        | 'homeUnavailable'
        | 'nameUnavailable'
        | 'nameUnsupported'
        | 'limitReached'
        | null
    }
    byHost: ApplicationInspection['extensionData']['byHost']
    revision: string
    code: 'observed' | 'unknown'
    issue:
      | 'identityChanged'
      | 'identityUnavailable'
      | 'rulesUnavailable'
      | 'bundleIdUnsupported'
      | 'homeUnavailable'
      | null
    discovery: {
      code: 'observed' | 'partial' | 'notChecked'
      issue: 'unavailable' | 'unsupported' | 'changed' | 'limitReached' | null
      rootCount: number
      entries: number
      candidates: number
      firstUncertainRoot: string | null
    }
    entries: {
      kind: string
      relativePath: string
      status: 'present' | 'notFound' | 'unknown'
      issue: 'unavailable' | 'unsupported' | 'changed' | null
      protectionReasons: ('systemLocation' | 'currentApplication' | 'clientDirectory')[]
      molePathProtection: MolePathProtection
    }[]
  }
  background: {
    version: 1
    revision: string
    code: 'notRequired' | 'directRoot' | 'restricted' | 'unknown'
    marker: 'absent' | 'enabled' | 'disabled' | 'unknown'
    rootKind:
      | 'systemApplications'
      | 'userApplications'
      | 'systemInputMethods'
      | 'userInputMethods'
      | 'playCover'
      | 'externalApplications'
      | null
    reason: 'outsideRoots' | 'unreadableRoot' | 'duplicateRoot' | null
    issue:
      | 'identityChanged'
      | 'identityUnavailable'
      | 'rulesUnavailable'
      | 'markerUnknown'
      | 'rootsUnavailable'
      | 'rootsChanged'
      | null
  }
  moleProtection: {
    version: 1
    revision: string
    code: 'protected' | 'notMatched' | 'unknown'
    issue: 'identityChanged' | 'identityUnavailable' | 'rulesUnavailable' | null
  }
  officialUninstaller: {
    version: 1
    revision: string
    code: 'officialRequired' | 'notMatched' | 'unknown'
    vendor: string | null
    issue: 'identityChanged' | 'identityUnavailable' | 'rulesUnavailable' | 'nameUnavailable' | null
  }
  protection: {
    reasons: ('systemLocation' | 'currentApplication' | 'clientDirectory')[]
    matchedPaths: string[]
  }
  running: {
    code: 'observed' | 'notObserved' | 'unknown'
    pids: number[]
    otherCopies: number
    reason: string
  }
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
    descendantRules?: {
      version: number
      code: string
      checkedEntries: number
      path?: string | null
      source?: string | null
      reason: string
    } | null
    provenance?: Provenance | null
    inventoryEntries?: number | null
    impact?: Impact | null
    permissions?: PermissionObservation | null
  }[]
}
export interface PermissionObservation {
  version: number
  code: string
  reason: string
  findings: {
    kind: string
    state: string
    reason: string
    osError?: number | null
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
  history: MoleHistory
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

export interface MoleHistory {
  version: 1
  revision: string
  collectedAt: number
  limit: number
  operationsSource: MoleHistorySource
  deletionsSource: MoleHistorySource
  sessions: {
    command: string
    run_id: string
    attribution: 'run' | 'command' | 'ambiguous'
    started_at: string
    ended_at: string
    items: number
    size: string
    operation_count: number
    failed_tasks: number
    actions: {
      removed: number
      trashed: number
      skipped: number
      failed: number
      rebuilt: number
      other: number
    }
  }[]
  deletions: {
    timestamp: string
    mode: string
    status: string
    size_kb: number | null
    path: string
  }[]
}

export interface MoleHistorySource {
  path: string
  state: 'ready' | 'missing' | 'unavailable'
  reason: string | null
}
export interface UpdateCheckResult {
  status: 'unpublished' | 'current' | 'available'
  currentVersion: string
  latestVersion: string | null
  channel: 'stable' | 'preview'
  architecture: 'arm64' | 'x64'
  notes: string
  notesTruncated: boolean
  asset: {
    platform: 'darwin'
    architecture: 'arm64' | 'x64'
    packageType: 'dmg'
    fileName: string
    url: string
    size: number
    sha512: string
  } | null
}
export interface UpdateSnapshot {
  checking: boolean
  checkError: string | null
  autoDownload: boolean
  preferenceError: string | null
  recovering: boolean
  recoveryError: string | null
  installing: boolean
  installError: string | null
  check: UpdateCheckResult | null
  download: {
    state: 'idle' | 'downloading' | 'verifying' | 'cancelling' | 'cancelled' | 'failed' | 'ready'
    restored: boolean
    fileName: string | null
    transferred: number
    total: number
    error: string | null
  }
}
