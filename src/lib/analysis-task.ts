import type { AnalysisHistoryTarget } from './analysis-navigation'
import { TaskResultError } from './task-result'

export interface AnalysisTask {
  id: string | null
  path: string | null
  historyTarget?: AnalysisHistoryTarget
  state:
    | 'starting'
    | 'running'
    | 'cancelling'
    | 'cancelled'
    | 'failed'
    | 'interrupted'
    | 'unavailable'
    | 'unconfirmed'
  detail?: string
  diagnostic?: string
  cancelError?: string
}

export function analysisTaskActive(task: AnalysisTask | null): boolean {
  return !!task && ['starting', 'running', 'cancelling'].includes(task.state)
}

export function failedAnalysisTask(task: AnalysisTask, error: unknown): AnalysisTask {
  let state: AnalysisTask['state'] = 'unconfirmed'
  if (error instanceof TaskResultError && error.id === task.id && error.kind === 'analyze') {
    if (error.state === 'cancelled') state = 'cancelled'
    if (error.state === 'failed') state = 'failed'
    if (error.state === 'interrupted') state = 'interrupted'
    if (error.state === 'succeeded') state = 'unavailable'
  }
  const detail = String(error).replace(/^(?:TaskResultError|Error): /, '')
  const diagnostic = detail.startsWith('空间分析结果格式异常，未更新。')
    ? /诊断：(summary|overview|total_files|entries|entry|large_files)$/.exec(detail)
    : null
  return {
    ...task,
    state,
    detail: diagnostic ? detail.slice(0, diagnostic.index).trim() : detail,
    diagnostic: diagnostic ? `不匹配字段：${diagnostic[1]}` : undefined,
    cancelError: undefined,
  }
}
