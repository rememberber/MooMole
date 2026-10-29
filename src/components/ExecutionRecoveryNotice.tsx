import type { ExecutionSummary } from '../types'

export function ExecutionRecoveryNotice({
  records,
  onReview,
}: {
  records: ExecutionSummary[]
  onReview: () => void
}) {
  const affected = records.filter(
    (record) =>
      record.reviewedAt == null && (record.state === 'interrupted' || record.uncertainCount > 0),
  )
  if (!affected.length) return null
  const uncertain = affected.reduce((total, record) => total + record.uncertainCount, 0)
  const moved = affected.reduce((total, record) => total + record.movedCount, 0)
  const hasMove = affected.some((record) => record.method === 'move')
  const movedFiles = affected
    .filter((record) => record.method === 'move')
    .reduce((total, record) => total + record.movedCount, 0)
  return (
    <section
      className="notice warning execution-recovery"
      aria-label={hasMove ? '文件处理中断与待核实结果' : '清理中断与待核实结果'}
    >
      <div>
        <h2>
          {hasMove
            ? uncertain > 0
              ? `有 ${uncertain} 项文件处理结果待核实`
              : `有 ${affected.length} 个文件处理批次中断`
            : uncertain > 0
              ? `有 ${uncertain} 项清理结果待核实`
              : `有 ${affected.length} 个清理批次中断`}
        </h2>
        <p>
          {hasMove
            ? `涉及 ${affected.length} 个批次，已记录移动 ${movedFiles} 项、移入废纸篓 ${moved - movedFiles} 项。`
            : `涉及 ${affected.length} 个批次，已记录移入废纸篓 ${moved} 项。`}
          已完成的结果会保留，未开始的项目不会自动继续。
          {uncertain > 0
            ? '请先查看逐项记录，核对待核实项目的实际文件状态。'
            : '其余项目以逐项记录为准，批次中断不会撤销已完成的操作。'}
        </p>
      </div>
      <button className="button secondary compact" onClick={onReview}>
        前往执行记录
      </button>
    </section>
  )
}
