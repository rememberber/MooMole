import type { MoleCleanRecord } from '../../src/lib/mole-cleanup'
import { moleCleanScopes } from '../../src/lib/mole-cleanup'
import type { ZipExtractionRecord } from '../../src/lib/zip-extraction'
import type { ManagedRecord } from '../../src/lib/managed-apps'
export function archivedJournals(at = 1000) {
  const moleId = 'a2000000-0000-4000-8000-000000000001',
    zipId = 'a2000000-0000-4000-8000-000000000002',
    managedId = 'a2000000-0000-4000-8000-000000000003'
  const moleClean: MoleCleanRecord = {
    id: moleId,
    plan: {
      version: 1,
      id: moleId,
      revision: '0'.repeat(40),
      engineVersion: '1.64.0',
      home: '/absent/historical-account',
      system: false,
      method: 'molePermanent',
      protectedPaths: ['/absent/protected'],
      createdAt: at,
      expiresAt: at + 120000,
      scope: moleCleanScopes.map(([id]) => id),
    },
    state: 'failed',
    createdAt: at,
    finishedAt: at + 1,
    beforeBytes: 1024,
    afterBytes: 2048,
    error: 'partial result; keep evidence',
    diagnostic: 'owned diagnostics',
    reviewedAt: null,
    events: Array.from({ length: 2101 }, (_, i) => ({
      seq: i + 1,
      kind: 'upstream' as const,
      path: '',
      detail: `owned event ${i + 1}`,
      token: '',
    })),
    eventsTruncated: false,
  }
  const extraction: ZipExtractionRecord = {
    id: zipId,
    requestId: zipId,
    draft: {
      format: 'tarGzip',
      id: zipId,
      root: '/absent/source.tar.gz',
      outputPath: '/absent/output',
      files: 1,
      directories: 1,
      hidden: 0,
      bytes: 12,
      metadataBytes: 0,
      sourceSha256: 'b'.repeat(64),
      samples: [{ name: 'owned.txt', directory: false }],
      createdAt: at,
      expiresAt: at + 120000,
    },
    temporaryPath: `/absent/.MooMole-zip-extract-${zipId}.partial`,
    state: 'completed',
    phase: 'finished',
    createdAt: at,
    finishedAt: at + 1,
    artifact: { device: '1', inode: '2', bytes: 12, sha256: 'c'.repeat(64) },
    reason: 'owned extraction evidence',
    reviewedAt: null,
  }
  const managedApp: ManagedRecord = {
    id: managedId,
    requestId: managedId,
    draft: {
      id: managedId,
      taskId: managedId,
      index: 0,
      path: '/absent/Owned.app',
      bundleId: 'org.moomole.owned',
      token: 'owned',
      name: 'Owned',
      installedVersion: '1.0',
      managerPath: '/opt/homebrew/bin/brew',
      metadataSha256: 'a'.repeat(64),
      createdAt: at,
      expiresAt: at + 120000,
    },
    state: 'removed',
    phase: 'finished',
    createdAt: at,
    finishedAt: at + 1,
    reason: 'owned evidence, not an actual command',
    reviewedAt: null,
  }
  return { moleClean, extraction, managedApp }
}
