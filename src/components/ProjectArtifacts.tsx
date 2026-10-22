import { ProjectGitCheck } from './ProjectGitCheck'
import {
  projectArtifactView,
  type ProjectArtifactFilters,
  type ProjectMeasurements,
} from '../lib/project-artifact-view'
import type { ProjectRootsReport } from '../lib/project-roots'
import {
  projectCatalog,
  projectContextText,
  projectIncompleteReason,
  projectMeasurementReason,
  type ProjectArtifactsReport,
} from '../lib/project-artifacts'
import { bytes, dateTime } from '../lib/format'
export function ProjectArtifacts({
  report,
  filters,
  onFiltersChange,
  roots,
  rootsError,
  loadingRoots,
  root,
  reading,
  error,
  canAnalyze,
  measuring,
  checkingGit,
  onCheckGit,
  measurements,
  onMeasure,
  onAnalyze,
}: {
  filters: ProjectArtifactFilters
  onFiltersChange: (filters: ProjectArtifactFilters) => void
  roots: ProjectRootsReport | null
  rootsError: string
  loadingRoots: boolean
  report: ProjectArtifactsReport | null
  root: string
  reading: boolean
  error: string
  canAnalyze: boolean
  checkingGit: string
  onCheckGit: (path: string) => void
  measuring: string
  measurements: ProjectMeasurements
  onMeasure: (path: string) => void
  onAnalyze: (path: string) => void
}) {
  const view = projectArtifactView(report?.items ?? [], measurements, filters)
  const locked = reading || loadingRoots || !!measuring || !!checkingGit
  const clearFilters = () => onFiltersChange({ ...filters, query: '', context: '', page: 1 })
  return (
    <section aria-label="工程产物结果">
      {loadingRoots && (
        <p className="muted" role="status">
          正在读取 Mole 保存的目录；不会扫描这些目录…
        </p>
      )}
      {rootsError && (
        <p className="notice" role="alert">
          {rootsError} 可继续手动选择目录；先前的配置列表已撤回。
        </p>
      )}
      {roots && (
        <p className="muted" role="status">
          {roots.state === 'missing'
            ? 'Mole 尚未保存项目目录。可手动选择目录。'
            : roots.paths.length === 0
              ? 'Mole 保存的目录列表为空。可手动选择目录。'
              : `已读取 ${roots.paths.length} 处 Mole 目录；从顶部逐处选择后再扫描。目录是否可访问尚未检查。`}
        </p>
      )}

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
          {report.items.length > 0 && (
            <>
              <div className="project-filters" role="group" aria-label="工程产物列表筛选">
                <input
                  aria-label="筛选工程产物"
                  placeholder="名称或路径…"
                  maxLength={256}
                  value={filters.query}
                  disabled={locked}
                  onChange={(event) =>
                    onFiltersChange({ ...filters, query: event.target.value, page: 1 })
                  }
                />
                <select
                  aria-label="工程产物类型线索"
                  value={filters.context}
                  disabled={locked}
                  onChange={(event) =>
                    onFiltersChange({
                      ...filters,
                      context: event.target.value as ProjectArtifactFilters['context'],
                      page: 1,
                    })
                  }
                >
                  <option value="">全部类型线索</option>
                  <option value="nodeContext">Node 依赖线索</option>
                  <option value="rustContext">Rust 构建线索</option>
                  <option value="nameOnly">用途待确认</option>
                </select>
                <select
                  aria-label="工程产物组内排序"
                  value={filters.sort}
                  disabled={locked}
                  onChange={(event) =>
                    onFiltersChange({
                      ...filters,
                      sort: event.target.value as ProjectArtifactFilters['sort'],
                      page: 1,
                    })
                  }
                >
                  <option value="path">组内路径顺序</option>
                  <option value="size">组内已测占用从大到小</option>
                </select>
                {(filters.query || filters.context) && (
                  <button
                    className="button secondary compact"
                    disabled={locked}
                    onClick={clearFilters}
                  >
                    清除筛选
                  </button>
                )}
              </div>
              <p className="muted" role="status">
                显示 {view.start}–{view.end} / {view.total} 个匹配目录 · 本次共发现{' '}
                {report.items.length} 处
              </p>
              {filters.sort === 'size' && (
                <p className="muted">
                  仅比较组内已测文件占用，可能包含部分或上次测量；未测量、占用未确认排在组内末尾，不代表可释放空间。
                </p>
              )}
            </>
          )}
          {report.items.length === 0 ? (
            <div className="empty-state">
              <h3>本次未发现匹配目录</h3>
              <p className="muted">这里只查找固定 Mole 版本的产物名称，不代表没有工程产物。</p>
            </div>
          ) : view.total === 0 ? (
            <div className="empty-state">
              <h3>当前筛选没有匹配目录</h3>
              <p className="muted">可清除筛选查看本次发现的目录；筛选不会扩大扫描范围。</p>
            </div>
          ) : (
            <div className="project-artifact-list">
              {view.groups.map((group) => (
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
                          {measured?.report && (
                            <details className="origin-details project-content-signals">
                              <summary>
                                {measured.git?.report?.state === 'recorded'
                                  ? 'Git 索引中有记录，请先保留'
                                  : measured.signalsValid !== true
                                    ? '内容保护本次未确认'
                                    : measured.report.contentSignals.state === 'found'
                                      ? '发现需保留的名称线索'
                                      : '内容保护仍未确认'}
                              </summary>
                              {measured.signalsValid === true &&
                              measured.report.contentSignals.evidence ? (
                                <>
                                  <p>
                                    {measured.report.contentSignals.evidence.kind ===
                                    'repositoryMarker'
                                      ? '上次测量看到仓库标记名称 .git，请先保留。'
                                      : '上次测量看到密钥文件命名线索，请先保留；未判断文件内容。'}
                                  </p>
                                  <p className="project-artifact-path">
                                    {measured.report.contentSignals.evidence.path}
                                  </p>
                                </>
                              ) : (
                                <p>
                                  {measured.signalsValid !== true
                                    ? '重新测量尚未得到有效观察；之前的内容线索已撤回。'
                                    : measured.report.contentSignals.reason === 'notObserved'
                                      ? '本次测量未看到 .git 或 *-keypair.json 名称，不代表没有用户数据。'
                                      : measured.report.contentSignals.reason === 'incomplete'
                                        ? '测量未覆盖全部内容；无法确认是否存在这些名称。'
                                        : '名称或路径在复核时无法确认；本次不发布具体线索。'}
                                </p>
                              )}
                              <p>
                                只观察名称和元数据，不读取密钥或仓库文件内容、不运行 Git。Git
                                记录可在下方单独检查；其他用户数据、活动构建与完整 Mole
                                保护仍未确认。这不是清理许可，也不是持续监测。
                              </p>
                              <ProjectGitCheck
                                observation={measured.git}
                                checking={checkingGit === item.path}
                                disabled={!canAnalyze || locked}
                                onCheck={() => onCheckGit(item.path)}
                              />
                            </details>
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
                            disabled={!canAnalyze || reading || !!measuring || !!checkingGit}
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
                            disabled={!canAnalyze || reading || !!measuring || !!checkingGit}
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
          {view.pages > 1 && (
            <div className="project-pagination" role="group" aria-label="工程产物分页">
              <span className="muted">
                第 {view.page} / {view.pages} 页 · 每页最多 50 项
              </span>
              <button
                className="button secondary compact"
                disabled={locked || view.page <= 1}
                onClick={() => onFiltersChange({ ...filters, page: view.page - 1 })}
              >
                上一页
              </button>
              <button
                className="button secondary compact"
                disabled={locked || view.page >= view.pages}
                onClick={() => onFiltersChange({ ...filters, page: view.page + 1 })}
              >
                下一页
              </button>
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
          “读取 Mole 目录”只读取当前账户 ~/.config/mole/purge_paths：最多 64 KiB、256 行、64 处。
          识别空行、整行注释、绝对路径与 ~/；不运行 Shell、自动发现目录或校正路径大小写。
          配置中的目标在选择后仍需手动扫描，链接、权限和扫描范围继续由扫描端核对。
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
