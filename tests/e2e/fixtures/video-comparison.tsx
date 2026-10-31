import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { VideoComparison } from '../../../src/components/VideoComparison'
import type { VideoTarget, VideoComparisonTransport } from '../../../src/lib/video-comparison'
// This transport only serves owned browser fixtures, never native file access.
export function mount(mode = 'normal', count = 2) {
  const host = document.createElement('div')
  host.id = 'video-comparison-fixture'
  document.body.append(host)
  const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
  const targets: VideoTarget[] = Array.from({ length: count }, (_, n) => ({
    domain: 'personalFiles',
    scanId: uuid(10),
    itemId: uuid(n + 1),
    name: `clip-${n + 1}.mp4`,
    path: `/Users/owned/clip-${n + 1}.mp4`,
    size: 12345,
  }))
  const transport: VideoComparisonTransport = {
    prepare: async (comparisonId, items) => {
      host.dataset.request = comparisonId
      host.dataset.reads = String(Number(host.dataset.reads ?? 0) + 1)
      if (mode === 'late') await new Promise((r) => setTimeout(r, 300))
      return {
        comparisonId,
        scanId: uuid(10),
        items: items.map((item, n) => ({
          scanId: item.scanId,
          itemId: item.itemId,
          path: mode === 'mismatch' && n === 1 ? '/Users/owned/other.mp4' : item.path,
          content: {
            kind: 'media',
            url: `http://moomole-media.localhost/${uuid(n + 100)}/media`,
            leaseId: uuid(n + 100),
            mime: 'video/mp4',
            mediaKind: 'video',
            expiresAt: Date.now() + (mode === 'expiry' ? 2000 : 60000),
          },
        })),
      }
    },
    release: async (id) => {
      host.dataset.released = id
      host.dataset.releases = String(Number(host.dataset.releases ?? 0) + 1)
    },
  }
  function Fixture() {
    const [open, setOpen] = useState(true)
    return (
      <VideoComparison
        targets={open ? targets : null}
        onClose={() => setOpen(false)}
        onChoose={(ids) => {
          host.dataset.selected = JSON.stringify(ids)
          host.dataset.applied = 'true'
        }}
        transport={transport}
      />
    )
  }
  host.dataset.selected = '[]'
  host.dataset.applied = 'false'
  createRoot(host).render(<Fixture />)
}
