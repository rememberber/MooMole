import type { ExecutionDetail } from '../types'
import { cleanupResultSizes, cleanupSpaceChange } from '../lib/cleanup-result'
import { bytes, dateTime } from '../lib/format'
import { getLocale } from '../i18n/locale'

export function CleanupResultSummary({ detail }: { detail: ExecutionDetail }) {
  const { selected, moved } = cleanupResultSizes(detail)
  const space = cleanupSpaceChange(detail)
  return (
    <section className="cleanup-result-summary" aria-label="清理容量结果">
      <dl>
        <div>
          <dt>本批所选大小</dt>
          <dd>
            {bytes(selected.knownBytes)}
            {selected.unknownCount ? ` + ${selected.unknownCount} 项大小未知` : ''}
          </dd>
        </div>
        <div>
          <dt>确认移入废纸篓</dt>
          <dd>
            {bytes(moved.knownBytes)}
            {moved.unknownCount ? ` + ${moved.unknownCount} 项大小未知` : ''}
          </dd>
        </div>
        <div>
          <dt>磁盘可用空间变化</dt>
          <dd>
            {space.code !== 'observed'
              ? '未取得可比较的观察'
              : space.delta > 0
                ? `增加 ${bytes(space.delta)}`
                : space.delta < 0
                  ? `减少 ${bytes(-space.delta)}`
                  : '未观察到变化'}
          </dd>
        </div>
      </dl>
      <p className="muted">
        项目大小沿用扫描时的估算。移入废纸篓不会立即释放空间；磁盘变化也可能来自其他程序、系统缓存或快照，不能归因于本次清理。
      </p>
      {space.code === 'volumeChanged' && (
        <p className="muted">前后观察的卷或容量已变化，未计算差值。</p>
      )}
      {space.code === 'timeChanged' && (
        <p className="muted">观察时间无法与本批处理核对，未计算差值。</p>
      )}
      {space.code === 'observed' && (
        <details>
          <summary>空间观察详情</summary>
          <p>
            处理前可用 {bytes(space.before.availableBytes)}（
            {space.before.availableBytes.toLocaleString(getLocale())} 字节） ·{' '}
            {dateTime(space.before.measuredAt)}
          </p>
          <p>
            处理后可用 {bytes(space.after.availableBytes)}（
            {space.after.availableBytes.toLocaleString(getLocale())} 字节） ·{' '}
            {dateTime(space.after.measuredAt)}
          </p>
          <p className="muted">
            观察当前用户主目录所在本地卷的文件系统可用块；不包含系统估算的可清除空间，也不等同于单个文件实际占用。
          </p>
        </details>
      )}
    </section>
  )
}
