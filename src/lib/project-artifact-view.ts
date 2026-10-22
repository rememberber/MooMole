import {
  groupProjectArtifacts,
  type ProjectArtifact,
  type ProjectMeasurement,
} from './project-artifacts'
export type ProjectMeasurements = Record<string, { report?: ProjectMeasurement; error?: string }>
export interface ProjectArtifactFilters {
  query: string
  context: '' | ProjectArtifact['context']
  sort: 'path' | 'size'
  page: number
}
export const initialProjectFilters: ProjectArtifactFilters = {
  query: '',
  context: '',
  sort: 'path',
  page: 1,
}
export const projectPageSize = 50
// Partial observations with no measured file remain unknown, rather than zero.
function observedSize(measurements: ProjectMeasurements, path: string): number | null {
  const report = measurements[path]?.report
  return !report || (report.completion === 'partial' && report.files === 0)
    ? null
    : report.allocatedBytes
}
export function projectArtifactView(
  items: ProjectArtifact[],
  measurements: ProjectMeasurements,
  filters: ProjectArtifactFilters,
) {
  const query = filters.query.trim().toLocaleLowerCase()
  const matches = items.filter(
    (item) =>
      (!filters.context || item.context === filters.context) &&
      (!query || item.path.toLocaleLowerCase().includes(query)),
  )
  const groups = groupProjectArtifacts(matches)
  for (const group of groups)
    group.items.sort((a, b) => {
      if (filters.sort === 'size') {
        const left = observedSize(measurements, a.path),
          right = observedSize(measurements, b.path)
        if (left === null && right !== null) return 1
        if (right === null && left !== null) return -1
        if (left !== null && right !== null && left !== right) return right - left
      }
      return a.path.localeCompare(b.path)
    })
  const total = matches.length
  const pages = Math.max(1, Math.ceil(total / projectPageSize))
  const page = Math.max(1, Math.min(pages, Math.trunc(filters.page) || 1))
  const start = (page - 1) * projectPageSize,
    end = Math.min(start + projectPageSize, total)
  let offset = 0
  const visible = groups.flatMap((group) => {
    const from = Math.max(0, start - offset),
      to = Math.min(group.items.length, end - offset)
    offset += group.items.length
    return from < to ? [{ ...group, items: group.items.slice(from, to) }] : []
  })
  return { groups: visible, total, pages, page, start: total ? start + 1 : 0, end }
}
