import { describe, expect, it } from 'vitest'
import { aiReviewable, type AiSnapshot } from './ai-scan'
const snapshot: AiSnapshot = {
  id: 'request',
  agentId: 'codex',
  root: '/Users/demo/Downloads',
  state: 'succeeded',
  stage: '完成',
  startedAt: 1,
  finishedAt: 2,
  scanId: 'scan',
  expiresAt: 100,
  evidence: [
    {
      id: 'mole',
      source: 'mole',
      path: '/Users/demo/Library/Logs/a.log',
      size: 100,
      modifiedAt: null,
      isDirectory: false,
      category: 'logs',
      candidateId: 'scan:0',
    },
    {
      id: 'extra',
      source: 'metadata',
      path: '/Users/demo/Downloads/b.zip',
      size: null,
      modifiedAt: 1,
      isDirectory: false,
      category: 'file',
      candidateId: null,
    },
    {
      id: 'keep',
      source: 'mole',
      path: '/Users/demo/Library/Logs/b.log',
      size: 100,
      modifiedAt: null,
      isDirectory: false,
      category: 'logs',
      candidateId: 'scan:1',
    },
  ],
  report: {
    summary: '建议',
    findings: [
      { evidenceId: 'mole', recommendation: 'review', reason: '日志', impact: '丢失诊断' },
      { evidenceId: 'extra', recommendation: 'review', reason: '无执行来源', impact: '未知' },
      { evidenceId: 'keep', recommendation: 'keep', reason: '保留', impact: '未知' },
    ],
    limitations: [],
  },
  limitations: [],
  error: null,
}
describe('AI review admission', () => {
  it('only offers explicit original Mole recommendations and keeps defaults unselected', () => {
    expect(aiReviewable(snapshot, [], 3).map((e) => e.candidateId)).toEqual(['scan:0'])
  })
  it('revokes review on expiry, new protections, missing scan or unfinished report', () => {
    expect(aiReviewable(snapshot, [], 100)).toEqual([])
    expect(aiReviewable(snapshot, ['/Users/demo/Library'], 3)).toEqual([])
    expect(aiReviewable({ ...snapshot, state: 'cancelled' }, [], 3)).toEqual([])
    expect(aiReviewable({ ...snapshot, scanId: null }, [], 3)).toEqual([])
  })
})
