import { X } from 'lucide-react'
import { analysisTaskActive, type AnalysisTask } from '../lib/analysis-task'

const titles: Record<AnalysisTask['state'], string> = {
  starting: '正在启动空间分析',
  running: '空间分析进行中',
  cancelling: '正在取消空间分析',
  cancelled: '空间分析已取消',
  failed: '空间分析未完成',
  interrupted: '空间分析已中断',
  unavailable: '分析结果未能显示',
  unconfirmed: '分析任务状态待确认',
}

export function AnalysisTaskNotice({
  task,
  hasResult,
  canRetry,
  showResults,
  onRetry,
  onHistory,
  onResults,
  onDismiss,
}: {
  task: AnalysisTask
  hasResult: boolean
  canRetry: boolean
  showResults: boolean
  onRetry: () => void
  onHistory: () => void
  onResults: () => void
  onDismiss: () => void
}) {
  const active = analysisTaskActive(task)
  const problem = !active && task.state !== 'cancelled'
  return (
    <section
      className="analysis-task notice motion-scan"
      data-scanning={active && task.state !== 'cancelling'}
      aria-label="空间分析任务"
    >
      <div className="analysis-task-body">
        <div role={problem ? 'alert' : 'status'}>
          <strong className="motion-text" data-active={active && task.state !== 'cancelling'}>
            {titles[task.state]}
          </strong>
          <p className="mono analysis-task-path">
            请求目录：{task.path ?? '任务未记录目录，请从历史核对'}
          </p>
          {active && (
            <p>
              {task.state === 'cancelling'
                ? '已请求停止，正在等待本机任务确认；任务可能先完成。'
                : '正在等待 Mole 返回结果；完成比例和剩余时间未知。'}
            </p>
          )}
          {!active && task.state !== 'cancelled' && <p>{task.detail}</p>}
          {!active && task.diagnostic && (
            <details key={`${task.id}:${task.diagnostic}`} className="analysis-boundaries">
              <summary>查看格式诊断</summary>
              <p className="mono">{task.diagnostic}</p>
            </details>
          )}
          {task.state === 'unconfirmed' && <p>本机任务可能仍在运行，请先从操作历史继续跟踪。</p>}
          <p>
            {hasResult
              ? active
                ? '上次成功结果仍保留，本次成功后才会替换。'
                : '上次成功结果仍保留，本次未替换。'
              : '本次尚无可展示的新结果。'}
          </p>
        </div>
        {active && <progress aria-label="空间分析进度，完成比例未知" />}
        {task.cancelError && <p role="alert">取消请求未成功，仍在跟踪任务：{task.cancelError}</p>}
        {!active && (
          <div className="analysis-task-actions">
            {task.path && task.state !== 'unconfirmed' && (
              <button className="button secondary compact" disabled={!canRetry} onClick={onRetry}>
                重新分析此目录
              </button>
            )}
            <button className="button secondary compact" onClick={onHistory}>
              查看操作历史
            </button>
            {showResults && hasResult && (
              <button className="button secondary compact" onClick={onResults}>
                查看上次结果
              </button>
            )}
          </div>
        )}
      </div>
      {!active && (
        <button className="icon-button" aria-label="关闭分析任务提示" onClick={onDismiss}>
          <X size={15} />
        </button>
      )}
    </section>
  )
}
