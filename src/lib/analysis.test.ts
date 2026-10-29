import { describe, expect, it } from 'vitest'
import type { Analysis, AnalysisObservation, Entry } from '../types'
import {
  analysisObservation,
  compareEntries,
  entryBytes,
  measurementCoverage,
  parentDirectory,
  spaceTiles,
} from './analysis'

const item = (
  name: string,
  size: number,
  scan_status: Entry['scan_status'] = 'complete',
): Entry => ({ name, path: `/fixture/${name}`, size, scan_status, is_dir: false })

describe('analysis measurements and map geometry', () => {
  it('counts whole-result coverage without promoting partial zero or malformed sizes to complete', () => {
    expect(
      measurementCoverage([
        item('complete-zero', 0),
        item('partial-zero', 0, 'partial'),
        item('unknown', 999, 'unavailable'),
        item('negative', -1),
        item('invalid', Infinity),
        { ...item('future-state', 9), scan_status: 'future' as Entry['scan_status'] },
      ]),
    ).toEqual({ complete: 1, partial: 1, unknown: 4 })
    expect(measurementCoverage([])).toEqual({ complete: 0, partial: 0, unknown: 0 })
  })
  it('accepts recorded read times but leaves old, future and malformed provenance unknown', () => {
    const analysis: Analysis = { path: '/', scan_status: 'complete', total_size: 0, entries: [] }
    const observation: AnalysisObservation = {
      version: 1,
      source: 'mole-directory-json',
      cacheUse: 'unreported',
      startedAt: 2000,
      finishedAt: 1000,
      elapsedMs: 250,
      engineVersion: '1.65.0',
      revision: 'fixture',
    }
    // Duration is independent of wall-clock ordering.
    expect(analysisObservation({ ...analysis, mooObservation: observation })).toEqual(observation)
    expect(analysisObservation(analysis)).toBeNull()
    for (const patch of [
      { version: 2 },
      { source: 'future' },
      { cacheUse: 'fresh' },
      { startedAt: 0 },
      { finishedAt: NaN },
      { finishedAt: 8640000000000001 },
      { elapsedMs: -1 },
      { elapsedMs: 0.5 },
      { engineVersion: 1 },
      { revision: null },
    ]) {
      expect(
        analysisObservation({
          ...analysis,
          mooObservation: { ...observation, ...patch } as AnalysisObservation,
        }),
      ).toBeNull()
    }
  })
  it('excludes unknown and zero measurements from area without treating unknown as zero', () => {
    const unknown = item('unknown', 999999, 'unavailable')
    const partial = item('partial', 10, 'partial')
    expect(entryBytes(unknown)).toBeNull()
    for (const value of [-1, NaN, Infinity, Number.MAX_VALUE])
      expect(entryBytes(item('bad', value))).toBeNull()
    expect(entryBytes(item('zero', 0))).toBe(0)
    expect(spaceTiles([unknown, item('zero', 0), partial]).map((tile) => tile.label)).toEqual([
      'partial',
    ])
    expect(
      [unknown, item('zero', 0), partial]
        .sort((a, b) => compareEntries(a, b, 'size'))
        .map((entry) => entry.name),
    ).toEqual(['partial', 'zero', 'unknown'])
  })
  it('fills the map with nonoverlapping rectangles proportional to measured size', () => {
    for (const sizes of [
      [1],
      [1, 2, 3],
      [1, 9, 100, 300, 5],
      Array.from({ length: 50 }, (_, i) => (i + 1) ** 3),
    ]) {
      const entries = sizes.map((size, i) => item(String(i), size))
      const tiles = spaceTiles(entries)
      const total = sizes.reduce((sum, size) => sum + size, 0)
      expect(tiles.reduce((sum, tile) => sum + tile.width * tile.height, 0)).toBeCloseTo(360000, 5)
      for (const tile of tiles) {
        expect(tile.x).toBeGreaterThanOrEqual(0)
        expect(tile.y).toBeGreaterThanOrEqual(0)
        expect(tile.x + tile.width).toBeLessThanOrEqual(1000.000001)
        expect(tile.y + tile.height).toBeLessThanOrEqual(360.000001)
        expect((tile.width * tile.height) / 360000).toBeCloseTo(tile.size / total, 10)
        for (const other of tiles)
          if (tile !== other) {
            const overlap =
              Math.max(
                0,
                Math.min(tile.x + tile.width, other.x + other.width) - Math.max(tile.x, other.x),
              ) *
              Math.max(
                0,
                Math.min(tile.y + tile.height, other.y + other.height) - Math.max(tile.y, other.y),
              )
            expect(overlap).toBeLessThan(0.000001)
          }
      }
    }
  })
  it('bounds individual map targets and preserves all remaining measured weight in one aggregate', () => {
    const entries = Array.from({ length: 205 }, (_, i) => item(String(i), i + 1))
    const original = entries.map((entry) => entry.path)
    const tiles = spaceTiles(entries)
    expect(tiles.filter((tile) => tile.entry)).toHaveLength(12)
    expect(tiles.at(-1)?.label).toBe('其他 193 项')
    expect(tiles.reduce((sum, tile) => sum + tile.size, 0)).toBe(
      entries.reduce((sum, entry) => sum + entry.size, 0),
    )
    expect(entries.map((entry) => entry.path)).toEqual(original)
    expect(spaceTiles([item('empty', 0), item('blocked', 0, 'unavailable')])).toEqual([])
  })
  it('keeps parent navigation at root and handles trailing separators', () => {
    expect(parentDirectory('/')).toBe('/')
    expect(parentDirectory('/folder')).toBe('/')
    expect(parentDirectory('/folder/child/')).toBe('/folder')
  })
})
