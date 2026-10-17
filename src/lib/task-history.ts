import type { TaskKind, TaskRecord } from '../types'
import { kindNames, stateNames } from './format'

export const taskStates = [
  'running',
  'queued',
  'succeeded',
  'failed',
  'cancelled',
  'interrupted',
] as const
export function taskKindName(kind: string): string {
  return Object.hasOwn(kindNames, kind) ? kindNames[kind as TaskKind] : `未知检查（${kind}）`
}
export function taskStateName(state: string): string {
  return Object.hasOwn(stateNames, state) ? stateNames[state] : `未知状态（${state}）`
}
export function canFollowTask(task: TaskRecord): boolean {
  return task.state === 'running' && Object.hasOwn(kindNames, task.kind)
}
export function taskTime(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 8.64e15
    ? value
    : null
}
export function taskTimeLabel(value: unknown): string {
  const time = taskTime(value)
  return time == null ? '未知' : new Date(time).toLocaleString('zh-CN', { hour12: false })
}
export function taskDuration(task: TaskRecord): string {
  const start = taskTime(task.createdAt)
  const finish = taskTime(task.finishedAt)
  if (start == null || finish == null || finish < start) return '未知'
  return `${((finish - start) / 1000).toLocaleString('zh-CN', { maximumFractionDigits: 3 })} 秒`
}
export function filterTasks(records: TaskRecord[], query: string, state: string): TaskRecord[] {
  const text = query.trim().toLocaleLowerCase('zh-CN')
  return records.filter((task) => {
    const stateMatches =
      state === 'all' ||
      (state === 'unknown' ? !Object.hasOwn(stateNames, task.state) : task.state === state)
    return (
      stateMatches &&
      [
        task.id,
        task.kind,
        taskKindName(task.kind),
        task.state,
        taskStateName(task.state),
        task.path ?? '',
        task.message ?? '',
      ]
        .join('\n')
        .toLocaleLowerCase('zh-CN')
        .includes(text)
    )
  })
}
