import type { Analysis } from '../types'

export const analysisLimits = {
  entries: 20_000,
  largeFiles: 1_000,
  pathBytes: 4_096,
  nameBytes: 1_024,
}
const encoder = new TextEncoder()
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)
const text = (value: unknown, max: number): value is string =>
  typeof value === 'string' &&
  value.length > 0 &&
  !value.includes('\0') &&
  encoder.encode(value).length <= max
const path = (value: unknown): value is string =>
  text(value, analysisLimits.pathBytes) &&
  value.startsWith('/') &&
  (value === '/' ||
    value
      .slice(1)
      .split('/')
      .every((part) => part !== '' && part !== '.' && part !== '..'))
const count = (value: unknown, minimum = 0) =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= minimum
const state = (value: unknown) => ['complete', 'partial', 'unavailable'].includes(value as string)

function reject(field: string): never {
  throw new Error(
    `空间分析结果格式异常，未更新。请重新分析；若反复出现，请核对内置引擎。诊断：${field}`,
  )
}

export function decodeAnalysis(value: unknown): Analysis {
  if (
    !object(value) ||
    !path(value.path) ||
    !state(value.scan_status) ||
    !count(value.total_size, -1)
  )
    reject('summary')
  if (value.overview !== undefined && value.overview !== false) reject('overview')
  if (value.total_files !== undefined && !count(value.total_files)) reject('total_files')
  if (!Array.isArray(value.entries)) reject('entries')
  if (value.entries.length > analysisLimits.entries)
    throw new Error('空间分析返回的直接子项超过 20000 项，未更新。请选择更小的目录后重新分析。')
  const seen = new Set<string>()
  for (const entry of value.entries) {
    if (
      !object(entry) ||
      !text(entry.name, analysisLimits.nameBytes) ||
      entry.name === '.' ||
      entry.name === '..' ||
      entry.name.includes('/') ||
      !path(entry.path) ||
      seen.has(entry.path) ||
      typeof entry.is_dir !== 'boolean' ||
      !state(entry.scan_status) ||
      !count(entry.size, -1)
    )
      reject('entry')
    const slash = entry.path.lastIndexOf('/')
    const parent = entry.path.slice(0, slash) || '/'
    const basename = entry.path.slice(slash + 1)
    // Pinned Mole appends this display marker to symlink names. The path
    // remains the literal link entry; the label does not prove its type.
    if (
      parent !== value.path ||
      !basename ||
      (entry.name !== basename && entry.name !== `${basename} →`)
    )
      reject('entry')
    seen.add(entry.path)
  }
  // Optional large-file records retain their existing per-record unknown and
  // scope handling. This gate bounds the container; it does not grant actions.
  if (
    value.large_files != null &&
    (!Array.isArray(value.large_files) || value.large_files.length > analysisLimits.largeFiles)
  )
    reject('large_files')
  return value as unknown as Analysis
}
