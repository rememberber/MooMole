import { createRoot } from 'react-dom/client'
import { useState } from 'react'
import { LogCleanupDialog } from '../../../src/components/LogCleanupDialog'
import type { CleanupConfirmation, ExecutionDetail } from '../../../src/types'
import { useLanguage, setLanguagePreference } from '../../../src/i18n/locale'

export function mount(mode: string) {
  const now = Date.now(),
    mib = 1024 ** 2,
    available = 50 * 1024 ** 3
  const confirmation: CleanupConfirmation = {
    version: 2,
    id: '12345678-1234-4123-8123-123456789abc',
    planId: 'plan',
    snapshotSha256: 'a'.repeat(64),
    method: 'trash',
    createdAt: now,
    expiresAt: now + 30000,
    retainedCount: 0,
    requiresAuthorization: false,
    items: [
      { id: 'moved', path: '/fixture/selected.log', sizeBytes: 8 * mib },
      { id: 'cancelled', path: '/fixture/other.log', sizeBytes: 4 * mib },
    ],
  }
  const result: ExecutionDetail = {
    id: 'run',
    planId: 'plan',
    method: 'trash',
    state: 'cancelled',
    createdAt: now,
    finishedAt: now + 2,
    itemCount: 2,
    movedCount: 1,
    uncertainCount: 0,
    reviewedAt: null,
    retainedForReview: true,
    revision: 'fixture',
    protocol: 2,
    snapshotSha256: 'a'.repeat(64),
    events: [],
    items: [
      {
        id: 'moved',
        path: '/fixture/selected.log',
        state: 'moved',
        reason: 'fixture moved',
        scanSizeBytes: 8 * mib,
        receipt: { destination: '/fixture/.Trash/selected.log', device: '1', inode: '2' },
      },
      {
        id: 'cancelled',
        path: '/fixture/other.log',
        state: 'cancelled',
        reason: 'fixture cancelled',
        scanSizeBytes: 4 * mib,
        receipt: null,
      },
    ],
    spaceObservation: {
      version: 1,
      before: {
        measuredAt: now + 1,
        volumeId: '1:aa',
        totalBytes: 100 * 1024 ** 3,
        availableBytes: available,
      },
      after: {
        measuredAt: now + 3,
        volumeId: mode === 'changed' ? '2:aa' : '1:aa',
        totalBytes: 100 * 1024 ** 3,
        availableBytes:
          available + (mode === 'decrease' ? -16 * mib : mode === 'zero' ? 0 : 16 * mib),
      },
    },
  }
  if (mode === 'missing') {
    delete result.spaceObservation
    delete result.items[1].scanSizeBytes
  }
  function Fixture() {
    useLanguage()
    const [calls, setCalls] = useState(0)
    return (
      <>
        <output aria-label="调用次数">{calls}</output>
        <button onClick={() => setLanguagePreference('en')}>English</button>
        <button onClick={() => setLanguagePreference('ja')}>日本語</button>
        <LogCleanupDialog
          confirmation={confirmation}
          simple
          onExecute={async () => {
            setCalls((count) => count + 1)
            return result
          }}
          onBack={() => {}}
          onClose={() => {}}
        />
      </>
    )
  }
  const host = document.createElement('div')
  document.body.append(host)
  createRoot(host).render(<Fixture />)
}
