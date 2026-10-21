import {
  projectCatalog,
  type ProjectArtifactsReport,
  type ProjectMeasurement,
} from '../../../src/lib/project-artifacts'
export function projectArtifactsReport(): ProjectArtifactsReport {
  return {
    protocol: 1,
    revision: projectCatalog.revision,
    root: '/fixture/projects',
    checkedAt: Date.now(),
    completion: 'complete',
    stopReason: null,
    visited: 20,
    skipped: { links: 0, unreadable: 0, nonLocal: 0, depth: 0, invalidName: 0, changed: 0 },
    canExecute: false,
    items: [
      {
        path: '/fixture/projects/web/node_modules',
        name: 'node_modules',
        context: 'nodeContext',
        projectPath: '/fixture/projects/web',
        protection: 'notMatched',
        measurement: 'notMeasured',
        canExecute: false,
      },
      {
        path: '/fixture/projects/rust/target',
        name: 'target',
        context: 'rustContext',
        projectPath: '/fixture/projects/rust',
        protection: 'matched',
        measurement: 'notMeasured',
        canExecute: false,
      },
      {
        path: '/fixture/projects/unknown/dist',
        name: 'dist',
        context: 'nameOnly',
        projectPath: null,
        protection: 'unknown',
        measurement: 'notMeasured',
        canExecute: false,
      },
    ],
  }
}

export function projectMeasurement(): ProjectMeasurement {
  const report = projectArtifactsReport()
  return {
    protocol: 1,
    revision: report.revision,
    root: report.root,
    path: report.items[0].path,
    checkedAt: Date.now(),
    completion: 'complete',
    stopReason: null,
    allocatedBytes: 8192,
    files: 2,
    visited: 3,
    skipped: report.skipped,
    hardLinks: 0,
    special: 0,
    canExecute: false,
  }
}
