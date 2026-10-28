import { pathOverlaps } from './clean-preview'

export interface AiAgent {
  id: string
  name: string
  path: string | null
  supported: boolean
  note: string
}
export interface AiEvidence {
  id: string
  source: 'mole' | 'metadata'
  path: string
  size: number | null
  modifiedAt: number | null
  isDirectory: boolean
  category: string
  candidateId: string | null
}
export interface AiFinding {
  evidenceId: string
  recommendation: 'review' | 'keep' | 'investigate'
  reason: string
  impact: string
}
export interface AiSnapshot {
  id: string
  agentId: string
  root: string
  state: 'running' | 'succeeded' | 'failed' | 'cancelled'
  stage: string
  startedAt: number
  finishedAt: number | null
  scanId: string | null
  expiresAt: number | null
  evidence: AiEvidence[]
  limitations: string[]
  report: { summary: string; findings: AiFinding[]; limitations: string[] } | null
  error: string | null
}
export interface AiTransport {
  detect: () => Promise<AiAgent[]>
  read: () => Promise<AiSnapshot | null>
  start: (agentId: string, root: string, metadataConsent: boolean) => Promise<AiSnapshot>
  cancel: (requestId: string) => Promise<void>
}
export function aiReviewable(
  snapshot: AiSnapshot,
  protectedPaths: string[],
  now: number,
): AiEvidence[] {
  if (
    snapshot.state !== 'succeeded' ||
    !snapshot.scanId ||
    snapshot.expiresAt == null ||
    now >= snapshot.expiresAt
  )
    return []
  const ids = new Set(
    snapshot.report?.findings.filter((f) => f.recommendation === 'review').map((f) => f.evidenceId),
  )
  return snapshot.evidence.filter(
    (e) =>
      e.source === 'mole' &&
      e.candidateId != null &&
      ids.has(e.id) &&
      !protectedPaths.some((p) => pathOverlaps(p, e.path)),
  )
}
export const aiRecommendation = { review: '建议审阅', keep: '建议保留', investigate: '需要核实' }
