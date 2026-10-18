import type { ApplicationInspection } from '../types'

export function parseApplicationInspection(
  value: unknown,
  expectedId: string,
): ApplicationInspection {
  const fail = () => {
    throw new Error('应用状态结果无法识别，请重新检查。')
  }
  if (!value || typeof value !== 'object') return fail()
  const data = value as ApplicationInspection
  const text = (value: unknown, limit: number) =>
    typeof value === 'string' &&
    value.length > 0 &&
    value.length <= limit &&
    !/[\u0000-\u001f\u007f]/.test(value)
  if (
    data.protocol !== 2 ||
    !Number.isSafeInteger(data.checkedAt) ||
    data.checkedAt <= 0 ||
    data.checkedAt > 8.64e15 ||
    !['matches', 'different', 'unreported', 'unavailable'].includes(data.identity) ||
    ![data.version, data.build].every((v) => v === null || text(v, 128)) ||
    !(
      data.bundleId === null ||
      (text(data.bundleId, 255) && /^[A-Za-z0-9_.-]+$/.test(data.bundleId))
    )
  )
    return fail()
  const protection = data.protection
  if (
    !protection ||
    !Array.isArray(protection.reasons) ||
    protection.reasons.length > 3 ||
    new Set(protection.reasons).size !== protection.reasons.length ||
    !protection.reasons.every((reason) =>
      ['systemLocation', 'currentApplication', 'clientDirectory'].includes(reason),
    ) ||
    !Array.isArray(protection.matchedPaths) ||
    protection.matchedPaths.length > 100 ||
    new Set(protection.matchedPaths).size !== protection.matchedPaths.length ||
    !protection.matchedPaths.every(
      (path) => text(path, 4096) && path.startsWith('/') && !path.split('/').includes('..'),
    ) ||
    protection.reasons.includes('clientDirectory') !== protection.matchedPaths.length > 0
  )
    return fail()
  if (
    (data.identity === 'matches' && (!expectedId || data.bundleId !== expectedId)) ||
    (data.identity === 'different' &&
      (!expectedId || !data.bundleId || data.bundleId === expectedId)) ||
    (data.identity === 'unreported' && (expectedId || !data.bundleId)) ||
    (data.identity === 'unavailable' && data.bundleId !== null)
  )
    return fail()
  const running = data.running
  if (
    ['different', 'unavailable'].includes(data.identity) &&
    (running?.code !== 'unknown' || running?.otherCopies !== 0)
  )
    return fail()
  if (
    !running ||
    !['observed', 'notObserved', 'unknown'].includes(running.code) ||
    !Array.isArray(running.pids) ||
    running.pids.length > 32 ||
    new Set(running.pids).size !== running.pids.length ||
    !running.pids.every((pid) => Number.isSafeInteger(pid) && pid > 0 && pid <= 2147483647) ||
    !Number.isSafeInteger(running.otherCopies) ||
    running.otherCopies < 0 ||
    running.otherCopies > 32 ||
    !text(running.reason, 512) ||
    (running.code === 'observed'
      ? !running.pids.length || !data.bundleId || data.identity === 'different'
      : running.pids.length > 0)
  )
    return fail()
  return data
}
