import { it, expect } from 'vitest'
import { readFilePreview, type PreviewTarget } from './file-preview'
it('accepts only a bounded native media capability bound to the current scan and file', () => {
  const id = '12345678-1234-4123-8123-123456789abc'
  const target: PreviewTarget = {
    domain: 'personalFiles',
    scanId: id,
    itemId: id,
    path: '/Users/owned/file.mp4',
  }
  const content = {
    kind: 'media',
    leaseId: id,
    url: `moomole-media://localhost/${id}/media`,
    mime: 'video/mp4',
    mediaKind: 'video',
    expiresAt: Date.now() + 60000,
  }
  const report = { ...target, content }
  expect(readFilePreview(report, target, 1).content.kind).toBe('media')
  for (const override of [
    { url: 'file:///etc/passwd' },
    { url: 'https://example.com/video.mp4' },
    { url: `moomole-media://localhost/${id}/media?path=/other` },
    { expiresAt: 1 },
    { mime: 'text/html' },
  ])
    expect(() =>
      readFilePreview({ ...report, content: { ...content, ...override } }, target, 1),
    ).toThrow()
  expect(() => readFilePreview({ ...report, itemId: 'other' }, target, 1)).toThrow()
})
