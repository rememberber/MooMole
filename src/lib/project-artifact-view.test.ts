import { expect, it } from 'vitest'
import {
  projectArtifactsReport,
  projectMeasurement,
} from '../../tests/e2e/fixtures/project-artifacts'
import { initialProjectFilters, projectArtifactView } from './project-artifact-view'
import type { ProjectArtifact } from './project-artifacts'
it('filters names and paths without changing scan evidence, and keeps unresolved groups last', () => {
  const report = projectArtifactsReport(),
    original = structuredClone(report)
  const view = projectArtifactView(
    report.items,
    {},
    { ...initialProjectFilters, query: ' WEB/Node_Modules ' },
  )
  expect(view.groups.flatMap((g) => g.items).map((i) => i.path)).toEqual([report.items[0].path])
  expect(
    projectArtifactView(report.items, {}, { ...initialProjectFilters, context: 'rustContext' })
      .groups[0].items[0].protection,
  ).toBe('matched')
  expect(
    projectArtifactView(report.items, {}, { ...initialProjectFilters, query: 'absent' }),
  ).toMatchObject({ total: 0, start: 0, end: 0, page: 1, pages: 1 })
  expect(projectArtifactView(report.items, {}, initialProjectFilters).groups.at(-1)?.key).toBe(
    'unresolved',
  )
  expect(report).toEqual(original)
})
it('orders observed sizes within a group, preserving zero, partial lower bounds and unknown observations', () => {
  const base = projectArtifactsReport().items[0],
    report = projectMeasurement()
  const names = ['unknown', 'zero', 'large', 'partial', 'incomplete']
  const items: ProjectArtifact[] = names.map((name) => ({
    ...base,
    path: `/fixture/projects/${name}/node_modules`,
    projectHint: { ...base.projectHint, path: '/fixture/projects' },
  }))
  const measurements = {
    [items[1].path]: { report: { ...report, allocatedBytes: 0, files: 0 } },
    [items[2].path]: { report: { ...report, allocatedBytes: 8192 }, error: 'last recheck failed' },
    [items[3].path]: {
      report: { ...report, completion: 'partial' as const, files: 1, allocatedBytes: 4096 },
    },
    [items[4].path]: {
      report: { ...report, completion: 'partial' as const, files: 0, allocatedBytes: 0 },
    },
  }
  const view = projectArtifactView(items, measurements, { ...initialProjectFilters, sort: 'size' })
  expect(view.groups[0].items.map((i) => i.path.split('/').at(-2))).toEqual([
    'large',
    'partial',
    'zero',
    'incomplete',
    'unknown',
  ])
  expect(items.map((i) => i.path.split('/').at(-2))).toEqual(names)
})
it('pages across group boundaries with no omission and clamps pages after result changes', () => {
  const base = projectArtifactsReport().items[0]
  const items = Array.from({ length: 123 }, (_, n) => ({
    ...base,
    path: `/fixture/projects/${n < 55 ? 'first' : 'second'}/p${String(n).padStart(3, '0')}/node_modules`,
    projectHint: {
      ...base.projectHint,
      path: n < 55 ? '/fixture/projects/first' : '/fixture/projects/second',
    },
  }))
  const paths: string[] = []
  for (let page = 1; page <= 3; page++) {
    const view = projectArtifactView(items, {}, { ...initialProjectFilters, page })
    expect(view).toMatchObject({ page, pages: 3, total: 123 })
    paths.push(...view.groups.flatMap((g) => g.items).map((i) => i.path))
    expect(view.end - view.start + 1).toBe(page === 3 ? 23 : 50)
    if (page === 2) expect(view.groups.map((g) => g.items.length)).toEqual([5, 45])
  }
  expect(paths).toEqual(items.map((i) => i.path))
  expect(new Set(paths).size).toBe(123)
  expect(
    projectArtifactView(items.slice(0, 4), {}, { ...initialProjectFilters, page: 3 }),
  ).toMatchObject({ page: 1, pages: 1, start: 1, end: 4 })
})
