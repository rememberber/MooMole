import catalog from './installers-catalog.json' with { type: 'json' }
export const installerCatalog = catalog
export interface InstallerItem {
  path: string
  name: string
  extension: string
  sizeBytes: number | null
  measurement: 'measured' | 'unavailable'
  classification: 'nameOnly' | 'archiveUnverified'
  sharing: 'single' | 'shared'
  protection: 'matched' | 'notMatched' | 'unknown'
  usage: 'notChecked'
  canExecute: false
}
export interface InstallersReport {
  protocol: 1
  revision: string
  root: string
  checkedAt: number
  completion: 'complete' | 'partial'
  stopReason: 'timeLimit' | 'entryLimit' | 'candidateLimit' | null
  visited: number
  skipped: {
    links: number
    unreadable: number
    nonLocal: number
    depth: number
    invalidName: number
    changed: number
    unsupportedType: number
  }
  items: InstallerItem[]
  canExecute: false
}
const absolute = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  !v.endsWith('/') &&
  new TextEncoder().encode(v).length <= 4096 &&
  !/[\u0000-\u001f\u007f-\u009f]/.test(v) &&
  !v
    .slice(1)
    .split('/')
    .some((p) => ['', '.', '..'].includes(p))
export function readInstallers(
  value: unknown,
  root: string,
  began: number,
  now = Date.now(),
): InstallersReport {
  const r = value as InstallersReport | null
  const invalid = () => {
    throw new Error('安装包扫描结果无法确认，请重新扫描。')
  }
  const count = (v: number, max: number) => Number.isSafeInteger(v) && v >= 0 && v <= max
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== catalog.revision ||
    r.root !== root ||
    !absolute(root) ||
    r.canExecute !== false ||
    !count(r.checkedAt, Number.MAX_SAFE_INTEGER) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    !['complete', 'partial'].includes(r.completion) ||
    ![null, 'timeLimit', 'entryLimit', 'candidateLimit'].includes(r.stopReason) ||
    !count(r.visited, 10000) ||
    !r.skipped ||
    !Array.isArray(r.items) ||
    r.items.length > 256 ||
    r.items.length > r.visited
  )
    return invalid()
  const keys = [
    'links',
    'unreadable',
    'nonLocal',
    'depth',
    'invalidName',
    'changed',
    'unsupportedType',
  ] as const
  if (
    Object.keys(r.skipped).length !== keys.length ||
    keys.some((k) => !count(r.skipped[k], 20001)) ||
    r.completion !== (r.stopReason || keys.some((k) => r.skipped[k] > 0) ? 'partial' : 'complete')
  )
    return invalid()
  const seen = new Set<string>()
  for (const item of r.items) {
    if (
      !item ||
      !absolute(item.path) ||
      !item.path.startsWith(`${root}/`) ||
      seen.has(item.path) ||
      item.path.slice(root.length + 1).split('/').length > catalog.maxDepth ||
      item.name !== item.path.split('/').at(-1) ||
      !catalog.extensions.includes(item.extension) ||
      item.name.split('.').at(-1)?.toLowerCase() !== item.extension ||
      item.classification !== (item.extension === 'zip' ? 'archiveUnverified' : 'nameOnly') ||
      !['single', 'shared'].includes(item.sharing) ||
      !['matched', 'notMatched', 'unknown'].includes(item.protection) ||
      item.usage !== 'notChecked' ||
      item.canExecute !== false ||
      !['measured', 'unavailable'].includes(item.measurement) ||
      (item.measurement === 'measured'
        ? !count(item.sizeBytes as number, Number.MAX_SAFE_INTEGER)
        : item.sizeBytes !== null)
    )
      return invalid()
    seen.add(item.path)
  }
  return r
}
export function installerIncompleteReason(report: InstallersReport): string {
  const labels = {
    links: '跳过链接',
    unreadable: '读取未完成',
    nonLocal: '非本机、未下载或跨文件系统',
    depth: '达到层级限制',
    invalidName: '名称无法处理',
    changed: '扫描期间变化',
    unsupportedType: '包目录或特殊文件未支持',
  }
  const reasons = (Object.keys(labels) as (keyof typeof labels)[])
    .filter((k) => report.skipped[k] > 0)
    .map((k) => `${labels[k]} ${report.skipped[k]} 次`)
  if (report.stopReason)
    reasons.push(
      {
        timeLimit: '达到时间预算',
        entryLimit: '达到子项数量限制',
        candidateLimit: '达到结果数量限制',
      }[report.stopReason],
    )
  return reasons.join('；')
}

