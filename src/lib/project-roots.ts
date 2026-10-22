import { projectCatalog } from './project-artifacts'
export interface ProjectRootsReport {
  protocol: 1
  revision: string
  source: string
  checkedAt: number
  state: 'ready' | 'missing'
  paths: string[]
  canExecute: false
}
const path = (v: unknown): v is string =>
  typeof v === 'string' &&
  new TextEncoder().encode(v).length <= 4096 &&
  v.startsWith('/') &&
  v !== '/' &&
  !/[\u0000-\u001f\u007f-\u009f]/.test(v) &&
  !v
    .slice(1)
    .split('/')
    .some((p) => ['', '.', '..'].includes(p))
export function readProjectRoots(
  value: unknown,
  home: string,
  began: number,
  now = Date.now(),
): ProjectRootsReport {
  const r = value as ProjectRootsReport | null
  if (
    !r ||
    r.protocol !== 1 ||
    r.revision !== projectCatalog.revision ||
    !path(home) ||
    r.source !== `${home}/.config/mole/purge_paths` ||
    !Number.isSafeInteger(r.checkedAt) ||
    r.checkedAt < began - 5000 ||
    r.checkedAt > now + 5000 ||
    r.canExecute !== false ||
    !['ready', 'missing'].includes(r.state) ||
    !Array.isArray(r.paths) ||
    r.paths.length > 64 ||
    r.paths.some((p) => !path(p)) ||
    new Set(r.paths).size !== r.paths.length ||
    (r.state === 'missing' && r.paths.length !== 0)
  ) {
    throw new Error('Mole 目录配置结果无法确认，请重新读取或手动选择目录。')
  }
  return r
}
