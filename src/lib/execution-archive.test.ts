import { describe, it, expect } from 'vitest'
import {
  archiveHeader,
  archivePreview,
  archiveList,
  archiveView,
  archiveDetail,
} from './execution-archive'
import { detailRaw, requestId } from '../../tests/fixtures/uninstall'
const at = Date.now(),
  archiveId = 'a1000000-0000-4000-8000-000000000001'
const header = {
  format: 'MooMole execution evidence',
  version: 1,
  id: archiveId,
  createdAt: at,
  counts: { cleanup: 0, uninstall: 1, maintenance: 0 },
  payloadSha256: 'a'.repeat(64),
  payloadBytes: 1000,
}
function fixture() {
  const detail = { ...detailRaw(requestId, 'uncertain'), reviewedAt: at }
  return {
    detail,
    view: { header, entries: [{ kind: 'uninstall', id: detail.id, summary: detail }] },
  }
}
describe('portable evidence boundaries', () => {
  it('accepts only a supported bounded header and scope, never a zero or malformed capacity', () => {
    expect(archiveHeader(header).counts.uninstall).toBe(1)
    for (const patch of [
      { version: 2 },
      { payloadBytes: 128 * 1024 * 1024 + 1 },
      { payloadBytes: 0 },
      { counts: { cleanup: 0, uninstall: 0, maintenance: 0 } },
      { createdAt: -1 },
      { payloadSha256: 'A'.repeat(64) },
    ])
      expect(() => archiveHeader({ ...header, ...patch })).toThrow()
    expect(() =>
      archivePreview({
        counts: header.counts,
        fingerprint: header.payloadSha256,
        reservedBytes: 0,
      }),
    ).toThrow()
  })
  it('keeps unreadable catalogue entries explicit and rejects duplicate archive identifiers', () => {
    expect(archiveList({ archives: [header], complete: false, unreadable: 1 })).toMatchObject({
      complete: false,
      unreadable: 1,
    })
    expect(() =>
      archiveList({ archives: [header, header], complete: true, unreadable: 0 }),
    ).toThrow()
  })
  it('rejects cross-archive substitution, running work, unreviewed unknowns and missing batches', () => {
    const f = fixture()
    expect(archiveView(f.view, archiveId).entries[0].summary.reviewedAt).toBe(at)
    expect(() => archiveView(f.view, requestId)).toThrow()
    expect(() => archiveView({ ...f.view, entries: [] }, archiveId)).toThrow()
    for (const raw of [{ ...f.detail, reviewedAt: null }, detailRaw(requestId, 'intent')])
      expect(() =>
        archiveView(
          { ...f.view, entries: [{ kind: 'uninstall', id: requestId, summary: raw }] },
          archiveId,
        ),
      ).toThrow()
  })
  it('requires the exact saved summary on detail reads and preserves original uncertainty', () => {
    const f = fixture(),
      entry = archiveView(f.view, archiveId).entries[0]
    expect(archiveDetail(f.detail, entry)).toMatchObject({
      itemState: 'uncertain',
      reviewedAt: at,
      receipt: f.detail.receipt,
    })
    for (const patch of [
      { id: archiveId },
      { name: 'rewritten identity' },
      { reviewedAt: at + 1 },
      { snapshotSha256: 'invalid' },
    ])
      expect(() => archiveDetail({ ...f.detail, ...patch }, entry)).toThrow()
  })
})
