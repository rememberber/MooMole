import { ArrowLeft, ArrowRight, ArrowUp } from 'lucide-react'
import type { AnalysisNavigationHistory } from '../lib/analysis-navigation'
import { parentDirectory } from '../lib/analysis'

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
  return (
    <div className="analysis-history-buttons" role="group" aria-label="分析目录导航">
      <button
        className="icon-button"
        aria-label="返回上次分析的目录"
        disabled={disabled || history.index <= 0}
        title={
          history.paths[history.index - 1] ? `返回：${history.paths[history.index - 1]}` : '返回'
        }
        onClick={() => onVisit(history.index - 1)}
      >
        <ArrowLeft size={16} />
      </button>
      <button
        className="icon-button"
        aria-label="前进到下一次分析的目录"
        disabled={disabled || history.index + 1 >= history.paths.length}
        title={
          history.paths[history.index + 1] ? `前进：${history.paths[history.index + 1]}` : '前进'
        }
        onClick={() => onVisit(history.index + 1)}
      >
        <ArrowRight size={16} />
      </button>
      <button
        className="icon-button"
        aria-label="上一级"
        title={`上一级：${parentDirectory(path)}`}
        disabled={disabled || path === '/'}
        onClick={() => onAnalyze(parentDirectory(path))}
      >
        <ArrowUp size={16} />
      </button>
    </div>
  )
}
