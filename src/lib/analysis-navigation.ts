export interface AnalysisNavigationHistory {
  paths: string[]
  index: number
}

export interface AnalysisHistoryTarget {
  index: number
  path: string
}

const HISTORY_LIMIT = 32

export function directoryCrumbs(path: string): { name: string; path: string }[] {
  if (!path.startsWith('/')) return []
  const crumbs = [{ name: '/', path: '/' }]
  let current = ''
  for (const name of path.split('/').filter(Boolean)) {
    current += `/${name}`
    crumbs.push({ name, path: current })
  }
  return crumbs
}

// Call only after a successful task. The target belongs to that request, so
// failures/cancellation cannot move the cursor or discard forward visits.
export function recordAnalysisVisit(
  history: AnalysisNavigationHistory,
  path: string,
  target?: AnalysisHistoryTarget,
): AnalysisNavigationHistory {
  if (
    target &&
    Number.isInteger(target.index) &&
    target.index >= 0 &&
    history.paths[target.index] === target.path
  ) {
    const paths = [...history.paths]
    paths[target.index] = path
    return { paths, index: target.index }
  }
  if (history.paths[history.index] === path) return history
  const paths = [...history.paths.slice(0, history.index + 1), path].slice(-HISTORY_LIMIT)
  return { paths, index: paths.length - 1 }
}
