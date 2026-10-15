import type { CleanCandidate } from '../types'

export function pathOverlaps(a: string, b: string): boolean {
  const normalize = (path: string) => path.replace(/\/+$/, '') || '/'
  a = normalize(a)
  b = normalize(b)
  return a === '/' || b === '/' || a === b || a.startsWith(`${b}/`) || b.startsWith(`${a}/`)
}

export function protectionFor(item: CleanCandidate, rules: string[]): string[] {
  return rules.filter(
    (rule) =>
      pathOverlaps(item.path, rule) ||
      (item.resolvedPath != null && pathOverlaps(item.resolvedPath, rule)),
  )
}

// Recompute coverage for the chosen subset. A child can be measured separately
// when its upstream parent is not selected; unknown-size parents cover nothing.
export function previewTotal(items: CleanCandidate[]): { size: number; unknown: number } {
  const measured = new Set(
    items.filter((item) => item.size != null).map((item) => item.path.replace(/\/+$/, '') || '/'),
  )
  let size = 0
  let unknown = 0
  for (const item of items) {
    let parent = item.path.replace(/\/+$/, '')
    let covered = false
    while (parent.lastIndexOf('/') >= 0) {
      parent = parent.slice(0, parent.lastIndexOf('/')) || '/'
      if (measured.has(parent)) {
        covered = true
        break
      }
      if (parent === '/') break
    }
    if (!covered) {
      if (item.size == null) unknown++
      else size += item.size
    }
  }
  return { size, unknown }
}
