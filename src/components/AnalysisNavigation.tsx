import { ArrowLeft, ArrowRight, ChevronRight } from 'lucide-react'
import { directoryCrumbs, type AnalysisNavigationHistory } from '../lib/analysis-navigation'

export function AnalysisNavigation({
  path,
  history,
  disabled,
  onVisit,
  onAnalyze,
}: {
  path: string
  history: AnalysisNavigationHistory
  disabled: boolean
  onVisit: (index: number) => void
  onAnalyze: (path: string) => void
}) {
  const crumbs = directoryCrumbs(path)
  return (
    <div className="analysis-navigation">
      <div className="analysis-history-buttons" role="group" aria-label="分析目录导航">
        <button
          className="button secondary compact"
          aria-label="返回上次分析的目录"
          disabled={disabled || history.index <= 0}
          title={history.paths[history.index - 1]}
          onClick={() => onVisit(history.index - 1)}
        >
          <ArrowLeft size={14} />
          返回
        </button>
        <button
          className="button secondary compact"
          aria-label="前进到下一次分析的目录"
          disabled={disabled || history.index + 1 >= history.paths.length}
          title={history.paths[history.index + 1]}
          onClick={() => onVisit(history.index + 1)}
        >
          <ArrowRight size={14} />
          前进
        </button>
      </div>
      <nav aria-label="分析目录路径">
        <ol>
          {crumbs.map((crumb, index) => (
            <li key={crumb.path}>
              {index > 0 && <ChevronRight size={12} aria-hidden="true" />}
              {index === crumbs.length - 1 ? (
                <span aria-current="location" title={path}>
                  {crumb.name}
                </span>
              ) : (
                <button
                  className="analysis-crumb"
                  disabled={disabled}
                  title={crumb.path}
                  aria-label={`分析目录 ${crumb.path}`}
                  onClick={() => onAnalyze(crumb.path)}
                >
                  {crumb.name}
                </button>
              )}
            </li>
          ))}
        </ol>
      </nav>
      <p>返回和前进会重新读取目录，成功后才更新位置。</p>
    </div>
  )
}
