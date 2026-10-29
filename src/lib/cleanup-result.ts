import type { CleanupSpaceSample, ExecutionDetail } from '../types'

const nonnegative = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
function sample(value: unknown): value is CleanupSpaceSample {
  if (!value || typeof value !== 'object') return false
  const v = value as CleanupSpaceSample
  return (
    nonnegative(v.measuredAt) &&
    typeof v.volumeId === 'string' &&
    /^[a-f\d:]{1,128}$/i.test(v.volumeId) &&
    nonnegative(v.totalBytes) &&
    v.totalBytes > 0 &&
    nonnegative(v.availableBytes) &&
    v.availableBytes <= v.totalBytes
  )
}
export function cleanupResultSizes(detail: ExecutionDetail) {
  function total(items: ExecutionDetail['items']) {
    let knownBytes = 0,
      unknownCount = 0
    for (const item of items) {
      if (nonnegative(item.scanSizeBytes) && Number.isSafeInteger(knownBytes + item.scanSizeBytes))
        knownBytes += item.scanSizeBytes
      else unknownCount++
    }
    return { knownBytes, unknownCount }
  }
  return {
    selected: total(detail.items),
    moved: total(detail.items.filter((item) => item.state === 'moved' && item.receipt)),
  }
}
export function cleanupSpaceChange(
  detail: ExecutionDetail,
):
  | { code: 'observed'; before: CleanupSpaceSample; after: CleanupSpaceSample; delta: number }
  | { code: 'unavailable' | 'volumeChanged' | 'timeChanged' } {
  const observation = detail.spaceObservation
  if (observation?.version !== 1 || !sample(observation.before) || !sample(observation.after))
    return { code: 'unavailable' }
  const { before, after } = observation
  if (before.volumeId !== after.volumeId || before.totalBytes !== after.totalBytes)
    return { code: 'volumeChanged' }
  if (
    !nonnegative(detail.createdAt) ||
    !nonnegative(detail.finishedAt) ||
    before.measuredAt < detail.createdAt ||
    before.measuredAt > detail.finishedAt ||
    after.measuredAt < detail.finishedAt ||
    after.measuredAt < before.measuredAt ||
    detail.finishedAt < detail.createdAt
  )
    return { code: 'timeChanged' }
  return { code: 'observed', before, after, delta: after.availableBytes - before.availableBytes }
}
