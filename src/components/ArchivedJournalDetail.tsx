import { useState } from 'react'
import type { JournalKind, JournalRecord } from '../lib/archived-journals'
import type { MoleCleanRecord } from '../lib/mole-cleanup'
import type { ZipExtractionRecord } from '../lib/zip-extraction'
import type { ManagedRecord } from '../lib/managed-apps'
import { moleCleanScopes } from '../lib/mole-cleanup'
import { bytes, dateTime } from '../lib/format'
const eventNames: Record<string, string> = {
  intent: '操作意图',
  removed: '已移除',
  retained: '已保留',
  uncertain: '结果待核实',
  commandCompleted: '命令完成',
  section: '清理范围',
  upstream: '引擎记录',
  authorization: '授权记录',
  complete: '本轮结束',
}
export function ArchivedJournalDetail({
  kind,
  record,
}: {
  kind: JournalKind
  record: JournalRecord
}) {
  const [page, setPage] = useState(0),
    [raw, setRaw] = useState(false)
  return (
    <>
      <p>
        {dateTime(record.createdAt)} · {dateTime(record.finishedAt!)}
      </p>
      {record.reviewedAt !== null && (
        <p className="notice">已人工核查；原结果保持，核查不证明操作成功或已恢复。</p>
      )}
      {kind === 'moleClean'
        ? (() => {
            const r = record as MoleCleanRecord
            return (
              <>
                <p>完整清理沿用当时的 Mole 永久删除方式；归档不提供文件恢复。</p>
                <p className="mono full-path" data-i18n="off">
                  {r.plan.home}
                </p>
                <p>
                  Mole {r.plan.engineVersion} · {r.plan.system ? '包含系统范围' : '仅当前账户范围'}
                </p>
                <details>
                  <summary>当时的清理范围与保护路径</summary>
                  <ul>
                    {r.plan.scope.map((id) => (
                      <li key={id}>{moleCleanScopes.find(([key]) => key === id)?.[1]}</li>
                    ))}
                  </ul>
                  {r.plan.protectedPaths.map((path) => (
                    <p className="mono full-path" data-i18n="off" key={path}>
                      {path}
                    </p>
                  ))}
                  <p className="mono full-path" data-i18n="off">
                    {r.plan.revision}
                  </p>
                </details>
                <p>
                  当时可用空间：{r.beforeBytes === null ? '未测得' : bytes(r.beforeBytes)} →{' '}
                  {r.afterBytes === null ? '未测得' : bytes(r.afterBytes)}
                </p>
                {r.error && (
                  <p className="notice warning" data-i18n="off">
                    {r.error}
                  </p>
                )}
                <p>
                  完整事件 {r.events.length} 条 · 第 {page + 1} /{' '}
                  {Math.max(1, Math.ceil(r.events.length / 100))} 页
                </p>
                {r.events.slice(page * 100, (page + 1) * 100).map((e) => (
                  <article className="saved-plan-row" key={e.seq}>
                    <div>
                      <strong>
                        {e.seq} · {eventNames[e.kind]}
                      </strong>
                      {e.path && (
                        <p className="mono full-path" data-i18n="off">
                          {e.path}
                        </p>
                      )}
                      <p className="full-path" data-i18n="off">
                        {e.detail}
                      </p>
                      {e.token && (
                        <small className="mono full-path" data-i18n="off">
                          {e.token}
                        </small>
                      )}
                    </div>
                  </article>
                ))}
                <div className="modal-actions">
                  <button
                    className="button secondary"
                    disabled={page === 0}
                    onClick={() => setPage((p) => p - 1)}
                  >
                    上一页事件
                  </button>
                  <button
                    className="button secondary"
                    disabled={(page + 1) * 100 >= r.events.length}
                    onClick={() => setPage((p) => p + 1)}
                  >
                    下一页事件
                  </button>
                </div>
                {r.diagnostic && (
                  <details>
                    <summary>当时的诊断记录</summary>
                    <pre
                      className="mono full-path"
                      data-i18n="off"
                      style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
                    >
                      {r.diagnostic}
                    </pre>
                  </details>
                )}
              </>
            )
          })()
        : kind === 'extraction'
          ? (() => {
              const r = record as ZipExtractionRecord
              return (
                <>
                  <p>原压缩包与当时输出位置</p>
                  <p className="mono full-path" data-i18n="off">
                    {r.draft.root}
                  </p>
                  <p className="mono full-path" data-i18n="off">
                    → {r.draft.outputPath}
                  </p>
                  {r.draft.macMetadata && (
                    <p>含 Mac 数据属性、普通权限和修改时间恢复；历史安全批准未恢复。</p>
                  )}
                  <p>
                    {r.draft.files} 个文件 · {r.draft.directories} 个目录 · {bytes(r.draft.bytes)}
                  </p>
                  <p className="full-path" data-i18n="off">
                    {r.reason}
                  </p>
                  <details>
                    <summary>当时的校验证据</summary>
                    <p className="mono full-path" data-i18n="off">
                      SHA-256: {r.draft.sourceSha256}
                    </p>
                    {r.artifact && (
                      <p className="mono full-path" data-i18n="off">
                        {r.artifact.device}:{r.artifact.inode} · SHA-256: {r.artifact.sha256}
                      </p>
                    )}
                    <p className="mono full-path" data-i18n="off">
                      {r.temporaryPath}
                    </p>
                  </details>
                  <p className="muted">
                    归档仅保留当时的解压证据，不重新解压、覆盖或删除当前位置。
                  </p>
                </>
              )
            })()
          : (() => {
              const r = record as ManagedRecord
              return (
                <>
                  <p data-i18n="off">
                    {r.draft.name} · {r.draft.token} · {r.draft.installedVersion}
                  </p>
                  {r.draft.upgrade && (
                    <p data-i18n="off">
                      {r.draft.installedVersion} → {r.draft.upgrade.version}
                    </p>
                  )}
                  <p className="mono full-path" data-i18n="off">
                    {r.draft.path}
                  </p>
                  <p className="mono full-path" data-i18n="off">
                    {r.draft.managerPath}
                  </p>
                  <p className="full-path" data-i18n="off">
                    {r.reason}
                  </p>
                  <p className="muted">
                    Homebrew
                    操作没有本应用的废纸篓收据；归档不会重新调用包管理器，也不提供版本回滚。
                  </p>
                </>
              )
            })()}
      <details onToggle={(e) => setRaw(e.currentTarget.open)}>
        <summary>原始执行记录</summary>
        {raw && (
          <pre
            className="mono full-path"
            data-i18n="off"
            style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}
          >
            {JSON.stringify(record, null, 2)}
          </pre>
        )}
      </details>
    </>
  )
}