export interface InstallerZipInspection {
  protocol: 1
  revision: string
  root: string
  path: string
  checkedAt: number
  state: 'installerHint' | 'noHintInWindow' | 'unknown'
  reason: 'listingUnavailable' | 'listingUncertain' | 'listingLimit' | null
  totalEntries: number | null
  checkedEntries: number
  matchedTypes: string[]
  canExecute: false
}
export type ZipObservations = Record<string, { report?: InstallerZipInspection; error?: string }>
export function readInstallerZip(
  value: unknown,
  root: string,
  path: string,
  began: number,
  now = Date.now(),
): InstallerZipInspection {
  const r = value as InstallerZipInspection | null
  const count = (n: number, max: number) => Number.isSafeInteger(n) && n >= 0 && n <= max
  const invalid = () => {
    throw new Error('ZIP 条目检查结果无法确认，请重新检查。')
  }
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== catalog.revision ||
    r.root !== root ||
    r.path !== path ||
    !absolute(root) ||
    !absolute(path) ||
    !path.startsWith(`${root}/`) ||
    path.slice(root.length + 1).split('/').length > catalog.maxDepth ||
    path.split('.').at(-1)?.toLowerCase() !== 'zip' ||
    !count(r.checkedAt, Number.MAX_SAFE_INTEGER) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    r.canExecute !== false ||
    !['installerHint', 'noHintInWindow', 'unknown'].includes(r.state) ||
    !Array.isArray(r.matchedTypes) ||
    r.matchedTypes.length > catalog.zipHintTypes.length ||
    r.matchedTypes.some((v) => !catalog.zipHintTypes.includes(v)) ||
    new Set(r.matchedTypes).size !== r.matchedTypes.length ||
    !count(r.checkedEntries, catalog.zipWindow)
  )
    return invalid()
  if (r.state === 'unknown') {
    if (
      !['listingUnavailable', 'listingUncertain', 'listingLimit'].includes(r.reason ?? '') ||
      r.totalEntries !== null ||
      r.checkedEntries !== 0 ||
      r.matchedTypes.length !== 0
    )
      return invalid()
  } else if (
    r.reason !== null ||
    !count(r.totalEntries as number, 10000) ||
    r.totalEntries === 0 ||
    r.checkedEntries !== Math.min(r.totalEntries!, catalog.zipWindow) ||
    r.state !== (r.matchedTypes.length ? 'installerHint' : 'noHintInWindow')
  )
    return invalid()
  return r
}
export function zipInspectionText(report: InstallerZipInspection): string {
  if (report.state === 'installerHint')
    return `发现安装条目线索（${report.matchedTypes.map((v) => v.toUpperCase()).join('、')}）`
  if (report.state === 'noHintInWindow') return `前 ${report.checkedEntries} 条未见安装线索`
  return (
    {
      listingUnavailable: '列表未读完：可能损坏、超时或读取受限',
      listingUncertain: '条目名称或列表格式无法确认',
      listingLimit: '条目列表超出检查限制',
    }[report.reason!] ?? 'ZIP 内容仍未确认'
  )
}

export function installerLocations(home: string) {
  if (!absolute(home)) return []
  return catalog.locations.map((location) => ({
    ...location,
    path: location.template.startsWith('$HOME/')
      ? `${home}/${location.template.slice(6)}`
      : location.template,
  }))
}
export type InstallerSort = 'size' | 'name'
export function installerView(
  items: InstallerItem[],
  query: string,
  extension: string,
  sort: InstallerSort,
): InstallerItem[] {
  const search = query.trim().toLowerCase()
  const filtered = items.filter(
    (item) =>
      (!extension || item.extension === extension) &&
      (!search ||
        item.name.toLowerCase().includes(search) ||
        item.path.toLowerCase().includes(search)),
  )
  return filtered.sort((a, b) => {
    if (sort === 'size') {
      const left = a.measurement === 'measured' ? a.sizeBytes : null,
        right = b.measurement === 'measured' ? b.sizeBytes : null
      if (left !== null && right === null) return -1
      if (left === null && right !== null) return 1
      if (left !== null && right !== null && left !== right) return right - left
    }
    return a.path.localeCompare(b.path, 'zh-CN')
  })
}

export interface InstallerConnectionInspection {
  protocol: 1
  revision: string
  root: string
  path: string
  checkedAt: number
  state: 'pathReported' | 'pathNotListed' | 'unknown'
  reason: 'queryUnavailable' | 'listingUncertain' | 'listingLimit' | null
  canExecute: false
}
export type ConnectionObservations = Record<
  string,
  { report?: InstallerConnectionInspection; error?: string }
>
export function readInstallerConnection(
  value: unknown,
  root: string,
  path: string,
  began: number,
  now = Date.now(),
): InstallerConnectionInspection {
  const r = value as InstallerConnectionInspection | null
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== catalog.revision ||
    r.root !== root ||
    r.path !== path ||
    !absolute(root) ||
    !absolute(path) ||
    !path.startsWith(`${root}/`) ||
    path.slice(root.length + 1).split('/').length > catalog.maxDepth ||
    !['dmg', 'iso'].includes(path.split('.').at(-1)?.toLowerCase() ?? '') ||
    !Number.isSafeInteger(r.checkedAt) ||
    r.checkedAt < 0 ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    r.canExecute !== false ||
    !['pathReported', 'pathNotListed', 'unknown'].includes(r.state) ||
    (r.state === 'unknown'
      ? !['queryUnavailable', 'listingUncertain', 'listingLimit'].includes(r.reason ?? '')
      : r.reason !== null)
  )
    throw new Error('映像连接结果无法确认，请重新检查。')
  return r
}
export function connectionInspectionText(report: InstallerConnectionInspection): string {
  if (report.state === 'pathReported') return '系统报告此路径已连接，请先保留'
  if (report.state === 'pathNotListed') return '本次连接列表未见此路径；文件占用仍未确认'
  return (
    {
      queryUnavailable: '连接状态无法确认：读取失败或超时',
      listingUncertain: '连接状态无法确认：列表格式或路径无法识别',
      listingLimit: '连接状态无法确认：列表超出检查限制',
    }[report.reason!] ?? '连接状态无法确认'
  )
}
