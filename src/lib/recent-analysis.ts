import type { TaskRecord } from '../types'

const encoder = new TextEncoder()

// The existing history query supplies newest-first records, at most 200.
// A recorded success is a navigation hint, not a fresh accessibility check.
export function recentAnalysisPaths(history: TaskRecord[]): string[] {
  const paths: string[] = []
  for (const record of history.slice(0, 200)) {
    const path = record.path
    // CR/LF are legal in filenames, but a single-line input strips them.
    // Keep those records in history rather than offering a lossy shortcut.
    if (
      record.kind !== 'analyze' ||
      record.state !== 'succeeded' ||
      typeof path !== 'string' ||
      !path.startsWith('/') ||
      path.includes('\0') ||
      /[\r\n]/.test(path) ||
      encoder.encode(path).length > 4096 ||
      (path !== '/' &&
        path
          .slice(1)
          .split('/')
          .some((part) => ['', '.', '..'].includes(part))) ||
      paths.includes(path)
    )
      continue
    paths.push(path)
    if (paths.length === 8) break
  }
  return paths
}
