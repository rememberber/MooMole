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
      r.protocol = 2 as 1
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
  expect(readProjectMeasurement(r, r.root, r.path, r.checkedAt, r.checkedAt).completion).toBe(
    'partial',
  )
  expect(projectMeasurementReason(r)).toBe('跳过链接 1 次；跳过共享硬链接 1 次；跳过特殊文件 1 次')
})
