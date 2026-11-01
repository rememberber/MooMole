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

const officeTarget: PreviewTarget = {
  domain: 'personalFiles',
  scanId: 'scan',
  itemId: 'item',
  path: '/Users/owned/预算.xlsx',
}
const office = {
  kind: 'document',
  format: 'xlsx',
  part: 1,
  sections: [
    { title: '清理', hidden: false },
    { title: 'Archive', hidden: true },
  ],
  text: 'A1\t<script>literal</script>\nB1\t=SUM(A2:A3) → 4\n',
  truncated: false,
}
it('binds office format, ordered part and literal data to the exact retained file', () => {
  const report = { ...officeTarget, content: office }
  expect(readFilePreview(report, officeTarget, 1).content).toEqual(office)
  expect(() => readFilePreview(report, officeTarget, 2)).toThrow()
  expect(() => readFilePreview({ ...report, path: '/other.xlsx' }, officeTarget, 1)).toThrow()
  for (const format of ['docx', 'pptx']) {
    const target = { ...officeTarget, path: `/Users/owned/file.${format}` }
    const content = { ...office, format, sections: [{ title: '', hidden: false }] }
    expect(readFilePreview({ ...target, content }, target, 1).content.kind).toBe('document')
    expect(() => readFilePreview({ ...report, content }, officeTarget, 1)).toThrow()
  }
})
it('rejects malformed, over-budget or mismatched office excerpts instead of displaying partial protocol data', () => {
  for (const patch of [
    { format: 'docm' },
    { part: 0 },
    { part: 1.5 },
    { part: 3 },
    { sections: [] },
    { sections: null },
    { sections: Array.from({ length: 513 }, () => office.sections[0]) },
    { sections: [{ title: 'bad\nname', hidden: false }] },
    { sections: [{ title: '\u202espoof', hidden: false }] },
    { sections: [{ title: 'x', hidden: 'true' }] },
    { sections: [{ title: '字'.repeat(43), hidden: false }] },
    { text: '字'.repeat(21846) },
    { text: '\u0000' },
    { truncated: 0 },
  ])
    expect(() =>
      readFilePreview({ ...officeTarget, content: { ...office, ...patch } }, officeTarget, 1),
    ).toThrow()
  const target = { ...officeTarget, path: '/Users/owned/word.docx' }
  expect(() =>
    readFilePreview(
      {
        ...target,
        content: {
          ...office,
          format: 'docx',
          sections: [
            { title: '', hidden: false },
            { title: '', hidden: false },
          ],
        },
      },
      target,
      1,
    ),
  ).toThrow()
})
