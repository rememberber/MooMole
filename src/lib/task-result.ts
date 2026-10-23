import type { TaskEvent, TaskKind, TaskResults } from '../types'
import { decodeAnalysis } from './analysis-protocol'

export class TaskResultError extends Error {
  readonly id: string
  readonly kind: TaskKind
  readonly state: string

  constructor(event: TaskEvent) {
    super(event.error ?? event.resultUnavailable ?? '任务未完成。')
    this.name = 'TaskResultError'
    this.id = event.id
    this.kind = event.kind
    this.state = event.state
  }
}

export function taskResult<K extends TaskKind>(event: TaskEvent): TaskResults[K] {
  if (event.error || event.state !== 'succeeded') throw new TaskResultError(event)
  if (event.result == null)
    throw new TaskResultError({
      ...event,
      resultUnavailable: event.resultUnavailable ?? '结果未保留，请重新检查。',
    })
  if (event.kind === 'analyze') {
    try {
      return decodeAnalysis(event.result) as TaskResults[K]
    } catch (error) {
      throw new TaskResultError({
        ...event,
        error: error instanceof Error ? error.message : String(error),
      })
    }
  }
  return event.result as TaskResults[K]
}
