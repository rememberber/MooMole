import type { Analysis, AnalysisObservation, Entry } from '../types'

export function analysisObservation(analysis: Analysis): AnalysisObservation | null {
  const value = analysis.mooObservation
  const timestamp = (n: number) => Number.isSafeInteger(n) && n > 0 && n <= 8640000000000000
  return value?.version === 1 &&
    value.source === 'mole-directory-json' &&
    value.cacheUse === 'unreported' &&
    timestamp(value.startedAt) &&
    timestamp(value.finishedAt) &&
    Number.isSafeInteger(value.elapsedMs) &&
    value.elapsedMs >= 0 &&
    (value.engineVersion === null || typeof value.engineVersion === 'string') &&
    typeof value.revision === 'string'
    ? value
    : null
}

export function measurementCoverage(entries: Entry[]) {
  const coverage = { complete: 0, partial: 0, unknown: 0 }
  for (const entry of entries) {
    if (entryBytes(entry) == null) coverage.unknown++
    else if (entry.scan_status === 'partial') coverage.partial++
    else coverage.complete++
  }
  return coverage
}

export function entryBytes(entry: Entry): number | null {
  return ['complete', 'partial'].includes(entry.scan_status) &&
    Number.isFinite(entry.size) &&
    entry.size >= 0 &&
    entry.size <= Number.MAX_SAFE_INTEGER
    ? entry.size
    : null
}

export function compareEntries(a: Entry, b: Entry, sort: 'size' | 'name') {
  if (sort === 'size') {
    const left = entryBytes(a),
      right = entryBytes(b)
    if (left == null && right != null) return 1
    if (right == null && left != null) return -1
    if (left != null && right != null && left !== right) return right - left
  }
  return a.name.localeCompare(b.name, 'zh-CN') || a.path.localeCompare(b.path, 'zh-CN')
}

export interface SpaceTile {
  entry: Entry | null
  label: string
  size: number
  x: number
  y: number
  width: number
  height: number
}

// Bounded binary partition: preserve area ratios without inventing minimum
// sizes for small/unknown entries. Coordinates use a 1000 × 360 view box.
export function spaceTiles(entries: Entry[]): SpaceTile[] {
  const measured = entries
    .filter((entry) => (entryBytes(entry) ?? 0) > 0)
    .sort((a, b) => compareEntries(a, b, 'size'))
  const tiles = measured.slice(0, 12).map((entry) => ({
    entry,
    label: entry.name,
    size: entry.size,
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  })) as SpaceTile[]
  if (measured.length > 12)
    tiles.push({
      entry: null,
      label: `其他 ${measured.length - 12} 项`,
      size: measured.slice(12).reduce((sum, entry) => sum + entry.size, 0),
      x: 0,
      y: 0,
      width: 0,
      height: 0,
    })
  if (!tiles.length) return []
  function partition(items: SpaceTile[], x: number, y: number, width: number, height: number) {
    if (items.length === 1) {
      Object.assign(items[0], { x, y, width, height })
      return
    }
    const total = items.reduce((sum, item) => sum + item.size, 0)
    let split = 1,
      weight = items[0].size
    while (
      split < items.length - 1 &&
      Math.abs(weight + items[split].size - total / 2) < Math.abs(weight - total / 2)
    ) {
      weight += items[split++].size
    }
    const ratio = weight / total
    if (width >= height) {
      partition(items.slice(0, split), x, y, width * ratio, height)
      partition(items.slice(split), x + width * ratio, y, width * (1 - ratio), height)
    } else {
      partition(items.slice(0, split), x, y, width, height * ratio)
      partition(items.slice(split), x, y + height * ratio, width, height * (1 - ratio))
    }
  }
  partition(tiles, 0, 0, 1000, 360)
  return tiles
}

export const parentDirectory = (path: string) =>
  path.replace(/\/+$/, '').substring(0, path.replace(/\/+$/, '').lastIndexOf('/')) || '/'
