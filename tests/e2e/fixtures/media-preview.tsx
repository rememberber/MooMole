import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { FilePreview } from '../../../src/components/FilePreview'
import type { PreviewTarget } from '../../../src/lib/file-preview'
export function mount(kind: 'audio' | 'video', mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'media-preview-fixture'
  document.body.append(host)
  const id = '12345678-1234-4123-8123-123456789abc'
  const target: PreviewTarget = {
    domain: 'personalFiles',
    scanId: id,
    itemId: '22345678-1234-4123-8123-123456789abc',
    path: kind === 'video' ? '/Users/owned/clip.mp4' : '/Users/owned/audio.wav',
  }
  const releases: string[] = []
  host.dataset.releases = '[]'
  const transport = async () => {
    host.dataset.reads = String(Number(host.dataset.reads ?? 0) + 1)
    if (mode === 'late') await new Promise((r) => setTimeout(r, 300))
    return {
      ...target,
      content: {
        kind: 'media',
        leaseId: id,
        url: `http://moomole-media.localhost/${id}/media`,
        mediaKind: kind,
        mime: kind === 'video' ? 'video/mp4' : 'audio/wav',
        expiresAt: Date.now() + (mode === 'expiry' ? 2000 : 900000),
      },
    }
  }
  const release = async (id: string) => {
    releases.push(id)
    host.dataset.releases = JSON.stringify(releases)
  }
  function Fixture() {
    const [open, setOpen] = useState(true)
    return (
      <FilePreview
        target={open ? target : null}
        onClose={() => setOpen(false)}
        transport={transport}
        release={release}
      />
    )
  }
  createRoot(host).render(<Fixture />)
}
