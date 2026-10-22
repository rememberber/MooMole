import type { Snapshot } from '../types'
import { bytes } from './format'
type Disk = NonNullable<Snapshot['disks']>[number]
export function diskMount(value: unknown): string | null {
  if (
    typeof value !== 'string' ||
    !value.startsWith('/') ||
    new TextEncoder().encode(value).length > 4096 ||
    /[\u0000-\u001f\u007f-\u009f]/.test(value)
  )
    return null
  return value === '/' ||
    !value
      .slice(1)
      .split('/')
      .some((p) => ['', '.', '..'].includes(p))
    ? value
    : null
}
const amount = (v: unknown): number | null =>
  typeof v === 'number' && Number.isSafeInteger(v) && v >= 0 ? v : null
export function diskReport(disk: Partial<Disk>) {
  const rawTotal = amount(disk.total)
  const total = rawTotal !== null && rawTotal > 0 ? rawTotal : null
  const rawUsed = amount(disk.used)
  const inconsistent = total !== null && rawUsed !== null && rawUsed > total
  const used = inconsistent ? null : rawUsed
  const remaining = total !== null && used !== null ? total - used : null
  return {
    mount: diskMount(disk.mount),
    total,
    used,
    remaining,
    percent: total !== null && used !== null ? (used / total) * 100 : null,
    inconsistent,
    external: typeof disk.external === 'boolean' ? disk.external : null,
  }
}
export function systemDisk(disks: Snapshot['disks']) {
  const roots = (Array.isArray(disks) ? disks : []).filter((d) => diskMount(d.mount) === '/')
  return roots.length === 1 ? diskReport(roots[0]) : null
}
export function diskBytes(value: number | null | undefined) {
  return value == null ? '未知' : bytes(value)
}
