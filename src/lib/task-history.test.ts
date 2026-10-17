import { describe, expect, it } from 'vitest'
import type { TaskRecord } from '../types'
import {
  canFollowTask,
  filterTasks,
  taskDuration,
  taskKindName,
  taskStateName,
  taskTime,
  taskTimeLabel,
} from './task-history'

const failed: TaskRecord = {
  id: 'Literal-ID',
  kind: 'analyze',
  state: 'failed',
  path: '/中文 空格/[*] $(literal)\n目录',
  createdAt: 1000,
  finishedAt: 2250,
  message: '权限拒绝；文本提及成功不改变失败状态',
}
const records: TaskRecord[] = [
  failed,
  { ...failed, id: 'running-id', state: 'running', path: null, finishedAt: null, message: null },
  { ...failed, id: 'future', kind: '__proto__' as TaskRecord['kind'], state: 'future-phase' },
]
describe('client task history', () => {
  it('searches literal paths, diagnostics, IDs and localized labels within the returned records without mutating them', () => {
    expect(filterTasks(records, ' literal-id ', 'all')).toEqual([failed])
    expect(filterTasks(records, '[*] $(literal)', 'failed')).toEqual([failed])
    expect(filterTasks(records, '权限拒绝', 'failed')).toEqual([failed])
    expect(filterTasks(records, '空间分析', 'running')).toEqual([records[1]])
    expect(filterTasks(records, '', 'all')).toEqual(records)
    expect(filterTasks(records, 'none', 'all')).toEqual([])
    expect(records.map((r) => r.id)).toEqual(['Literal-ID', 'running-id', 'future'])
  })
  it('keeps unknown kinds and states explicit and only offers observation for supported running records', () => {
    expect(taskKindName('__proto__')).toBe('未知检查（__proto__）')
    expect(taskStateName('toString')).toBe('未知状态（toString）')
    expect(filterTasks(records, '', 'unknown')).toEqual([records[2]])
    expect(taskStateName(failed.state)).toBe('未完成')
    expect(canFollowTask(failed)).toBe(false)
    expect(canFollowTask(records[1])).toBe(true)
    expect(canFollowTask({ ...records[2], state: 'running' })).toBe(false)
    expect(canFollowTask({ ...records[1], state: 'queued' })).toBe(false)
  })
  it('retains zero durations and rejects invalid, missing or backwards record times', () => {
    expect(taskDuration(failed)).toBe('1.25 秒')
    expect(taskDuration({ ...failed, finishedAt: failed.createdAt })).toBe('0 秒')
    for (const finishedAt of [null, -1, NaN, Infinity, 999, 9e15])
      expect(taskDuration({ ...failed, finishedAt })).toBe('未知')
    for (const at of [null, undefined, -1, NaN, Infinity, '1000', 9e15]) {
      expect(taskTime(at)).toBeNull()
      expect(taskTimeLabel(at)).toBe('未知')
    }
    expect(taskTime(0)).toBe(0)
  })
})
