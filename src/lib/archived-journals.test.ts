import { it, expect } from 'vitest'
import { archivedJournals } from '../../tests/fixtures/archived-journals'
import { journalSummary, readJournalRecord, type JournalKind } from './archived-journals'
import {
  archiveHeader,
  archiveView,
  archiveDetail,
  archiveEntryStateName,
} from './execution-archive'
const archiveId = 'a1000000-0000-4000-8000-000000000001'
function fixture() {
  const records = archivedJournals(),
    header = {
      format: 'MooMole execution evidence',
      version: 4,
      id: archiveId,
      createdAt: 1002,
      counts: {
        cleanup: 0,
        uninstall: 0,
        maintenance: 0,
        moleClean: 1,
        extraction: 1,
        managedApp: 1,
      },
      payloadSha256: 'a'.repeat(64),
      payloadBytes: 200000,
    }
  const entries = (Object.keys(records) as JournalKind[]).map((kind) => ({
    kind,
    id: records[kind].id,
    summary: journalSummary(kind, records[kind]),
  }))
  return { records, header, entries }
}
it('reads v4 journals with old engine revisions and all events without converting evidence into live plans', () => {
  const f = fixture(),
    view = archiveView(f, archiveId)
  for (const entry of view.entries)
    expect(archiveDetail(f.records[entry.kind as JournalKind], entry)).toEqual(
      f.records[entry.kind as JournalKind],
    )
  expect(f.records.moleClean.events).toHaveLength(2101)
  expect(archiveEntryStateName('moleClean', 'failed')).toBe('执行失败，需核对已发生的结果')
  expect(archiveEntryStateName('managedApp', 'removed')).not.toContain('废纸篓')
  for (const version of [1, 2, 3]) expect(() => archiveHeader({ ...f.header, version })).toThrow()
  expect(() => archiveView({ ...f, entries: f.entries.slice(1) }, archiveId)).toThrow()
})
it('requires exact kind, summary, source path, outcome and uncertainty on detail responses', () => {
  const f = fixture(),
    view = archiveView(f, archiveId)
  for (const entry of view.entries) {
    const raw = f.records[entry.kind as JournalKind]
    for (const patch of [
      { state: 'running' },
      { reviewedAt: 1003 },
      { finishedAt: 1003 },
      { id: archiveId },
    ])
      expect(() => archiveDetail({ ...raw, ...patch }, entry)).toThrow()
    const swapped = f.records[entry.kind === 'managedApp' ? 'extraction' : 'managedApp']
    expect(() => archiveDetail(swapped, entry)).toThrow()
  }
})
it('keeps reviewed unknowns unknown and blocks unreviewed pending Mole intent even if its state says failed', () => {
  const f = fixture(),
    r = f.records.moleClean
  r.events = [{ seq: 1, kind: 'intent', path: '/absent/owned', detail: 'pending', token: r.id }]
  expect(() => readJournalRecord(r, 'moleClean')).toThrow()
  r.reviewedAt = 1002
  expect(readJournalRecord(r, 'moleClean').state).toBe('failed')
  expect(journalSummary('moleClean', r).resultUncertain).toBe(true)
  const entry = {
    kind: 'moleClean' as const,
    id: r.id,
    summary: { ...journalSummary('moleClean', r), resultUncertain: false },
  }
  expect(() => archiveDetail(r, entry)).toThrow()
  for (const kind of ['extraction', 'managedApp'] as const) {
    const r = f.records[kind]
    r.state = 'unknown'
    expect(() => readJournalRecord(r, kind)).toThrow()
    r.reviewedAt = 1002
    expect(readJournalRecord(r, kind).state).toBe('unknown')
  }
})
it('rejects truncated or reordered events, malformed token bindings, excess evidence and impossible dates', () => {
  const r = fixture().records.moleClean
  for (const patch of [
    { eventsTruncated: true },
    { events: [{ ...r.events[0], seq: 2 }] },
    { events: [{ ...r.events[0], kind: 'removed' }] },
    { events: [{ ...r.events[0], path: '/absent/../escape' }] },
    { events: Array.from({ length: 20001 }, (_, i) => ({ ...r.events[0], seq: i + 1 })) },
    { finishedAt: 999 },
    { beforeBytes: Number.MAX_SAFE_INTEGER + 1 },
    { diagnostic: 'x'.repeat(256 * 1024 + 1) },
  ])
    expect(() => readJournalRecord({ ...r, ...patch }, 'moleClean')).toThrow()
  const intent = { seq: 1, kind: 'intent', token: r.id, path: '/absent/a', detail: 'owned' },
    result = { ...intent, seq: 2, kind: 'removed', path: '/absent/b' }
  expect(() => readJournalRecord({ ...r, events: [intent, result] }, 'moleClean')).toThrow()
})

it('upgrades require v5 and preserve their evidence without being labelled as uninstalls', () => {
  const f = fixture(),
    r = f.records.managedApp
  r.draft.upgrade = {
    version: '2.0',
    url: 'https://example.org/owned.zip',
    sha256: 'a'.repeat(64),
    publisher: 'adHoc',
  }
  r.state = 'upgraded'
  r.upgradeResult = {
    version: '2.0',
    publisher: 'adHoc',
    bundleSha256: 'b'.repeat(64),
    receiptSha256: 'c'.repeat(64),
  }
  f.entries = f.entries.map((e) =>
    e.kind === 'managedApp' ? { ...e, summary: journalSummary('managedApp', r) } : e,
  )
  const raw = { ...f, header: { ...f.header, version: 5 } },
    view = archiveView(raw, archiveId),
    entry = view.entries.find((e) => e.kind === 'managedApp')!
  expect(archiveDetail(r, entry)).toEqual(r)
  expect(archiveEntryStateName('managedApp', 'upgraded')).toContain('新版')
  for (const version of [1, 2, 3, 4])
    expect(() => archiveView({ ...raw, header: { ...raw.header, version } }, archiveId)).toThrow()
  expect(() =>
    archiveView(
      {
        ...raw,
        entries: f.entries.map((e) =>
          e.kind === 'managedApp' ? { ...e, summary: { ...e.summary, operation: undefined } } : e,
        ),
      },
      archiveId,
    ),
  ).toThrow()
})
