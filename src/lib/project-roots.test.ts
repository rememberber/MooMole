import { expect, it } from 'vitest'
import { projectCatalog } from './project-artifacts'
import { readProjectRoots } from './project-roots'
it('binds root choices to the account, pinned revision, fresh time and non-executable protocol', () => {
  const now = Date.now()
  const report = {
    protocol: 1 as const,
    revision: projectCatalog.revision,
    source: '/Users/demo/.config/mole/purge_paths',
    checkedAt: now,
    state: 'ready' as const,
    paths: ['/Users/demo/Projects'],
    canExecute: false as const,
  }
  expect(readProjectRoots(report, '/Users/demo', now, now)).toEqual(report)
  for (const update of [
    { protocol: 2 },
    { revision: 'other' },
    { source: '/Users/other/.config/mole/purge_paths' },
    { checkedAt: now - 6000 },
    { checkedAt: now + 6000 },
    { checkedAt: NaN },
    { canExecute: true },
    { state: 'discovered' },
    { state: 'missing' },
    { paths: ['/tmp/x', '/tmp/x'] },
    { paths: ['/tmp/../x'] },
    { paths: ['/tmp//x'] },
    { paths: ['/tmp/x/'] },
    { paths: ['/'] },
    { paths: ['~/Code'] },
    { paths: ['/tmp/a\u0000'] },
    { paths: ['/tmp/a\u0085'] },
    { paths: ['/tmp/' + '中'.repeat(1500)] },
    { paths: Array.from({ length: 65 }, (_, i) => `/tmp/x${i}`) },
  ])
    expect(() => readProjectRoots({ ...report, ...update }, '/Users/demo', now, now)).toThrow()
  expect(
    readProjectRoots({ ...report, state: 'missing', paths: [] }, '/Users/demo', now, now).state,
  ).toBe('missing')
  expect(readProjectRoots({ ...report, paths: [] }, '/Users/demo', now, now).paths).toEqual([])
})
