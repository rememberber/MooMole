import { expect, it } from 'vitest'
import {
  projectArtifactsReport,
  projectMeasurement,
} from '../../tests/e2e/fixtures/project-artifacts'
import {
  readProjectArtifacts,
  readProjectMeasurement,
  projectMeasurementReason,
} from './project-artifacts'
it('binds project name discovery to the selected scope, metadata context and non-executable state', () => {
  const report = projectArtifactsReport()
  expect(readProjectArtifacts(report, report.root, report.checkedAt, report.checkedAt)).toEqual(
    report,
  )
  for (const mutate of [
    (r: typeof report) => {
      r.protocol = 1 as 2
    },
    (r: typeof report) => {
      r.revision = 'other'
    },
    (r: typeof report) => {
      r.root = '/fixture/other'
    },
    (r: typeof report) => {
      r.checkedAt -= 6000
    },
    (r: typeof report) => {
      r.canExecute = true as false
    },
    (r: typeof report) => {
      r.visited = -1
    },
    (r: typeof report) => {
      r.skipped.links = 1
    },
    (r: typeof report) => {
      r.items[0].path = '/fixture/projects-old/web/node_modules'
    },
    (r: typeof report) => {
      r.items[0].name = 'source'
    },
    (r: typeof report) => {
      r.items[0].canExecute = true as false
    },
    (r: typeof report) => {
      r.items[0].measurement = 'measured' as 'notMeasured'
    },
    (r: typeof report) => {
      r.items[0].projectPath = '/other'
    },
    (r: typeof report) => {
      r.items.push(r.items[0])
    },
    (r: typeof report) => {
      r.items.push({
        ...r.items[0],
        path: `${r.items[0].path}/node_modules`,
        projectPath: r.items[0].path,
      })
    },
    (r: typeof report) => {
      r.items[2].context = 'nodeContext'
    },
  ]) {
    const value = structuredClone(report)
    mutate(value)
    expect(() =>
      readProjectArtifacts(value, report.root, report.checkedAt, report.checkedAt),
    ).toThrow()
  }
})
it('retains partial and empty inventories without implying full discovery or zero sizes', () => {
  const report = projectArtifactsReport()
  report.completion = 'partial'
  report.skipped.links = 1
  expect(readProjectArtifacts(report, report.root, report.checkedAt, report.checkedAt)).toEqual(
    report,
  )
  report.items = []
  expect(
    readProjectArtifacts(report, report.root, report.checkedAt, report.checkedAt).items,
  ).toEqual([])
})

it('explains actual skipped causes instead of listing hypothetical failures', async () => {
  const { projectIncompleteReason } = await import('./project-artifacts')
  const report = projectArtifactsReport()
  report.completion = 'partial'
  report.skipped.depth = 2
  expect(projectIncompleteReason(report)).toBe('达到层级限制 2 次')
  report.stopReason = 'candidateLimit'
  expect(projectIncompleteReason(report)).toBe('达到层级限制 2 次；达到结果数量限制')
})

it('binds allocation observations to the requested directory and rejects unsafe or contradictory metadata', () => {
  const fixture = projectMeasurement()
  expect(
    readProjectMeasurement(
      fixture,
      fixture.root,
      fixture.path,
      fixture.checkedAt,
      fixture.checkedAt,
    ),
  ).toEqual(fixture)
  for (const change of [
    (r: typeof fixture) => {
      r.path += '-other'
    },
    (r: typeof fixture) => {
      r.root += '-other'
    },
    (r: typeof fixture) => {
      r.revision = 'other'
    },
    (r: typeof fixture) => {
      r.checkedAt -= 6000
    },
    (r: typeof fixture) => {
      r.canExecute = true as false
    },
    (r: typeof fixture) => {
      r.allocatedBytes = -1
    },
    (r: typeof fixture) => {
      r.allocatedBytes = Number.MAX_SAFE_INTEGER + 1
    },
    (r: typeof fixture) => {
      r.allocatedBytes = 12
    },
    (r: typeof fixture) => {
      r.files = 0
    },
    (r: typeof fixture) => {
      r.visited = 20001
    },
    (r: typeof fixture) => {
      r.skipped.links = 1
    },
    (r: typeof fixture) => {
      r.hardLinks = 1
    },
    (r: typeof fixture) => {
      r.completion = 'partial'
    },
    (r: typeof fixture) => {
      r.stopReason = 'timeLimit'
    },
  ]) {
    const value = structuredClone(fixture)
    change(value)
    expect(() =>
      readProjectMeasurement(
        value,
        fixture.root,
        fixture.path,
        fixture.checkedAt,
        fixture.checkedAt,
      ),
    ).toThrow()
  }
})
it('keeps complete empty measurements distinct from partial zero observations', () => {
  const r = projectMeasurement()
  r.files = 0
  r.allocatedBytes = 0
  r.visited = 0
  expect(readProjectMeasurement(r, r.root, r.path, r.checkedAt, r.checkedAt).completion).toBe(
    'complete',
  )
  r.visited = 3
  r.hardLinks = 1
  r.special = 1
  r.skipped.links = 1
  r.completion = 'partial'
  r.contentSignals.reason = 'incomplete'
  expect(readProjectMeasurement(r, r.root, r.path, r.checkedAt, r.checkedAt).completion).toBe(
    'partial',
  )
  expect(projectMeasurementReason(r)).toBe('跳过链接 1 次；跳过共享硬链接 1 次；跳过特殊文件 1 次')
})

