import { Fragment, useEffect, useMemo, useState } from 'react'
import { FolderOpen } from 'lucide-react'
import * as api from '../lib/api'
import type { PersonalController } from '../lib/use-personal-files'
import { filterPersonalFiles, type PersonalConfirmation } from '../lib/personal-files'
import { fileKinds, type FileKind } from '../lib/large-files'
import { bytes, dateTime, shortPath } from '../lib/format'
import { pathOverlaps } from '../lib/clean-preview'
import { useNow } from '../lib/use-now'
import { useLanguage } from '../i18n/locale'
import { Modal } from './Modal'
import { CleanupResultSummary } from './CleanupResultSummary'
import { ExecutionReason } from './ExecutionReason'
import { ExecutionLocation } from './ExecutionLocation'
import { FilePreview } from './FilePreview'
import { VideoComparison } from './VideoComparison'
import { comparableVideo, type VideoTarget } from '../lib/video-comparison'
import type { PreviewTarget } from '../lib/file-preview'
import type { FileArchiveTarget } from '../lib/file-archive'
import { transferTransport, type TransferTarget } from '../lib/file-transfer'
import {
  defaultBrowserProfile,
  downloadBrowsers,
  sameBrowserProfile,
  type BrowserProfile,
} from '../lib/browser-downloads'

