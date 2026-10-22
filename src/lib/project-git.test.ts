import { expect, it } from 'vitest'
import { projectCatalog } from './project-artifacts'
import { readProjectGit, projectGitReasons, type ProjectGitReport } from './project-git'
it('binds positive index records to the selected scope and repository without granting execution', () => {
  const now = Date.now()
  const r: ProjectGitReport = {
    protocol: 1,
    revision: projectCatalog.revision,
    root: '/repo',
    path: '/repo/target[x]',
    checkedAt: now,
    state: 'recorded',
    reason: null,
    repository: '/repo',
    records: 1,
    example: 'TARGET[x]/source.rs',
    canExecute: false,
  }
  const read = (value: unknown) => readProjectGit(value, r.root, r.path, now, now)
  expect(read(r)).toEqual(r)
  expect(read({ ...r, example: 'target[x]' }).state).toBe('recorded')
  for (const change of [
    { protocol: 2 },
    { revision: 'other' },
    { root: '/other' },
    { path: '/repo/other' },
    { checkedAt: now - 6000 },
    { checkedAt: now + 6000 },
    { canExecute: true },
    { state: 'safe' },
    { reason: 'notListed' },
    { repository: '/outside' },
    { repository: '/repo/elsewhere' },
    { records: 0 },
    { records: 20001 },
    { records: NaN },
    { example: 'target[x]-other/source.rs' },
    { example: 'targetX/source.rs' },
    { example: '/repo/target[x]/source.rs' },
    { example: 'target[x]/../secret' },
    { example: 'target[x]/a\u0000' },
    { example: 'target[x]/.git/index' },
    { example: null },
  ])
    expect(() => read({ ...r, ...change })).toThrow()
})
it('unknown observations carry a reason with no positive repository, count or filename', () => {
  const now = Date.now()
  const r = {
    protocol: 1,
    revision: projectCatalog.revision,
    root: '/repo',
    path: '/repo/target',
    checkedAt: now,
    state: 'unknown',
    reason: 'notListed',
    repository: null,
    records: null,
    example: null,
    canExecute: false,
  }
  for (const reason of Object.keys(projectGitReasons))
    expect(readProjectGit({ ...r, reason }, r.root, r.path, now, now).state).toBe('unknown')
  for (const change of [
    { reason: null },
    { reason: 'clear' },
    { repository: '/repo' },
    { records: 0 },
    { example: 'target/a' },
  ])
    expect(() => readProjectGit({ ...r, ...change }, r.root, r.path, now, now)).toThrow()
})
