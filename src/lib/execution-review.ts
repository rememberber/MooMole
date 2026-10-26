// A review response may add a timestamp, never replace the operation's facts.
function canonical(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`
  if (value && typeof value === 'object')
    return `{${Object.entries(value)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([key, item]) => `${JSON.stringify(key)}:${canonical(item)}`)
      .join(',')}}`
  return JSON.stringify(value)
}

export function reviewedResult<T extends { reviewedAt: number | null }>(before: T, next: T): T {
  const { reviewedAt: _before, ...facts } = before
  const { reviewedAt, ...nextFacts } = next
  if (
    reviewedAt === null ||
    !Number.isSafeInteger(reviewedAt) ||
    reviewedAt < 0 ||
    canonical(facts) !== canonical(nextFacts)
  )
    throw new Error('核查响应与原记录不一致，保留原结果；请重新读取，不会重新执行。')
  return next
}
