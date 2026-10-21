import {
  projectCatalog,
  groupProjectArtifacts,
  projectContextText,
  projectIncompleteReason,
  projectMeasurementReason,
  type ProjectMeasurement,
  type ProjectArtifactsReport,
} from '../lib/project-artifacts'
import { bytes, dateTime } from '../lib/format'
export function ProjectArtifacts({
  report,
  root,
  reading,
  error,
  canAnalyze,
  measuring,
  measurements,
  onMeasure,
  onAnalyze,
}: {
  report: ProjectArtifactsReport | null
  root: string
  reading: boolean
  error: string
  canAnalyze: boolean
  measuring: string
  measurements: Record<string, { report?: ProjectMeasurement; error?: string }>
  onMeasure: (path: string) => void
  onAnalyze: (path: string) => void
}) {
  return (
    <section aria-label="工程产物结果">
      {error && (
        <p className="notice" role="alert">
          {error}
          {report && ' 以下保留上次结果，本次未更新。'}
        </p>
      )}
      {reading && (
        <p className="muted" role="status">
          正在所选目录中查找工程产物名称；不会进入产物内部或修改文件…
        </p>
      )}
      {!report ? (
        <div className="empty-state">
          <h3>{root ? '目录已选择' : '选择项目或项目所在目录'}</h3>
          <p className="muted">
            {root
              ? '点击顶部“扫描目录”查看命名匹配的产物目录。'
              : '从顶部选择目录开始，只读取所选范围，不自动扫描整个主目录。'}
          </p>
        </div>
      ) : (
        <>
          <p role="status">
            发现 {report.items.length} 处 Mole 命名目录 ·{' '}
            {report.completion === 'partial' ? '扫描不完整' : '本次命名查找结束'}
          </p>
          <p className="muted">
            上次扫描 {dateTime(report.checkedAt)}
            。按所选范围内的项目标记分组，仅为线索；可逐项测量文件占用。活动构建、源码/Git 和 Mole
            保护规则尚未检查，执行未开放。
          </p>
          {report.completion === 'partial' && (
            <p className="notice">
              本次未覆盖：{projectIncompleteReason(report)}。可缩小目录后重试，未显示不代表不存在。
            </p>
          )}
          {report.items.length === 0 ? (
            <div className="empty-state">
              <h3>本次未发现匹配目录</h3>
              <p className="muted">这里只查找固定 Mole 版本的产物名称，不代表没有工程产物。</p>
            </div>
          ) : (
            <div className="project-artifact-list">
              {groupProjectArtifacts(report.items).map((group) => (
                <section
                  className="project-group"
                  key={group.key}
                  aria-label={
                    group.hint.state === 'hint' ? `项目线索 ${group.hint.path}` : '归属待确认'
                  }
                >
                  <h3
                    className="project-group-heading"
                    title={
                      group.hint.state === 'hint'
                        ? `仅根据范围内标记：${group.hint.markers.join('、')}；不证明 Git 或内容归属。`
                        : undefined
                    }
                  >
                    <span>
                      {group.hint.state === 'hint'
                        ? group.hint.kind === 'workspace'
                          ? '工作区线索'
                          : '项目线索'
                        : '归属待确认'}
                    </span>
                    {group.hint.path && <span className="muted">{group.hint.path}</span>}
                  </h3>
                  {group.items.map((item) => {
                    const measured = measurements[item.path]
                    return (
                      <article
                        className="project-artifact-row"
                        key={item.path}
                        aria-label={item.path}
                      >
                        <div>
                          <div className="project-artifact-heading">
                            <strong>{item.name}</strong>
                            <span
                              className="muted"
                              title={
                                item.projectPath
                                  ? `相邻项目标记所在目录：${item.projectPath}`
                                  : undefined
                              }
                            >
                              {projectContextText[item.context]}
                            </span>
                          </div>
                          <span className="project-artifact-path">{item.path}</span>
                          <span>
                            {item.protection === 'matched'
                              ? '客户端保护命中，保持现状'
                              : item.protection === 'unknown'
                                ? '客户端保护无法确认，保持现状'
                                : '未命中客户端保护；其他清理条件未确认'}
                          </span>
                          {item.projectHint.state !== 'hint' && (
                            <span className="muted">
                              {item.projectHint.state === 'unknown'
                                ? '项目标记无法确认；保留未知'
                                : '所选范围内未找到项目标记'}
                            </span>
                          )}
                          {measured?.report && (
                            <span className="muted">
                              上次测量 {dateTime(measured.report.checkedAt)} ·{' '}
                              {measured.report.files} 个文件
                              {measured.report.completion === 'partial' &&
                                ` · ${projectMeasurementReason(measured.report)}`}
                            </span>
                          )}
                          {measured?.error && (
                            <span className="notice" role="alert">
                              {measured.error}
                              {measured.report && ' 保留上次测量，本次未更新。'}
                            </span>
                          )}
                        </div>
                        <div className="project-artifact-actions">
                          <span className="project-measurement">
                            <strong>
                              {measured?.report
                                ? measured.report.completion === 'partial' &&
                                  measured.report.files === 0
                                  ? '占用未确认'
                                  : bytes(measured.report.allocatedBytes)
                                : '未测量'}
                            </strong>
                            {measured?.report && (
                              <span className="muted">
                                {measured.report.completion === 'partial'
                                  ? '部分文件占用'
                                  : '已测文件占用'}
                              </span>
                            )}
                          </span>
                          <button
                            className="button secondary compact"
                            disabled={!canAnalyze || reading || !!measuring}
                            onClick={() => onMeasure(item.path)}
                          >
                            {measuring === item.path
                              ? '正在测量…'
                              : measured?.report
                                ? '重新测量'
                                : '测量占用'}
                          </button>
                          <button
                            className="button secondary compact"
                            disabled={!canAnalyze || reading || !!measuring}
                            onClick={() => onAnalyze(item.path)}
                          >
                            查看空间
                          </button>
                        </div>
                      </article>
                    )
                  })}
                </section>
              ))}
            </div>
          )}
        </>
      )}
      <details className="origin-details">
        <summary>扫描范围与来源</summary>
        <p>
          固定 Mole {projectCatalog.version} 的 purge 名称表，共 {projectCatalog.targets.length}{' '}
          类；只读查找，最多 6 层、10000 个子项、256 个结果，约 4
          秒预算。隐藏目录（匹配的产物名称除外）、Library、Applications 与产物内容不递归。
        </p>
        <p>
          相邻 package.json / Cargo.toml 仅提供类型线索，不证明可重建、Git
          归属或清理许可。没有计算可回收大小、判断七天保留规则、执行 purge 或改写 purge_paths。
        </p>
        <p>
          测量由用户逐项发起：最多 32 层、20000 个子项、约 3
          秒，仅统计普通单链接文件的系统已分配块（含隐藏文件），不读取文件内容。跳过链接、共享硬链接、特殊文件、跨文件系统及未下载内容，保留不完整原因。目录自身元数据不计入；APFS
          共享、压缩、稀疏文件可能使显示值不同于逻辑大小，不能据此预测释放空间。结果不是原子快照，也不证明归属或可重建。
        </p>
        <p>
          项目线索只检查所选范围内祖先的固定 Mole
          标记：优先最近的仓库/工作区标记，再取最近的项目清单。标记文件不解析内容，.git
          文件不跟随其指向，也不运行
          Git。链接、共享标记或无法读取时保留未知；不搜索所选范围外，分组不等于实际归属或可重建。
        </p>
        <p>查看空间会进入现有只读空间分析；分析结果也不代表可以清理。</p>
      </details>
    </section>
  )
}
