import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { AiDeepScan } from '../../../src/components/AiDeepScan'
import type { AiSnapshot } from '../../../src/lib/ai-scan'
import type { AiScanController } from '../../../src/lib/use-ai-scan'

export function mount() {
  const host = document.createElement('div')
  host.id = 'ai-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:10000;overflow:auto;background:var(--background);padding:20px'
  document.body.append(host)
  function Fixture() {
    const [protectedPaths, setProtectedPaths] = useState<string[]>([])
    const [consent, setConsent] = useState(false)
    const [agentId, setAgentId] = useState('codex')
    const [root, setRoot] = useState('/Users/demo/Downloads')
    const [snapshot, setSnapshot] = useState<AiSnapshot | null>({
      id: 'request',
      agentId: 'codex',
      root,
      state: 'succeeded',
      stage: '分析完成，等待审阅',
      startedAt: Date.now(),
      finishedAt: Date.now(),
      scanId: 'scan',
      expiresAt: Date.now() + 10_000,
      evidence: [
        {
          id: 'mole',
          source: 'mole',
          path: '/Users/demo/Library/Logs/a.log',
          size: 2048,
          modifiedAt: null,
          isDirectory: false,
          category: 'logs',
          candidateId: 'scan:0',
        },
        {
          id: 'extra',
          source: 'metadata',
          path: '/Users/demo/Downloads/archive.zip',
          size: 4096,
          modifiedAt: 1,
          isDirectory: false,
          category: 'file',
          candidateId: null,
        },
      ],
      report: {
        summary: '发现 1 项可审阅日志和 1 项需要核实的归档。',
        findings: [
          {
            evidenceId: 'mole',
            recommendation: 'review',
            reason: '诊断日志，可审阅。',
            impact: '可能丢失排障信息。',
          },
          {
            evidenceId: 'extra',
            recommendation: 'investigate',
            reason: '无法确认备份和归属。',
            impact: '可能包含个人资料。',
          },
        ],
        limitations: ['元数据不能证明内容重复。'],
      },
      limitations: ['仅扫描 3 层。'],
      error: null,
    })
    const noop = async () => {}
    const controller: AiScanController = {
      agents: [
        {
          id: 'codex',
          name: 'Codex CLI',
          path: '/test/codex',
          supported: true,
          note: '检测样例，登录待验证',
        },
        {
          id: 'gemini',
          name: 'Gemini CLI',
          path: '/test/gemini',
          supported: false,
          note: '尚未适配',
        },
      ],
      agentId,
      setAgentId,
      root,
      setRoot,
      consent,
      setConsent,
      snapshot,
      error: '',
      detecting: false,
      starting: false,
      stopping: false,
      running: false,
      ready: true,
      detect: noop,
      start: noop,
      cancel: noop,
    }
    return (
      <>
        <button onClick={() => setProtectedPaths(['/Users/demo/Library/Logs'])}>新增保护</button>
        <button onClick={() => setSnapshot((s) => (s ? { ...s, expiresAt: Date.now() - 1 } : s))}>
          使清单过期
        </button>
        <AiDeepScan
          controller={controller}
          home="/Users/demo"
          protectedPaths={protectedPaths}
          enabled
          onReveal={() => {}}
          onCreatePlan={async (scanId, ids) => {
            sessionStorage.setItem('ai-plan', JSON.stringify({ scanId, ids }))
          }}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
