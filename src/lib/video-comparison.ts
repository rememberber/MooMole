import { invoke } from '@tauri-apps/api/core'
import {
  readFilePreview,
  type PreviewTarget,
  type FilePreviewReport,
  type PreviewContent,
} from './file-preview'
export interface VideoTarget extends PreviewTarget {
  name: string
  size: number
}
export type VideoContent = Extract<PreviewContent, { kind: 'media' }>
export type VideoReport = FilePreviewReport & { content: VideoContent }
export interface VideoComparisonTransport {
  prepare: (comparisonId: string, targets: VideoTarget[]) => Promise<unknown>
  release: (comparisonId: string) => Promise<unknown>
}
const uuid = (s: unknown): s is string =>
  typeof s === 'string' && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(s)
function fail(): never {
  throw Error('视频对比结果与本次选择不一致，已停止显示；请重新扫描。')
}
export function videoTargets(targets: VideoTarget[]): void {
  if (
    targets.length < 2 ||
    targets.length > 4 ||
    new Set(targets.map((t) => t.itemId)).size !== targets.length ||
    new Set(targets.map((t) => t.path)).size !== targets.length ||
    targets.some(
      (t) =>
        !uuid(t.scanId) ||
        !uuid(t.itemId) ||
        t.scanId !== targets[0].scanId ||
        t.domain !== targets[0].domain ||
        !['personalFiles', 'duplicateFiles', 'softwareContent'].includes(t.domain) ||
        !/^\/.+\.(mp4|mov|m4v)$/i.test(t.path) ||
        t.path.split('/').some((p) => p === '.' || p === '..') ||
        !Number.isSafeInteger(t.size) ||
        t.size <= 0,
    )
  )
    fail()
}
export function readVideoComparison(
  raw: unknown,
  comparisonId: string,
  targets: VideoTarget[],
): VideoReport[] {
  videoTargets(targets)
  if (!uuid(comparisonId)) fail()
  const value = raw as { comparisonId?: string; scanId?: string; items?: unknown[] }
  if (
    !value ||
    value.comparisonId !== comparisonId ||
    value.scanId !== targets[0].scanId ||
    !Array.isArray(value.items) ||
    value.items.length !== targets.length
  )
    fail()
  const reports = value.items.map((item, index) => readFilePreview(item, targets[index], 1))
  if (
    reports.some((r) => r.content.kind !== 'media' || r.content.mediaKind !== 'video') ||
    new Set(reports.map((r) => (r.content as VideoContent).leaseId)).size !== targets.length
  )
    fail()
  return reports as VideoReport[]
}
export const videoComparison: VideoComparisonTransport = {
  prepare: (comparisonId, targets) => {
    videoTargets(targets)
    return invoke('prepare_media_comparison', {
      comparisonId,
      domain: targets[0].domain,
      scanId: targets[0].scanId,
      itemIds: targets.map((t) => t.itemId),
    })
  },
  release: (comparisonId) => invoke('release_media_comparison', { comparisonId }),
}
export const comparableVideo = (path: string) => /\.(mp4|m4v|mov)$/i.test(path)
