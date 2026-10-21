import catalog from './project-artifacts-catalog.json' with { type: 'json' }
export const projectCatalog = catalog
export interface ProjectArtifact {
  path: string
  name: string
  context: 'nodeContext' | 'rustContext' | 'nameOnly'
  projectPath: string | null
  protection: 'matched' | 'notMatched' | 'unknown'
  measurement: 'notMeasured'
  canExecute: false
}
export interface ProjectArtifactsReport {
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
  }
  items: ProjectArtifact[]
  canExecute: false
}
const absolute = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  v.length <= 4096 &&
  !/[\u0000-\u001f\u007f]/.test(v) &&
  !v
    .slice(1)
    .split('/')
    .some((p) => p.length === 0) &&
  !v.split('/').some((p) => p === '.' || p === '..') &&
  !v.endsWith('/')
export function readProjectArtifacts(
  value: unknown,
  root: string,
  began: number,
  now = Date.now(),
): ProjectArtifactsReport {
  const r = value as ProjectArtifactsReport | null
  const invalid = () => {
    throw new Error('工程产物结果无法确认，请重新扫描。')
  }
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== catalog.revision ||
    r.root !== root ||
    !absolute(r.root) ||
    r.canExecute !== false ||
    !Number.isSafeInteger(r.checkedAt) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    !['complete', 'partial'].includes(r.completion) ||
    ![null, 'timeLimit', 'entryLimit', 'candidateLimit'].includes(r.stopReason) ||
    !Number.isSafeInteger(r.visited) ||
    r.visited < 0 ||
    r.visited > 10000 ||
    !r.skipped ||
    !Array.isArray(r.items) ||
    r.items.length > 256
  )
    return invalid()
  const keys = ['links', 'unreadable', 'nonLocal', 'depth', 'invalidName', 'changed'] as const
  if (
    Object.keys(r.skipped).length !== keys.length ||
    keys.some(
      (k) => !Number.isSafeInteger(r.skipped[k]) || r.skipped[k] < 0 || r.skipped[k] > 20001,
    ) ||
    r.items.length > r.visited ||
    r.completion !==
      (r.stopReason !== null || keys.some((k) => r.skipped[k] > 0) ? 'partial' : 'complete')
  )
    return invalid()
  const seen = new Set<string>()
  for (const item of r.items) {
    if (
      !item ||
      !absolute(item.path) ||
      !item.path.startsWith(`${root}/`) ||
      seen.has(item.path) ||
      item.path.split('/').at(-1) !== item.name ||
      !catalog.targets.includes(item.name) ||
      !['nodeContext', 'rustContext', 'nameOnly'].includes(item.context) ||
      !['matched', 'notMatched', 'unknown'].includes(item.protection) ||
      item.measurement !== 'notMeasured' ||
      item.canExecute !== false
    )
      return invalid()
    const parts = item.path.slice(root.length + 1).split('/')
    if (
      parts.length > 6 ||
      parts
        .slice(0, -1)
        .some(
          (name) =>
            name.startsWith('.') ||
            ['Library', 'Applications'].includes(name) ||
            catalog.targets.includes(name),
        )
    )
      return invalid()
    const parent = item.path.slice(0, item.path.lastIndexOf('/'))
    if (
      item.context === 'nameOnly'
        ? item.projectPath !== null
        : item.projectPath !== parent ||
          !absolute(item.projectPath) ||
          (item.context === 'nodeContext' ? item.name !== 'node_modules' : item.name !== 'target')
    )
      return invalid()
    if (
      [...seen].some((path) => item.path.startsWith(`${path}/`) || path.startsWith(`${item.path}/`))
    )
      return invalid()
    seen.add(item.path)
  }
  return r
}
export const projectContextText = {
  nodeContext: 'Node 依赖目录线索',
  rustContext: 'Rust 构建目录线索',
  nameOnly: '用途与归属待确认',
}
export function projectIncompleteReason(
  report: Pick<ProjectArtifactsReport, 'skipped' | 'stopReason'>,
): string {
  const labels = {
    links: '跳过链接',
    unreadable: '读取未完成',
    nonLocal: '非本机或未下载',
    depth: '达到层级限制',
    invalidName: '名称无法处理',
    changed: '扫描期间变化',
  }
  const reasons = (Object.keys(labels) as (keyof typeof labels)[])
    .filter((key) => report.skipped[key] > 0)
    .map((key) => `${labels[key]} ${report.skipped[key]} 次`)
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

export interface ProjectMeasurement {
  protocol: 1
  revision: string
  root: string
  path: string
  checkedAt: number
  completion: 'complete' | 'partial'
  stopReason: 'timeLimit' | 'entryLimit' | 'sizeLimit' | null
  allocatedBytes: number
  files: number
  visited: number
  skipped: ProjectArtifactsReport['skipped']
  hardLinks: number
  special: number
  canExecute: false
}
export function readProjectMeasurement(
  value: unknown,
  root: string,
  path: string,
  began: number,
  now = Date.now(),
): ProjectMeasurement {
  const r = value as ProjectMeasurement | null
  const invalid = () => {
    throw new Error('空间测量结果无法确认，请重新测量。')
  }
  const count = (v: number, max: number) => Number.isSafeInteger(v) && v >= 0 && v <= max
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== catalog.revision ||
    r.root !== root ||
    r.path !== path ||
    !absolute(root) ||
    !absolute(path) ||
    !path.startsWith(`${root}/`) ||
    !catalog.targets.includes(path.split('/').at(-1)!) ||
    r.canExecute !== false ||
    !count(r.checkedAt, Number.MAX_SAFE_INTEGER) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    !['complete', 'partial'].includes(r.completion) ||
    ![null, 'timeLimit', 'entryLimit', 'sizeLimit'].includes(r.stopReason) ||
    !count(r.allocatedBytes, Number.MAX_SAFE_INTEGER) ||
    r.allocatedBytes % 512 !== 0 ||
    !count(r.visited, 20000) ||
    !count(r.files, r.visited) ||
    (r.files === 0 && r.allocatedBytes !== 0) ||
    !count(r.hardLinks, r.visited) ||
    !count(r.special, r.visited) ||
    !r.skipped
  )
    return invalid()
  const parts = path.slice(root.length + 1).split('/')
  if (
    parts.length > 6 ||
    parts
      .slice(0, -1)
      .some(
        (p) =>
          p.startsWith('.') ||
          ['Library', 'Applications'].includes(p) ||
          catalog.targets.includes(p),
      )
  )
    return invalid()
  const keys = ['links', 'unreadable', 'nonLocal', 'depth', 'invalidName', 'changed'] as const
  if (
    Object.keys(r.skipped).length !== keys.length ||
    keys.some((k) => !count(r.skipped[k], 40001)) ||
    r.skipped.invalidName !== 0 ||
    r.completion !==
      (r.stopReason || r.hardLinks || r.special || keys.some((k) => r.skipped[k] > 0)
        ? 'partial'
        : 'complete')
  )
    return invalid()
  return r
}
export function projectMeasurementReason(report: ProjectMeasurement): string {
  const base = projectIncompleteReason({
    skipped: report.skipped,
    stopReason: report.stopReason === 'sizeLimit' ? null : report.stopReason,
  })
  return [
    base,
    report.hardLinks ? `跳过共享硬链接 ${report.hardLinks} 次` : '',
    report.special ? `跳过特殊文件 ${report.special} 次` : '',
    report.stopReason === 'sizeLimit' ? '达到大小计数限制' : '',
  ]
    .filter(Boolean)
    .join('；')
}
