export interface ProjectActivity {
  windowDays: 7
  referenceAt: number
  state: 'recent' | 'unknown'
  reason: 'notObserved' | 'incomplete' | 'unavailable' | null
  evidence: { kind: 'directory' | 'file'; path: string; modifiedAt: number } | null
}
export function readProjectActivity(
  value: unknown,
  path: string,
  checkedAt: number,
  partial: boolean,
): ProjectActivity {
  const r = value as ProjectActivity | null
  const invalid = () => {
    throw new Error('近期修改观察无法确认，请重新测量。')
  }
  if (
    !r ||
    r.windowDays !== 7 ||
    !Number.isSafeInteger(r.referenceAt) ||
    r.referenceAt < 0 ||
    r.referenceAt > checkedAt ||
    checkedAt - r.referenceAt > 6000 ||
    !['recent', 'unknown'].includes(r.state)
  )
    return invalid()
  if (r.state === 'recent') {
    const e = r.evidence
    if (
      r.reason !== null ||
      !e ||
      !['directory', 'file'].includes(e.kind) ||
      !Number.isSafeInteger(e.modifiedAt) ||
      e.modifiedAt < 0 ||
      e.modifiedAt % 1000 !== 0 ||
      e.modifiedAt > r.referenceAt ||
      r.referenceAt - e.modifiedAt >= 7 * 86400000 ||
      typeof e.path !== 'string' ||
      new TextEncoder().encode(e.path).length > 4096 ||
      /[\u0000-\u001f\u007f-\u009f]/.test(e.path) ||
      (e.kind === 'directory' ? e.path !== path : !e.path.startsWith(`${path}/`))
    )
      return invalid()
    if (e.kind === 'file') {
      const parts = e.path.slice(path.length + 1).split('/')
      if (parts.length > 33 || parts.some((p) => ['', '.', '..'].includes(p))) return invalid()
    }
  } else if (
    r.evidence !== null ||
    !['notObserved', 'incomplete', 'unavailable'].includes(r.reason ?? '') ||
    (r.reason === 'notObserved' && partial) ||
    (r.reason === 'incomplete' && !partial)
  )
    return invalid()
  return r
}