export function PersonalFiles({
  controller: c,
  home,
  protectedPaths,
  enabled,
  onHistory,
  onArchive,
  onTransfer,
  mode = 'personal',
  softwareSource,
  playlistPath,
  exportPath,
  browserProfile,
}: {
  controller: PersonalController
  home: string
  protectedPaths: string[]
  enabled: boolean
  onHistory: () => void
  onArchive?: (target: FileArchiveTarget) => void
  onTransfer?: (target: TransferTarget) => void
  mode?: 'personal' | 'duplicates' | 'similar' | 'software' | 'downloads' | 'offline' | 'chat'
  softwareSource?: string
  playlistPath?: string
  exportPath?: string
  browserProfile?: BrowserProfile | null
}) {
  const [preview, setPreview] = useState<PreviewTarget | null>(null)
  const [comparisonIds, setComparisonIds] = useState<string[]>([])
  const [comparison, setComparison] = useState<VideoTarget[] | null>(null)
  const [focusOnly, setFocusOnly] = useState(false)
  const { photoSequences, setPhotoSequences } = c
  const language = useLanguage()
  const software =
    mode === 'software' || mode === 'downloads' || mode === 'offline' || mode === 'chat'
  const [query, setQuery] = useState(''),
    [kind, setKind] = useState<FileKind>('all')
  const [minimum, setMinimum] = useState(0),
    [older, setOlder] = useState(0),
    [sort, setSort] = useState<'size' | 'name' | 'modified'>('size')
  const [selected, setSelected] = useState<string[]>([]),
    [limit, setLimit] = useState(100)
  const [confirmation, setConfirmation] = useState<PersonalConfirmation | null>(null)
  const [destination, setDestination] = useState('')
  const [conflict, setConflict] = useState<'skip' | 'keepBoth'>('skip')
  const [pending, setPending] = useState(false),
    [ack, setAck] = useState(false),
    [error, setError] = useState('')
  // Tick once per second, but use the current clock on async result renders.
  // A cached pre-scan tick must not momentarily mark a new report as expired.
  useNow()
  const now = Date.now(),
    report = c.snapshot.report
  const fresh =
    !!report &&
    c.root.trim().replace(/\/+$/, '') === report.root &&
    (mode !== 'similar' || !!report.similarPhotos?.sequence === photoSequences) &&
    (!softwareSource || report.softwareContent?.source === softwareSource) &&
    (mode !== 'offline' ||
      (!!playlistPath && report.softwareContent?.playlistPath === playlistPath)) &&
    (mode !== 'chat' || (!!exportPath && report.softwareContent?.exportPath === exportPath)) &&
    (mode !== 'downloads' ||
      sameBrowserProfile(
        browserProfile === undefined ? defaultBrowserProfile : browserProfile,
        report.softwareContent?.browserProfile ?? defaultBrowserProfile,
      )) &&
    now >= report.createdAt &&
    now < report.expiresAt
  const blocked = (path: string) => protectedPaths.some((rule) => pathOverlaps(path, rule))
  const actionable = report?.softwareContent?.executionSupported !== false
  const chosen =
    report?.items.filter(
      (item) => actionable && selected.includes(item.id) && !blocked(item.path) && fresh,
    ) ?? []
  const compared =
    report?.items.filter(
      (item) =>
        comparisonIds.includes(item.id) &&
        comparableVideo(item.path) &&
        !blocked(item.path) &&
        fresh,
    ) ?? []
  const previewDomain: PreviewTarget['domain'] = software
    ? 'softwareContent'
    : mode === 'similar'
      ? 'similarPhotos'
      : mode === 'duplicates'
        ? 'duplicateFiles'
        : 'personalFiles'
  const items = useMemo(
    () =>
      filterPersonalFiles(report?.items ?? [], query, kind, minimum, older, now, sort).filter(
        (i) =>
          !focusOnly || !report?.aiFocus || report.aiFocus.items.some((f) => f.itemId === i.id),
      ),
    [report, query, kind, minimum, older, now, sort, language, focusOnly],
  )
  const grouped = mode === 'duplicates' || mode === 'similar'
  const groups =
    mode === 'similar'
      ? (report?.similarPhotos?.groups ?? [])
      : mode === 'duplicates'
        ? (report?.duplicates?.groups ?? [])
        : []
  const displayItems = grouped
    ? groups
        .filter((g) => g.itemIds.some((id) => items.some((i) => i.id === id)))
        .flatMap((g) => g.itemIds.map((id) => report!.items.find((i) => i.id === id)!))
    : items
  const title =
    mode === 'offline'
      ? '确认处理离线影音文件'
      : mode === 'chat'
        ? '确认处理聊天导出附件'
        : mode === 'downloads'
          ? '确认处理浏览器下载'
          : mode === 'software'
            ? '确认处理软件附件副本'
            : mode === 'similar'
              ? photoSequences
                ? '确认处理连续拍摄候选'
                : '确认处理相似照片'
              : mode === 'duplicates'
                ? '确认处理重复文件'
                : '确认处理个人文件'
  useEffect(() => {
    setSelected([])
    setLimit(100)
    setPreview(null)
    setComparisonIds([])
    setComparison(null)
    setFocusOnly(!!report?.aiFocus)
  }, [report?.id])
  useEffect(() => {
    setSelected([])
    setComparisonIds([])
    setComparison(null)
    setPreview(null)
  }, [
    c.root,
    softwareSource,
    playlistPath,
    exportPath,
    browserProfile?.browser,
    browserProfile?.profile,
    photoSequences,
  ])
  useEffect(() => {
    if (!fresh || c.running) setComparison(null)
  }, [fresh, c.running])
  useEffect(() => {
    setLimit(100)
  }, [query, kind, minimum, older, sort])
  useEffect(
    () => () => {
      if (confirmation) void c.discard(confirmation.id).catch(() => {})
    },
    [confirmation?.id],
  )
  async function prepare(moving = false) {
    if (pending || !chosen.length || !fresh) return
    setPending(true)
    setError('')
    try {
      const directory = destination.trim().replace(/\/+$/, '')
      if (
        moving &&
        onTransfer &&
        report &&
        (await transferTransport.strategy(
          report.id,
          chosen.map((i) => i.id),
          directory,
        )) === 'crossVolume'
      ) {
        onTransfer({
          scanId: report.id,
          root: report.root,
          items: chosen,
          destination: directory,
          conflict,
        })
        return
      }
      setConfirmation(
        await c.prepare(
          chosen.map((item) => item.id),
          moving ? { directory: destination.trim().replace(/\/+$/, ''), conflict } : undefined,
        ),
      )
      setAck(false)
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
    } finally {
      setPending(false)
    }
  }
  const close = () => {
    setConfirmation(null)
    setAck(false)
  }
  return (
    <div className="personal-files">
      <FilePreview target={preview} onClose={() => setPreview(null)} />
      <VideoComparison
        targets={comparison}
        onClose={() => setComparison(null)}
        onChoose={(ids) => {
          if (!comparison || !fresh || c.running) return '当前扫描已失效，请关闭对比后重新扫描。'
          const pool = new Set(comparison.map((t) => t.itemId))
          const allowed = ids.filter(
            (id) => pool.has(id) && report?.items.some((i) => i.id === id && !blocked(i.path)),
          )
          if (allowed.length !== ids.length)
            return '文件选择或保护范围已变化，请关闭对比后重新扫描。'
          const next = [
            ...chosen.filter((item) => !pool.has(item.id)).map((item) => item.id),
            ...allowed,
          ]
          if (next.length > 20) return '每批最多选择 20 项，请减少本次对比中的处理选择。'
          if (groups.some((g) => g.itemIds.every((id) => next.includes(id))))
            return '每组至少保留一份，请调整视频选择。'
          setSelected(next)
        }}
      />
      <form
        id="personal-files-form"
        className="personal-scope"
        onSubmit={(event) => {
          event.preventDefault()
          if (enabled && !c.running) void c.start()
        }}
      >
        <label className="personal-path">
          <span>整理目录</span>
          <input
            aria-label={
              mode === 'offline'
                ? '离线影音文件范围'
                : mode === 'chat'
                  ? '聊天导出附件目录'
                  : mode === 'downloads'
                    ? '下载文件范围'
                    : mode === 'software'
                      ? '软件附件目录'
                      : mode === 'similar'
                        ? '相似照片目录'
                        : mode === 'duplicates'
                          ? '重复文件目录'
                          : '个人文件目录'
            }
            value={
              mode === 'chat' ? (exportPath?.slice(0, exportPath.lastIndexOf('/')) ?? '') : c.root
            }
            disabled={c.running}
            readOnly={software}
            onChange={(event) => c.setRoot(event.target.value)}
          />
        </label>
        {mode === 'similar' && (
          <label className="personal-comparison-mode">
            <span>照片比较方式</span>
            <select
              aria-label="照片比较方式"
              value={photoSequences ? 'sequence' : 'visual'}
              disabled={c.running || pending}
              onChange={(e) => setPhotoSequences(e.target.value === 'sequence')}
            >
              <option value="visual">视觉相似照片</option>
              <option value="sequence">连续拍摄候选</option>
            </select>
          </label>
        )}
        {!software && (
          <>
            <button
              type="button"
              className="button secondary compact"
              disabled={!api.desktop || c.running}
              onClick={async () => {
                const root = await api.chooseDirectory('选择要整理的个人目录')
                if (root) c.setRoot(root)
              }}
            >
              <FolderOpen size={14} />
              选择目录
            </button>
            <button
              type="button"
              className="button text compact"
              disabled={c.running}
              onClick={() => c.setRoot(`${home}/Downloads`)}
            >
              下载目录
            </button>
          </>
        )}
      </form>
      <p className="muted">
        {mode === 'offline'
          ? '选择 VLC 导出的 M3U8 播放列表，找出其中的本地影音文件，默认不选。文件可能是唯一原件，无法保证可重新下载；移走后原播放列表可能无法继续播放。仅处理你选择的普通文件，播放列表及播放器数据库保持原样；流媒体、未列入的文件与未支持的格式保留。可预览部分普通影音，播放取决于系统编码支持。'
          : mode === 'chat'
            ? '只根据所选 Telegram Desktop JSON 导出清单定位包内附件，默认不选，不扫描软件内部资料库。导出附件可能是唯一副本，无法保证可重新下载；移走会使导出包中的对应链接失效。聊天正文清单、未列入文件和未知格式保留，不影响线上聊天或发送删除消息；先预览并关闭相关文件，再逐项确认。'
            : mode === 'downloads'
              ? '仅根据所选浏览器配置中已完成的下载记录定位普通文件，不遍历整个账户目录。下载记录仅提供来源线索，不证明当前内容仍与原下载相同、可重新下载或可以删除；默认不选。只处理所选本地文件，浏览器历史和下载记录保持原样。'
              : mode === 'software'
                ? '这些是 Mail 打开或下载的附件副本，可能含有尚未另存的编辑内容，不能视为可重建缓存。仅列出超过 30 个完整天数的普通文件；默认不选。处理前请正常退出 Mail，核对内容和原邮件是否仍可取得。'
                : mode === 'similar'
                  ? '按本机视觉特征寻找相似照片，默认不选。相似不等于相同，可能包含不同表情、细节或唯一内容；请并排比较并自行选择，每组至少保留一张。连续拍摄候选还需要照片内的拍摄时间与相机线索，不代表已确认连拍关系。只比较普通目录中的静态图片，不进入照片图库或处理视频。'
                  : mode === 'duplicates'
                    ? '按文件内容核对并分组，默认不选，每组至少保留一份。相同内容在不同位置仍可能各有用途，请核对路径后选择；文件夹、应用资料库与相似但不同的照片不在本轮范围。'
                    : '按内容需要自行选择个人文件，默认不选。文件较大或久未修改不代表可以删除；本轮处理普通文件，文件夹和应用资料库保留。'}
      </p>
      {mode === 'similar' && (
        <details>
          <summary>相似照片扫描范围</summary>
          <p className="muted">
            支持 JPEG、PNG、HEIC/HEIF、WebP、TIFF、BMP 静态图片；每张最多 32
            MiB、八千万像素。连续拍摄候选限定同一父目录、相同相机与日期/时区线索、距本组最早照片两秒以内，并核对视觉相似。元数据可能错误，缺失时不按文件名或修改时间推测；未知时区不当成本地时区。照片图库、RAW、动态图、权威连拍关系与视频保留。达到数量、读取或时间上限时，结果会标记为未完整覆盖。
          </p>
        </details>
      )}
      {c.error && (
        <p className="notice warning" role="alert">
          {c.error}
        </p>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      {c.running && (
        <p role="status">
          {c.snapshot.phase === 'processing'
            ? '正在逐项处理个人文件，结果会保存到历史。'
            : '正在读取目录，可停止扫描并保留已取得的结果。'}
        </p>
      )}
      {report && (
        <>
          {fresh && report.aiFocus && (
            <section className="notice" aria-label="AI 关注项说明">
              <p>
                已根据你带入的 AI 线索重新扫描，所有处理仍默认不选。AI
                说明可能有误，不是删除依据；需要自行预览、选择并单独确认。
              </p>
              <p className="muted">
                分析于 {dateTime(report.aiFocus.analyzedAt)} · {report.aiFocus.agentId}
                。本轮说明仅随扫描保留在内存，退出后失效，不写入执行历史。
              </p>
              <label>
                <input
                  type="checkbox"
                  checked={focusOnly}
                  onChange={(e) => setFocusOnly(e.target.checked)}
                />
                只显示本轮 AI 关注项
              </label>
            </section>
          )}
          <div className="personal-summary">
            <span className="mono">{shortPath(report.root, home)}</span>
            <span>
              {report.items.length} 个普通文件 ·{' '}
              {bytes(report.items.reduce((sum, item) => sum + item.size, 0))} ·{' '}
              {dateTime(report.createdAt)}
            </span>
          </div>
          {report.softwareContent && !report.softwareContent.executionSupported && (
            <p className="notice warning">
              此 Mail 容器位置受到 Mole 核心规则保护，仅供预览；附件与目录保持原样，不提供处理确认。
            </p>
          )}
          {report.softwareContent && !report.softwareContent.minimumSizeMet && (
            <p className="notice">
              当前已核对的普通文件未达到 Mole 默认 5 MiB
              目录门槛，本轮不提供附件处理；未访问的范围仍然未知。
            </p>
          )}
          {report.similarPhotos && (
            <p className="notice">
              {report.similarPhotos.sequence ? (
                <>
                  已核对 {report.similarPhotos.checkedFiles} 张静态图片，找到 {groups.length}{' '}
                  组连续拍摄候选；{report.similarPhotos.sequence.missingMetadata}{' '}
                  张缺少可用拍摄线索。组内按拍摄时间排列，仅供人工比较，不自动挑选最佳帧或认定可删除。
                </>
              ) : (
                <>
                  已核对 {report.similarPhotos.checkedFiles} 张静态图片，找到 {groups.length}{' '}
                  组视觉相似候选。结果仅供人工比较，不自动挑选或认定可删除；本轮最多核对 200 张。
                </>
              )}
            </p>
          )}
          {report.duplicates && (
            <p className="notice">
              已核对 {report.duplicates.checkedFiles} 个文件，找到 {groups.length}{' '}
              组相同内容。每组保留一份后的额外副本合计 {bytes(report.duplicates.extraBytes)}
              ，不等于可立即释放空间。筛选匹配组，组内副本一起显示。
            </p>
          )}
          {(!report.complete || report.skipped > 0) && (
            <p className="notice">
              {report.cancelled ? '扫描已停止。' : '本次扫描范围有限。'} 已跳过 {report.skipped}{' '}
              项；隐藏、链接、受保护、共享、云占位、其他卷或无法读取的内容未纳入。已显示项目不代表目录全部内容。
            </p>
          )}
          {!fresh && (
            <p className="notice warning">
              {mode === 'similar' && !!report.similarPhotos?.sequence !== photoSequences
                ? '比较方式已变化，请重新扫描后选择。'
                : '扫描已过期或整理目录已变化，请重新扫描后选择。'}
            </p>
          )}
          <div className="personal-filters">
            <input
              type="search"
              aria-label="搜索个人文件"
              placeholder="搜索名称或路径…"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <select
              aria-label="个人文件类型"
              value={kind}
              onChange={(event) => setKind(event.target.value as FileKind)}
            >
              {Object.entries(fileKinds).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
            <select
              aria-label="个人文件最小大小"
              value={minimum}
              onChange={(event) => setMinimum(Number(event.target.value))}
            >
              <option value={0}>全部大小</option>
              <option value={100 * 1024 ** 2}>至少 100 MB</option>
              <option value={1024 ** 3}>至少 1 GB</option>
            </select>
            <select
              aria-label="个人文件修改时间"
              value={older}
              onChange={(event) => setOlder(Number(event.target.value))}
            >
              <option value={0}>全部修改时间</option>
              <option value={30}>30 天前修改</option>
              <option value={90}>90 天前修改</option>
              <option value={365}>一年前修改</option>
            </select>
            <select
              aria-label="个人文件排序"
              value={sort}
              onChange={(event) => setSort(event.target.value as typeof sort)}
            >
              <option value="size">大小从大到小</option>
              <option value="modified">最早修改优先</option>
              <option value="name">按名称</option>
            </select>
          </div>
          <div className="personal-selection">
            {(mode === 'personal' || mode === 'duplicates' || software) &&
              report.items.some((item) => comparableVideo(item.path)) && (
                <>
                  <button
                    className="button secondary compact"
                    disabled={
                      !enabled ||
                      c.running ||
                      pending ||
                      compared.length < 2 ||
                      compared.length > 4 ||
                      !fresh
                    }
                    onClick={() => {
                      setPreview(null)
                      setComparison(
                        compared.map((item) => ({
                          domain: previewDomain,
                          scanId: report.id,
                          itemId: item.id,
                          path: item.path,
                          name: item.name,
                          size: item.size,
                        })),
                      )
                    }}
                  >
                    并排对比 {compared.length} 段视频
                  </button>
                  {comparisonIds.length > 0 && (
                    <button
                      className="button text compact"
                      disabled={pending || c.running}
                      onClick={() => setComparisonIds([])}
                    >
                      清空对比
                    </button>
                  )}
                </>
              )}
            <strong>
              已选 {chosen.length} 项 · {bytes(chosen.reduce((sum, item) => sum + item.size, 0))}
            </strong>
            <span>每批最多 20 项</span>
            {mode === 'personal' && onArchive && (
              <button
                className="button secondary compact"
                disabled={!enabled || c.running || !fresh || !chosen.length || pending}
                onClick={() => onArchive({ scanId: report.id, root: report.root, items: chosen })}
              >
                归档为 ZIP…
              </button>
            )}
            {!grouped && (
              <button
                className="button secondary compact"
                disabled={
                  !enabled ||
                  c.running ||
                  !fresh ||
                  !chosen.length ||
                  !destination.trim() ||
                  pending
                }
                onClick={() => void prepare(true)}
              >
                移动到目录…
              </button>
            )}
            <button
              className="button secondary compact"
              disabled={!selected.length || pending || c.running}
              onClick={() => setSelected([])}
            >
              清空选择
            </button>
            <button
              className="button primary compact"
              disabled={!enabled || c.running || !fresh || !chosen.length || pending}
              onClick={() => void prepare()}
            >
              {pending ? '正在准备…' : '移入废纸篓…'}
            </button>
          </div>
          {!grouped && !software && (
            <>
              <div className="personal-scope">
                <label className="personal-path">
                  <span>移动到</span>
                  <input
                    aria-label="个人文件移动目标"
                    value={destination}
                    disabled={pending || c.running}
                    placeholder="选择目标目录后审阅移动计划"
                    onChange={(event) => setDestination(event.target.value)}
                  />
                </label>
                <button
                  type="button"
                  className="button secondary compact"
                  disabled={!api.desktop || pending || c.running}
                  onClick={async () => {
                    const dir = await api.chooseDirectory('选择个人文件移动目标')
                    if (dir) setDestination(dir)
                  }}
                >
                  <FolderOpen size={14} />
                  选择目标目录
                </button>
                <select
                  aria-label="移动重名处理"
                  value={conflict}
                  disabled={pending || c.running}
                  onChange={(event) => setConflict(event.target.value as typeof conflict)}
                >
                  <option value="skip">重名时跳过</option>
                  <option value="keepBoth">保留两份并编号</option>
                </select>
              </div>
              <p className="muted">
                同一磁盘直接移动；其他磁盘会单独确认复制校验和原件废纸篓处理。目标被占用时不覆盖、不重新编号。
              </p>
            </>
          )}
          <div
            className={`card personal-list ${mode === 'similar' ? 'similar-photo-list' : ''}`}
            role="region"
            aria-label="个人文件列表"
          >
            {displayItems.slice(0, limit).map((item, index) => {
              const group = groups.find((g) => g.itemIds.includes(item.id))
              const photo = report?.similarPhotos?.images.find((p) => p.id === item.id)
              const lastCopy =
                !!group &&
                group.itemIds.filter((id) => !chosen.some((i) => i.id === id)).length <= 1 &&
                !chosen.some((i) => i.id === item.id)
              const first =
                group && (index === 0 || !group.itemIds.includes(displayItems[index - 1].id))
              return (
                <Fragment key={item.id}>
                  {first && (
                    <header className="duplicate-group">
                      <strong>
                        {mode === 'similar' ? (
                          report.similarPhotos?.sequence ? (
                            `连续拍摄候选 · ${group.itemIds.length} 张`
                          ) : (
                            `视觉相似 · ${group.itemIds.length} 张`
                          )
                        ) : (
                          <>
                            相同内容 · {group.itemIds.length} 份 · 每份{' '}
                            {bytes(
                              'size' in group && typeof group.size === 'number' ? group.size : 0,
                            )}
                          </>
                        )}
                      </strong>
                      <span>
                        {mode === 'similar' ? '每组至少保留一张，内容可能不同' : '每组至少保留一份'}
                      </span>
                      {'captureSpanMs' in group && typeof group.captureSpanMs === 'number' && (
                        <small>拍摄跨度 {group.captureSpanMs} 毫秒</small>
                      )}
                    </header>
                  )}
                  <article
                    className={`personal-row ${mode === 'similar' ? 'similar-photo-row' : ''}`}
                  >
                    <input
                      type="checkbox"
                      aria-label={`选择 ${item.path}`}
                      checked={chosen.some((value) => value.id === item.id)}
                      disabled={
                        !enabled ||
                        !actionable ||
                        !fresh ||
                        blocked(item.path) ||
                        c.running ||
                        pending ||
                        lastCopy ||
                        (!selected.includes(item.id) && chosen.length >= 20)
                      }
                      onChange={(event) =>
                        setSelected((ids) =>
                          event.target.checked
                            ? [...ids, item.id]
                            : ids.filter((id) => id !== item.id),
                        )
                      }
                    />
                    {mode === 'similar' && photo && (
                      <img
                        className="similar-photo-thumbnail"
                        src={photo.thumbnail}
                        alt={item.name}
                        data-i18n="off"
                      />
                    )}
                    <div>
                      <strong data-i18n="off">{item.name}</strong>
                      <p className="mono full-path">{shortPath(item.path, home)}</p>
                      {photo?.capturedAt && (
                        <p className="muted">
                          拍摄时间：<span data-i18n="off">{photo.capturedAt}</span>
                        </p>
                      )}
                      <small>
                        {blocked(item.path)
                          ? '已保护'
                          : grouped
                            ? chosen.some((i) => i.id === item.id)
                              ? mode === 'similar'
                                ? '所选照片'
                                : '所选副本'
                              : '将保留'
                            : '个人文件 · 需自行确认'}{' '}
                        · {dateTime(item.modifiedAt)}
                      </small>
                      {report.aiFocus?.items.find((f) => f.itemId === item.id) && (
                        <details className="ai-details">
                          <summary>查看当时的 AI 说明</summary>
                          <p>
                            {report.aiFocus.items.find((f) => f.itemId === item.id)
                              ?.recommendation === 'keep'
                              ? 'AI 建议保留；整理不代表可以删除。'
                              : 'AI 建议核实；尚不能确认可删除。'}
                          </p>
                          <p data-i18n="off">
                            {report.aiFocus.items.find((f) => f.itemId === item.id)?.reason}
                          </p>
                          <p data-i18n="off">
                            {report.aiFocus.items.find((f) => f.itemId === item.id)?.impact}
                          </p>
                        </details>
                      )}
                    </div>
                    <span>
                      {photo && (
                        <>
                          {photo.width} × {photo.height} 像素 ·{' '}
                        </>
                      )}
                      {bytes(item.size)}
                    </span>
                    <div className="personal-preview-actions">
                      <button
                        className="button secondary compact"
                        disabled={!enabled || !fresh || blocked(item.path) || c.running || pending}
                        onClick={() =>
                          setPreview({
                            domain: software
                              ? 'softwareContent'
                              : mode === 'similar'
                                ? 'similarPhotos'
                                : mode === 'duplicates'
                                  ? 'duplicateFiles'
                                  : 'personalFiles',
                            scanId: report!.id,
                            itemId: item.id,
                            path: item.path,
                          })
                        }
                      >
                        预览内容
                      </button>
                      {comparableVideo(item.path) && mode !== 'similar' && (
                        <button
                          className="button secondary compact"
                          aria-pressed={comparisonIds.includes(item.id)}
                          disabled={
                            !enabled ||
                            !fresh ||
                            blocked(item.path) ||
                            c.running ||
                            pending ||
                            (!comparisonIds.includes(item.id) && compared.length >= 4)
                          }
                          onClick={() =>
                            setComparisonIds((ids) =>
                              ids.includes(item.id)
                                ? ids.filter((id) => id !== item.id)
                                : [...ids, item.id],
                            )
                          }
                        >
                          {comparisonIds.includes(item.id) ? '移出视频对比' : '加入视频对比'}
                        </button>
                      )}
                    </div>
                    <button
                      className="button secondary compact"
                      disabled={!api.desktop}
                      onClick={() =>
                        void api.reveal(item.path).catch((reason) => setError(String(reason)))
                      }
                    >
                      在 Finder 中查看
                    </button>
                  </article>
                </Fragment>
              )
            })}
            {!items.length && (
              <p className="empty-inline">没有匹配的个人文件，可调整筛选或重新扫描。</p>
            )}
          </div>
          {displayItems.length > limit && (
            <button
              className="button secondary compact"
              onClick={() => setLimit((value) => value + 100)}
            >
              再显示 100 项
            </button>
          )}
        </>
      )}
      {!report && !c.running && !c.result && (
        <p className="empty-inline">
          {mode === 'chat'
            ? '选择 Telegram Desktop 导出的 result.json，再点击顶部“扫描聊天导出附件”。'
            : mode === 'downloads'
              ? '选择浏览器和配置目录，再点击顶部“扫描下载记录”。'
              : mode === 'offline'
                ? '选择播放列表，再点击顶部“扫描离线影音”。'
                : mode === 'software'
                  ? '点击顶部“扫描附件副本”，查看可审阅的 Mail 附件副本。'
                  : '选择下载目录或其他个人目录，再点击顶部“扫描文件”。'}
        </p>
      )}
      {c.result && (
        <section className="card personal-result" aria-label="个人文件处理结果">
          <strong>
            {c.result.method === 'move' ? '已移动' : '已移入'} {c.result.movedCount} /{' '}
            {c.result.itemCount} 项 · 待核实 {c.result.uncertainCount} 项
          </strong>
          <CleanupResultSummary detail={c.result} />
          {c.result.items.map((item) => (
            <article className="personal-result-row" key={item.id}>
              <strong>
                {item.state === 'moved'
                  ? c.result!.method === 'move'
                    ? '已移动到目标目录'
                    : '已移入废纸篓'
                  : item.state === 'uncertain'
                    ? '结果待核实'
                    : item.state === 'cancelled'
                      ? '开始前取消'
                      : item.state === 'skipped'
                        ? '已跳过'
                        : item.state === 'failed'
                          ? '失败，未作修改'
                          : item.state === 'pending'
                            ? '尚未开始'
                            : item.state === 'intent'
                              ? '等待完成证据'
                              : item.state === 'notStarted'
                                ? '中断后未开始'
                                : c.result!.method === 'move'
                                  ? '本项尚未确认移动'
                                  : '本项未确认移入'}
              </strong>
              <p className="mono full-path">{item.path}</p>
              {item.targetPath && <p className="mono full-path">→ {item.targetPath}</p>}
              <ExecutionReason reason={item.reason} />
              {item.receipt && (
                <ExecutionLocation
                  runId={c.result!.id}
                  itemId={item.id}
                  onReveal={api.revealExecutionItem}
                  method={c.result!.method}
                />
              )}
            </article>
          ))}
        </section>
      )}
      <button className="button secondary compact" onClick={onHistory}>
        查看处理历史
      </button>
      {!api.desktop && <p className="notice">浏览器预览不能读取或处理本机文件，请使用桌面 App。</p>}
      {confirmation && (
        <Modal title={title} onClose={pending ? undefined : close} className="review-plan-modal">
          <h2>{title}</h2>
          {mode === 'downloads' && (
            <p className="notice" data-i18n="off">
              {
                downloadBrowsers.find(
                  (b) =>
                    b.id ===
                    (report?.softwareContent?.browserProfile ?? defaultBrowserProfile).browser,
                )?.label
              }
              {' · '}
              {(report?.softwareContent?.browserProfile ?? defaultBrowserProfile).profile}
            </p>
          )}
          {mode === 'similar' && photoSequences && (
            <p className="notice">
              连续拍摄仅是元数据与视觉线索，不能证明照片可以互相替代。请核对所选帧与保留帧的表情、清晰度和独有内容；处理前仍会核验双方完整内容。
            </p>
          )}
          {mode === 'chat' && (
            <p className="notice">
              仅处理导出包中的所选附件，可能令导出链接失效；不发送线上聊天删除请求。导出正文清单保留，附件是否可重新取得未知。
            </p>
          )}
          <p>
            {confirmation.items.length} 个文件 ·{' '}
            {bytes(confirmation.items.reduce((sum, item) => sum + item.size, 0))}
          </p>
          <p className="notice warning">
            这些是你选择的个人文件，可能包含唯一副本或未同步内容。请核对内容并关闭相关文件；本次不完整检查所有应用占用，不能保证文件空闲。
          </p>
          {confirmation.movement ? (
            <p className="notice">
              只移动到下面确认的位置，不覆盖文件。重名时
              {confirmation.movement.conflict === 'keepBoth' ? '保留两份并编号' : '跳过'}
              ；确认后若名称被占用，本项失败并保留源文件。取消不会撤销已移动项。当前可从历史定位后在
              Finder 手动移回；也可在操作历史重新检查恢复原位置，不符合条件时保留。
            </p>
          ) : (
            <p>
              仅将下面列出的普通文件移入废纸篓，保留文件夹。处理前重新检查路径保护、权限和文件变化；不清空废纸篓。恢复需在
              Finder 中核对实际收据，原位置有同名文件时不要覆盖。
            </p>
          )}
          <div className="plan-items">
            {confirmation.items.map((item) => (
              <article className="plan-item" key={item.id}>
                <strong data-i18n="off">{item.name}</strong>
                <p className="mono full-path">{item.path}</p>
                {confirmation.movement &&
                  (confirmation.movement.targets.find((t) => t.id === item.id)?.destination ? (
                    <p className="mono full-path">
                      {confirmation.movement.targets.find((t) => t.id === item.id)?.destination}
                    </p>
                  ) : (
                    <p>跳过：目标重名或已在目标目录</p>
                  ))}
                <span>{bytes(item.size)}</span>
              </article>
            ))}
          </div>
          {confirmation.keptFiles && (
            <section className="notice">
              <strong>{mode === 'similar' ? '本批保留的照片' : '这批保留的副本'}</strong>
              {confirmation.keptFiles.map((item) => (
                <p className="mono full-path" key={item.id}>
                  {item.path}
                </p>
              ))}
              <p>
                {mode === 'similar'
                  ? '每张照片的内容单独核验，视觉相似不代表内容可以互相替代。保留照片变化或缺失时，本项不处理。'
                  : '执行前会重新核对所选文件与保留副本的内容；保留副本变化或缺失时，本项不处理。'}
              </p>
            </section>
          )}
          <label className="uninstall-ack">
            <input
              type="checkbox"
              checked={ack}
              onChange={(event) => setAck(event.target.checked)}
            />
            {confirmation.movement
              ? '我已核对源文件、目标路径和重名处理，并了解占用与恢复边界。'
              : '我已核对所选个人文件，确认不再需要，并了解占用与恢复边界。'}
          </label>
          <div className="modal-actions">
            <button className="button secondary" disabled={pending} onClick={close}>
              返回选择
            </button>
            <button
              className="button primary"
              disabled={
                !ack || pending || now >= confirmation.expiresAt || now < confirmation.createdAt
              }
              onClick={async () => {
                setPending(true)
                try {
                  await c.execute(confirmation.id, !!confirmation.movement)
                  close()
                } finally {
                  setPending(false)
                }
              }}
            >
              {confirmation.movement ? '确认移动文件' : '确认移入废纸篓'}
            </button>
          </div>
          {now >= confirmation.expiresAt && (
            <p className="notice warning">个人文件确认已过期，请返回重新检查。</p>
          )}
        </Modal>
      )}
    </div>
  )
}
