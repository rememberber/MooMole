import { expect, it } from 'vitest'
import { recentAnalysisPaths } from './recent-analysis'
import type { TaskRecord } from '../types'

const task = (path: string | null, overrides: Partial<TaskRecord> = {}): TaskRecord => ({
  id: 'fixture',
  kind: 'analyze',
  state: 'succeeded',
  path,
  createdAt: 1,
  finishedAt: 2,
  message: null,
  ...overrides,
})

it('keeps the newest distinct successful analysis paths in existing history order', () => {
  const paths = [
    '/latest',
    '/older',
    '/latest',
    '/Older',
    ...Array.from({ length: 12 }, (_, i) => `/dir${i}`),
  ]
  const records = paths.map((path) => task(path))
  expect(recentAnalysisPaths(records)).toEqual([
    '/latest',
    '/older',
    '/Older',
    '/dir0',
    '/dir1',
    '/dir2',
    '/dir3',
    '/dir4',
  ])
  expect(records.map((record) => record.path)).toEqual(paths)
  expect(recentAnalysisPaths([])).toEqual([])
})

it('ignores incomplete, failed and unrelated tasks and invalid paths without probing targets', () => {
  const records = [
    ...['queued', 'running', 'failed', 'cancelled', 'interrupted', 'future'].map((state) =>
      task('/invalid-state', { state }),
    ),
    task('/unrelated', { kind: 'apps' }),
    ...[
      null,
      '',
      'relative',
      '~/Code',
      '/tmp/../x',
      '/tmp/./x',
      '/tmp//x',
      '/tmp/',
      '/tmp\0',
      '/' + 'x'.repeat(4096),
    ].map((path) => task(path)),
    task('/'),
    task('/missing-volume/project'),
    task('/中文 [*] $(literal)'),
    task('/valid-but-multiline\nname'),
    task('/valid-but-carriage\rname'),
  ]
  expect(recentAnalysisPaths(records)).toEqual([
    '/',
    '/missing-volume/project',
    '/中文 [*] $(literal)',
  ])
})

it('respects the existing 200-record source bound and counts UTF-8 bytes', () => {
  const records = Array.from({ length: 200 }, () => task('/unused', { state: 'failed' }))
  records.push(task('/outside-history'))
  expect(recentAnalysisPaths(records)).toEqual([])
  expect(
    recentAnalysisPaths([task('/' + '中'.repeat(1365)), task('/' + '中'.repeat(1366))]),
  ).toEqual(['/' + '中'.repeat(1365)])
})
