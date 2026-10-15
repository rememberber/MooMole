import type { Analysis, Bootstrap, Cleanup, CleanPreview, Snapshot } from '../types'

const GB = 1024 ** 3
export const demoCleanPreview: CleanPreview = {
  output: '示例预览日志：本页仅用于展示界面，没有扫描本机文件。',
  dryRun: true,
  protocol: 1,
  revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
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
    },
  ],
}
export const demoBootstrap: Bootstrap = {
  home: '/Users/demo',
  engine: {
    available: false,
    revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
    reason: '浏览器仅展示示例数据；请启动桌面 App 读取本机。',
  },
  settings: { theme: 'system', retentionDays: 30, protectedPaths: [], acknowledged: true },
  history: [],
  latestCleanPreview: null,
  reviewPlans: [],
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
