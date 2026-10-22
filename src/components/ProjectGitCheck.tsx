import { projectGitReasons, type ProjectGitObservation } from '../lib/project-git'
import { dateTime } from '../lib/format'
export function ProjectGitCheck({
  observation,
  checking,
  disabled,
  onCheck,
}: {
  observation?: ProjectGitObservation
  checking: boolean
  disabled: boolean
  onCheck: () => void
}) {
  const report = observation?.report
  return (
    <div className="project-git-check" role="group" aria-label="Git 索引记录观察">
      <p role="status">
        {checking
          ? '正在检查 Git 索引记录…'
          : report?.state === 'recorded'
            ? `Git 索引中仍有 ${report.records} 条记录，请先保留。`
            : report
              ? 'Git 记录仍未确认。'
              : observation?.attempted
                ? 'Git 记录本次未确认。'
                : 'Git 记录尚未检查。'}
      </p>
      {report?.state === 'recorded' && (
        <>
          <p className="project-artifact-path">仓库：{report.repository}</p>
          <p className="project-artifact-path">索引记录：{report.example}</p>
        </>
      )}
      {report?.state === 'unknown' && <p>{projectGitReasons[report.reason!]}</p>}
      {report && <p>上次检查 {dateTime(report.checkedAt)}</p>}
      {observation?.error && <p role="alert">{observation.error} 之前的 Git 记录观察已撤回。</p>}
      <p>
        仅只读索引，不运行 Git
        或钩子，不读取源码、不改仓库。记录也可能属于尚未提交或当前已不存在的路径；未找到记录也不代表可清理。
      </p>
      <button className="button secondary compact" disabled={disabled} onClick={onCheck}>
        {checking
          ? '正在检查 Git 记录…'
          : observation?.attempted
            ? '重新检查 Git 记录'
            : '检查 Git 记录'}
      </button>
    </div>
  )
}
