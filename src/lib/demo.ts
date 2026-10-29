import type { Analysis, Bootstrap, Cleanup, CleanPreview, Snapshot, Impact } from '../types'

const GB = 1024 ** 3
const demoCacheImpact: Impact = {
  version: 1,
  kind: 'appCache',
  title: '应用缓存范围',
  purpose: '位置处于应用缓存范围，但实际内容仍由各应用决定。',
  consequence: '可能触发重建或重新下载，也可能影响离线使用、会话或数据库状态。',
  recovery: '无法统一判断可恢复性；重要会话、离线内容和未同步数据应保留。',
  requiresReview: true,
}
const demoLogImpact: Impact = {
  version: 1,
  kind: 'logs',
  title: '应用运行日志',
  purpose: '该范围通常存放应用运行和排查问题的历史记录。',
  consequence: '清理可能影响故障追踪、审计或支持人员分析历史问题。',
  recovery: '历史日志通常不能重新生成；未来日志不能补回过去的记录。',
  requiresReview: true,
}
export const demoCleanPreview: CleanPreview = {
  output: '示例预览日志：本页仅用于展示界面，没有扫描本机文件。',
  dryRun: true,
  protocol: 2,
  revision: '308bf9911179ddfa40e5df9fdc42095a18ba5cfc',
  systemIncluded: false,
  items: [
    {
      id: 'demo-1',
      upstreamIdentity: 'demo-1',
      path: '/Users/demo/Library/Caches/ExampleApp',
      section: 'User app cache',
      size: 1.5 * GB,
      itemCount: 24,
      coveredBy: null,
      isDirectory: true,
      resolvedPath: '/Users/demo/Library/Caches/ExampleApp',
      impact: demoCacheImpact,
      provenance: {
        version: 1,
        observations: [
          {
            path: '/Users/demo/Library/Caches/ExampleApp',
            functions: [
              'record_dry_run_cleanup_target',
              '_safe_clean_impl',
              'safe_clean_guarded',
              'clean_user_essentials',
            ],
            deleteGuard: '_user_cache_deno_delete_guard',
            finalGuard: '_user_cache_deno_delete_guard',
          },
        ],
      },
    },
    {
      id: 'demo-2',
      upstreamIdentity: 'demo-2',
      path: '/Users/demo/Library/Caches/ExampleApp/Images',
      section: 'User app cache',
      size: 0.5 * GB,
      itemCount: 12,
      coveredBy: '/Users/demo/Library/Caches/ExampleApp',
      isDirectory: true,
      resolvedPath: '/Users/demo/Library/Caches/ExampleApp/Images',
      impact: demoCacheImpact,
    },
    {
      id: 'demo-3',
      upstreamIdentity: 'demo-3',
      path: '/Users/demo/Library/Logs/ExampleApp',
      section: 'User logs',
      size: null,
      itemCount: 1,
      coveredBy: null,
      isDirectory: true,
      resolvedPath: '/Users/demo/Library/Logs/ExampleApp',
      impact: demoLogImpact,
    },
  ],
}
export const demoBootstrap: Bootstrap = {
  home: '/Users/demo',
  engine: {
    available: false,
    version: null,
    revision: '308bf9911179ddfa40e5df9fdc42095a18ba5cfc',
    reason: '浏览器仅展示示例数据；请启动桌面 App 读取本机。',
  },
  settings: {
    language: 'system',
    theme: 'system',
    retentionDays: 30,
    protectedPaths: [],
    acknowledged: true,
  },
  history: [],
  latestCleanPreview: null,
  reviewPlans: [],
  executions: [],
}
export const demoSnapshot: Snapshot = {
  collected_at: '2026-10-15T08:00:00Z',
  host: '示例 Mac',
  uptime: '3d 8h',
  hardware: {
    model: 'MacBook Pro',
    cpu_model: 'Apple Silicon',
    total_ram: '32 GB',
    os_version: 'macOS',
  },
  cpu: { usage: 12.8, logical_cpu: 12 },
  memory: { total: 32 * GB, used: 13.4 * GB, used_percent: 41.9, pressure: 'normal' },
  disks: [{ mount: '/', total: 512 * GB, used: 328.6 * GB, used_percent: 64.2, external: false }],
  top_processes: [{ pid: 101, name: '示例进程', cpu: 6.2, memory_bytes: 680 * 1024 ** 2 }],
}
export const demoAnalysis: Analysis = {
  path: '/Users/demo',
  scan_status: 'partial',
  total_size: 118.4 * GB,
  entries: [
    {
      name: 'Library',
      path: '/Users/demo/Library',
      size: 46.2 * GB,
      is_dir: true,
      scan_status: 'partial',
    },
    {
      name: 'Projects',
      path: '/Users/demo/Projects',
      size: 32.6 * GB,
      is_dir: true,
      scan_status: 'complete',
    },
    {
      name: 'Downloads',
      path: '/Users/demo/Downloads',
      size: 21.8 * GB,
      is_dir: true,
      scan_status: 'complete',
    },
    {
      name: 'Documents',
      path: '/Users/demo/Documents',
      size: 11.4 * GB,
      is_dir: true,
      scan_status: 'complete',
    },
    {
      name: 'Pictures',
      path: '/Users/demo/Pictures',
      size: 6.4 * GB,
      is_dir: true,
      scan_status: 'complete',
    },
  ],
}
export const demoCleanup: Cleanup = {
  readOnly: true,
  items: [
    {
      id: 'xcode',
      name: 'Xcode 构建缓存',
      path: '/Users/demo/Library/Developer/Xcode/DerivedData',
      impact: '由 Xcode 重新生成，后续构建可能变慢。',
      size: 8.2 * GB,
      scanStatus: 'complete',
      protected: false,
      reason: null,
    },
    {
      id: 'npm',
      name: 'npm 下载缓存',
      path: '/Users/demo/.npm/_cacache',
      impact: '包下载缓存，后续安装可能需要重新联网下载。',
      size: 2.4 * GB,
      scanStatus: 'complete',
      protected: false,
      reason: null,
    },
    {
      id: 'homebrew',
      name: 'Homebrew 下载缓存',
      path: '/Users/demo/Library/Caches/Homebrew',
      impact: '安装包缓存，后续安装可能需要重新下载。',
      size: null,
      scanStatus: 'unavailable',
      protected: false,
      reason: '未发现此目录。',
    },
  ],
}
