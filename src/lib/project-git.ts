import { projectCatalog } from './project-artifacts'
export interface ProjectGitReport {
  protocol: 1
  revision: string
  root: string
  path: string
  checkedAt: number
  state: 'recorded' | 'unknown'
  reason: keyof typeof projectGitReasons | null
  repository: string | null
  records: number | null
  example: string | null
  canExecute: false
}
export interface ProjectGitObservation {
  report?: ProjectGitReport
  error?: string
  attempted?: boolean
}
export const projectGitReasons = {
  notListed: '本次索引未列出该目录的记录，其他内容与保护仍未确认。',
  noRepository: '所选范围内未找到仓库；可选择项目根目录后重新扫描。',
  unsupportedRepository: '仓库使用链接或工作树指针，当前读取方式尚未支持。',
  indexUnavailable: '索引缺失、共享、受限或无法确认。',
  sizeLimit: '索引超过 4 MiB 读取上限。',
  entryLimit: '索引超过 20000 条记录上限。',
  unsupportedIndex: '当前索引格式或扩展尚未支持。',
  invalidIndex: '索引格式或校验未能确认。',
  unavailable: '仓库或索引在读取期间无法确认，请稍后重试。',
}
const relative = (v: unknown): v is string =>
  typeof v === 'string' &&
  !!v &&
  new TextEncoder().encode(v).length <= 4096 &&
  !/[\u0000-\u001f\u007f-\u009f]/.test(v) &&
  !v.split('/').some((p) => ['', '.', '..', '.git'].includes(p))
const absolute = (v: unknown): v is string =>
  typeof v === 'string' &&
  v.startsWith('/') &&
  new TextEncoder().encode(v).length <= 4096 &&
  relative(v.slice(1))
const asciiFold = (v: string) => v.replace(/[A-Z]/g, (c) => c.toLowerCase())
export function readProjectGit(
  value: unknown,
  root: string,
  path: string,
  began: number,
  now = Date.now(),
): ProjectGitReport {
  const r = value as ProjectGitReport | null
  const invalid = () => {
    throw new Error('Git 记录观察无法确认，请重新检查。')
  }
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== projectCatalog.revision ||
    r.root !== root ||
    r.path !== path ||
    !absolute(root) ||
    !absolute(path) ||
    !path.startsWith(`${root}/`) ||
    r.canExecute !== false ||
    !Number.isSafeInteger(r.checkedAt) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    !['recorded', 'unknown'].includes(r.state)
  )
    return invalid()
  if (r.state === 'recorded') {
    if (
      r.reason !== null ||
      !absolute(r.repository) ||
      (r.repository !== root && !r.repository.startsWith(`${root}/`)) ||
      (r.repository !== path && !path.startsWith(`${r.repository}/`)) ||
      !Number.isSafeInteger(r.records) ||
      r.records! < 1 ||
      r.records! > 20000 ||
      !relative(r.example)
    )
      return invalid()
    const prefix = path.slice(r.repository.length + 1)
    if (
      prefix &&
      asciiFold(r.example) !== asciiFold(prefix) &&
      !asciiFold(r.example).startsWith(`${asciiFold(prefix)}/`)
    )
      return invalid()
  } else if (
    !Object.hasOwn(projectGitReasons, r.reason ?? '') ||
    r.repository !== null ||
    r.records !== null ||
    r.example !== null
  )
    return invalid()
  return r
}
