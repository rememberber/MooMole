export interface CleanupVisibilitySnapshot {
  version: 1
  readOnly: true
  requestId: string | null
  state: 'idle' | 'running' | 'cancelling' | 'finished'
  startedAt: number | null
  finishedAt: number | null
  diagnostic?: string | null
  observation:
    'completeViewObserved' | 'incompleteOrFailed' | 'cancelled' | 'expired' | 'unsupported' | null
}

export function readCleanupVisibility(value: unknown): CleanupVisibilitySnapshot {
  const snapshot = value as CleanupVisibilitySnapshot | null
  const validTime = (time: unknown) =>
    typeof time === 'number' &&
    Number.isSafeInteger(time) &&
    time > 0 &&
    time <= 8_640_000_000_000_000
  const invalid = () => {
    throw new Error('权限检查状态无法确认，请重新读取。')
  }
  if (
    !snapshot ||
    (snapshot.diagnostic != null &&
      (typeof snapshot.diagnostic !== 'string' ||
        snapshot.diagnostic.length > 1000 ||
        snapshot.diagnostic.includes('\0'))) ||
    snapshot.version !== 1 ||
    snapshot.readOnly !== true ||
    !['idle', 'running', 'cancelling', 'finished'].includes(snapshot.state)
  )
    return invalid()
  if (snapshot.state === 'idle') {
    if (
      snapshot.requestId !== null ||
      snapshot.startedAt !== null ||
      snapshot.finishedAt !== null ||
      snapshot.observation !== null
    )
      return invalid()
  } else {
    if (
      typeof snapshot.requestId !== 'string' ||
      !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
        snapshot.requestId,
      ) ||
      !validTime(snapshot.startedAt)
    )
      return invalid()
    if (snapshot.state === 'finished') {
      if (
        !validTime(snapshot.finishedAt) ||
        ![
          'completeViewObserved',
          'incompleteOrFailed',
          'cancelled',
          'expired',
          'unsupported',
        ].includes(snapshot.observation ?? '')
      )
        return invalid()
    } else if (snapshot.finishedAt !== null || snapshot.observation !== null) return invalid()
  }
  return snapshot
}

export function cleanupVisibilityText(snapshot: CleanupVisibilitySnapshot): string {
  if (snapshot.state === 'idle') return '尚未检查系统进程可见性。'
  if (snapshot.state === 'running') return '等待系统确认或读取检查结果…'
  if (snapshot.state === 'cancelling') return '正在停止等待；若系统授权窗口仍在，请在该窗口取消。'
  switch (snapshot.observation) {
    case 'completeViewObserved':
      return '本次已读到所需的系统进程字段。实际清理仍须对所选对象逐项复核。'
    case 'cancelled':
      return '本次检查已取消。若系统授权窗口仍在，请在该窗口取消。'
    case 'expired':
      return '本次检查等待过久，结果已过期；需要时重新检查。'
    case 'unsupported':
      return '系统检查工具不可用，无法使用这条授权检查路线。'
    default:
      return '本次未能证明完整进程视图。无法确认占用的清理项目仍应保留。'
  }
}
