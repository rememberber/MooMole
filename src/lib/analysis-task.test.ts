import { describe, expect, it } from 'vitest'
import { analysisTaskActive, failedAnalysisTask, type AnalysisTask } from './analysis-task'
import { taskResult, TaskResultError } from './task-result'
import type { TaskEvent } from '../types'

const task: AnalysisTask = {
  id: 'analysis-id',
  path: '/literal/[*] $(x)',
  state: 'running',
  historyTarget: { index: 0, path: '/literal/[*] $(x)' },
}
const event = (state: string, error: string | null = null): TaskEvent => ({
  id: task.id!,
  kind: 'analyze',
  state,
  error,
  result: null,
})

describe('structured analysis task outcomes', () => {
  it('keeps native terminal state independent of error prose and preserves the original retry target', () => {
    const failed = failedAnalysisTask(
      task,
      new TaskResultError(event('failed', '文本包含已取消，但结构化状态是失败')),
    )
    expect(failed.state).toBe('failed')
    expect(failed.path).toBe(task.path)
    expect(failed.historyTarget).toEqual(task.historyTarget)
    expect(
      failedAnalysisTask(task, new TaskResultError(event('cancelled', 'arbitrary diagnostic')))
        .state,
    ).toBe('cancelled')
    expect(failedAnalysisTask(task, new TaskResultError(event('interrupted'))).state).toBe(
      'interrupted',
    )
    expect(analysisTaskActive({ ...task, state: 'cancelling' })).toBe(true)
    expect(analysisTaskActive(failed)).toBe(false)
  })
  it('never turns loss of observation, start failure or mismatched outcomes into confirmed cancellation', () => {
    for (const error of [
      new Error('任务已取消'),
      new TaskResultError({ ...event('cancelled'), id: 'other' }),
      new TaskResultError({ ...event('cancelled'), kind: 'status' }),
    ])
      expect(failedAnalysisTask(task, error).state).toBe('unconfirmed')
    expect(
      failedAnalysisTask({ ...task, id: null, state: 'starting' }, new Error('connection lost'))
        .state,
    ).toBe('unconfirmed')
    expect(analysisTaskActive(null)).toBe(false)
  })
  it('returns only successful retained results and carries exact terminal metadata on failure', () => {
    const result = { path: '/actual', entries: [], scan_status: 'partial', total_size: 0 }
    expect(taskResult({ ...event('succeeded'), result })).toBe(result)
    for (const state of ['failed', 'cancelled', 'interrupted']) {
      try {
        taskResult(event(state, 'native reason'))
        throw new Error('expected failure')
      } catch (error) {
        expect(error).toBeInstanceOf(TaskResultError)
        expect(error).toMatchObject({
          id: task.id,
          kind: 'analyze',
          state,
          message: 'native reason',
        })
      }
    }
    try {
      taskResult({ ...event('succeeded'), resultUnavailable: 'budget exceeded' })
      throw new Error('expected failure')
    } catch (error) {
      expect(error).toBeInstanceOf(TaskResultError)
      expect(failedAnalysisTask(task, error)).toMatchObject({
        state: 'unavailable',
        detail: 'budget exceeded',
      })
    }
  })
})