it('requires scoped project-marker hints and never turns missing or uncertain evidence into ownership', () => {
  const fixture = projectArtifactsReport()
  for (const change of [
    (r: typeof fixture) => {
      r.items[0].projectHint.path = '/fixture/outside'
    },
    (r: typeof fixture) => {
      r.items[0].projectHint.path = '/fixture/projects/elsewhere'
    },
    (r: typeof fixture) => {
      r.items[0].projectHint.markers = ['.git']
    },
    (r: typeof fixture) => {
      r.items[0].projectHint.markers = []
    },
    (r: typeof fixture) => {
      r.items[0].projectHint.markers = ['package.json', 'package.json']
    },
    (r: typeof fixture) => {
      r.items[0].projectHint.state = 'unknown'
    },
    (r: typeof fixture) => {
      r.items[2].projectHint.markers = ['Makefile']
    },
  ]) {
    const r = structuredClone(fixture)
    change(r)
    expect(() =>
      readProjectArtifacts(r, fixture.root, fixture.checkedAt, fixture.checkedAt),
    ).toThrow()
  }
  fixture.items[0].projectHint = { state: 'unknown', path: null, kind: null, markers: [] }
  expect(
    readProjectArtifacts(fixture, fixture.root, fixture.checkedAt, fixture.checkedAt).items[0]
      .projectHint.state,
  ).toBe('unknown')
})
it('groups exact workspace hints together while leaving unresolved directories separate', async () => {
  const { groupProjectArtifacts } = await import('./project-artifacts')
  const r = projectArtifactsReport()
  const shared = {
    state: 'hint' as const,
    path: r.root,
    kind: 'workspace' as const,
    markers: ['.git'],
  }
  r.items[0].projectHint = shared
  r.items[1].projectHint = shared
  const groups = groupProjectArtifacts(r.items)
  expect(groups).toHaveLength(2)
  expect(groups[0].items.map((i) => i.name)).toEqual(['node_modules', 'target'])
  expect(groups[1].hint.state).toBe('notFound')
  expect(groups[1].items[0].name).toBe('dist')
})

it('requires bound metadata name signals and rejects clearance or contradictory evidence', () => {
  const base = projectMeasurement()
  const read = (r: unknown) =>
    readProjectMeasurement(r, base.root, base.path, base.checkedAt, base.checkedAt)
  const evidence = { kind: 'repositoryMarker' as const, path: `${base.path}/nested/.git` }
  const found = { ...base, contentSignals: { state: 'found' as const, reason: null, evidence } }
  expect(read(found).contentSignals.evidence).toEqual(evidence)
  expect(
    read({ ...found, completion: 'partial', skipped: { ...base.skipped, links: 1 } }).contentSignals
      .state,
  ).toBe('found')
  expect(
    read({ ...base, contentSignals: { state: 'unknown', reason: 'unavailable', evidence: null } })
      .contentSignals.state,
  ).toBe('unknown')
  for (const change of [
    { protocol: 1 },
    { contentSignals: undefined },
    { contentSignals: { state: 'safe', reason: null, evidence: null } },
    { contentSignals: { state: 'found', reason: 'notObserved', evidence } },
    { contentSignals: { state: 'found', reason: null, evidence: null } },
    { contentSignals: { state: 'unknown', reason: 'notObserved', evidence } },
    { contentSignals: { state: 'unknown', reason: 'incomplete', evidence: null } },
    {
      contentSignals: {
        state: 'found',
        reason: null,
        evidence: { ...evidence, path: '/outside/.git' },
      },
    },
    {
      contentSignals: {
        state: 'found',
        reason: null,
        evidence: { ...evidence, path: base.path + '/../.git' },
      },
    },
    {
      contentSignals: {
        state: 'found',
        reason: null,
        evidence: { ...evidence, path: base.path + '/not-git' },
      },
    },
    {
      contentSignals: {
        state: 'found',
        reason: null,
        evidence: { ...evidence, kind: 'keypairName' },
      },
    },
    {
      contentSignals: {
        state: 'found',
        reason: null,
        evidence: { ...evidence, kind: 'gitTracked' },
      },
    },
    { visited: 0, files: 0, allocatedBytes: 0 },
  ])
    expect(() => read({ ...found, ...change })).toThrow()
})
