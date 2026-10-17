import { describe, expect, it } from 'vitest'
import type { Analysis, LargeFile } from '../types'
import {
  fileKind,
  filterLargeFiles,
  largeFileBytes,
  largeFileInScope,
  largeFileReport,
} from './large-files'

const file = (path: string, size = 1): LargeFile => ({
  name: path.slice(path.lastIndexOf('/') + 1),
  path,
  size,
})

describe('Mole large-file result boundaries', () => {
  it('keeps reported, omitted and malformed lists distinct without inventing files', () => {
    const analysis: Analysis = { path: '/', entries: [], total_size: 0, scan_status: 'complete' }
    expect(largeFileReport(analysis).reported).toBe(false)
    expect(largeFileReport({ ...analysis, large_files: [] })).toEqual({
      reported: true,
      files: [],
      unreadable: 0,
    })
    const a = file('/a')
    expect(
      largeFileReport({ ...analysis, large_files: [a, a, null, { size: 3 }] as LargeFile[] }),
    ).toEqual({ reported: true, files: [a, a], unreadable: 2 })
    expect(largeFileReport({ ...analysis, large_files: {} as LargeFile[] }).reported).toBe(false)
  })
  it('classifies only filename extensions, handles dotfiles and retains literal paths', () => {
    expect(fileKind(file('/dir.MP4/archive.TAR.GZ'))).toBe('archive')
    expect(fileKind(file('/dir.mp4/no-extension'))).toBe('other')
    expect(fileKind(file('/.mp4'))).toBe('other')
    expect(fileKind(file('/电影.MP4'))).toBe('video')
    expect(fileKind(file('/图片.HEIC'))).toBe('image')
    const path = "/root/中文\n[*] $(literal) 'quote'.pdf"
    expect(filterLargeFiles([file(path)], '$(literal)', 'document', 0, 'size')[0].path).toBe(path)
  })
  it('sorts unknown sizes last, distinguishes zero, and applies thresholds only to known returned sizes', () => {
    const files = [
      file('/unknown', -1),
      file('/zero', 0),
      file('/large', 1024 ** 3),
      file('/small', 100 * 1024 ** 2),
    ]
    for (const value of [-1, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, 1.5])
      expect(largeFileBytes(file('/bad', value))).toBeNull()
    expect(largeFileBytes(files[1])).toBe(0)
    expect(filterLargeFiles(files, '', 'all', 0, 'size').map((f) => f.name)).toEqual([
      'large',
      'small',
      'zero',
      'unknown',
    ])
    expect(filterLargeFiles(files, '', 'all', 100 * 1024 ** 2, 'size').map((f) => f.name)).toEqual([
      'large',
      'small',
    ])
    expect(filterLargeFiles(files, '', 'all', 1024 ** 3, 'name').map((f) => f.name)).toEqual([
      'large',
    ])
    expect(files[0].name).toBe('unknown')
  })
  it('checks full path boundaries before read-only actions without treating sibling prefixes as descendants', () => {
    expect(largeFileInScope('/root/deep/a', '/root/')).toBe(true)
    expect(largeFileInScope('/a', '/')).toBe(true)
    expect(largeFileInScope('/root/[*] $(literal)\n名', '/root')).toBe(true)
    for (const path of [
      '/root',
      '/root/',
      '/root-other/a',
      '/root/../escape',
      '/root/./a',
      'relative',
      '/root/null\0a',
    ])
      expect(largeFileInScope(path, '/root')).toBe(false)
  })
})
