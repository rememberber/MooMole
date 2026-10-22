import {
  installerCatalog,
  type InstallersReport,
  type InstallerZipInspection,
  type InstallerZipDataInspection,
  type InstallerConnectionInspection,
  type InstallerUsageInspection,
  type InstallerImageInspection,
} from '../../../src/lib/installers'
export function installersReport(): InstallersReport {
  return {
    protocol: 1,
    revision: installerCatalog.revision,
    root: '/fixture/downloads',
    checkedAt: Date.now(),
    completion: 'complete',
    stopReason: null,
    visited: 10,
    skipped: {
      links: 0,
      unreadable: 0,
      nonLocal: 0,
      depth: 0,
      invalidName: 0,
      changed: 0,
      unsupportedType: 0,
    },
    canExecute: false,
    items: [
      {
        path: '/fixture/downloads/Tool.dmg',
        name: 'Tool.dmg',
        extension: 'dmg',
        sizeBytes: 1048576,
        measurement: 'measured',
        classification: 'nameOnly',
        sharing: 'single',
        protection: 'notMatched',
        usage: 'notChecked',
        canExecute: false,
      },
      {
        path: '/fixture/downloads/archive.zip',
        name: 'archive.zip',
        extension: 'zip',
        sizeBytes: null,
        measurement: 'unavailable',
        classification: 'archiveUnverified',
        sharing: 'shared',
        protection: 'matched',
        usage: 'notChecked',
        canExecute: false,
      },
    ],
  }
}

export function installerZipInspection(): InstallerZipInspection {
  const r = installersReport()
  return {
    protocol: 2,
    revision: r.revision,
    root: r.root,
    path: r.items[1].path,
    checkedAt: Date.now(),
    fileStamp: 'a'.repeat(64),
    state: 'installerHint',
    reason: null,
    totalEntries: 83,
    checkedEntries: 50,
    matchedTypes: ['app'],
    canExecute: false,
  }
}

export function installerConnectionInspection(): InstallerConnectionInspection {
  const r = installersReport()
  return {
    protocol: 1,
    revision: r.revision,
    root: r.root,
    path: r.items[0].path,
    checkedAt: Date.now(),
    state: 'pathReported',
    reason: null,
    canExecute: false,
  }
}

export function installerUsageInspection(): InstallerUsageInspection {
  const r = installersReport()
  return {
    protocol: 1,
    revision: r.revision,
    root: r.root,
    path: r.items[0].path,
    checkedAt: Date.now(),
    state: 'busy',
    reason: null,
    canExecute: false,
  }
}

export function installerZipDataInspection(): InstallerZipDataInspection {
  const r = installerZipInspection()
  return {
    protocol: 1,
    revision: r.revision,
    root: r.root,
    path: r.path,
    checkedAt: Date.now(),
    fileStamp: r.fileStamp,
    state: 'passed',
    reason: null,
    canExecute: false,
  }
}

export function installerImageInspection(): InstallerImageInspection {
  const r = installersReport()
  return {
    protocol: 1,
    revision: r.revision,
    root: r.root,
    path: r.items[0].path,
    checkedAt: Date.now(),
    fileStamp: 'c'.repeat(64),
    state: 'passed',
    reason: null,
    canExecute: false,
  }
}
