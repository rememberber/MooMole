import type { ProjectActivity as Observation } from '../lib/project-activity'
import { dateTime } from '../lib/format'
export function ProjectActivity({ report }: { report?: Observation }) {
  return (
    <div className="project-activity" role="group" aria-label="近期修改观察">
      <p>
        {report?.state === 'recent' ? '近 7 天观察到修改，请先保留。' : '近期修改情况仍未确认。'}
      </p>
      {report?.evidence ? (
        <>
          <p className="project-artifact-path">
            {report.evidence.kind === 'directory' ? '目录' : '文件'}：{report.evidence.path}
          </p>
          <p>系统报告修改时间 {dateTime(report.evidence.modifiedAt)}</p>
        </>
      ) : (
        <p>
          {!report
            ? '重新测量尚未得到有效观察；之前的近期修改线索已撤回。'
            : report.reason === 'notObserved'
              ? '本次未观察到近 7 天修改，不代表没有活动构建或占用。'
              : report.reason === 'incomplete'
                ? '测量未覆盖全部内容；无法确认近期修改情况。'
                : '时间、条目或路径无法确认；本次不发布修改线索。'}
        </p>
      )}
      <p>
        仅依据本次测量中的修改时间；时间可能被更改，也不能证明构建是否仍在进行。这不是清理许可。
      </p>
    </div>
  )
}
