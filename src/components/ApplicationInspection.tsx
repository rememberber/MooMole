import { useEffect, useRef, useState } from 'react'
import { desktop, inspectApplication } from '../lib/api'
import { parseApplicationInspection } from '../lib/application-inspection'
import type { ApplicationInspection as Inspection } from '../types'

export function ApplicationInspection({
  taskId,
  index,
  expectedId,
}: {
  taskId?: string | null
  index: number
  expectedId: string
}) {
  const [value, setValue] = useState<Inspection | null>(null)
  const [pending, setPending] = useState(Boolean(desktop && taskId))
  const [error, setError] = useState('')
  const [stale, setStale] = useState(false)
  const active = useRef(false)
  const generation = useRef(0)
  const busy = useRef(false)
  async function read() {
    if (!desktop || !taskId || busy.current) return
    busy.current = true
    const request = ++generation.current
    setPending(true)
    setError('')
    setValue(null)
    setStale(false)
    try {
      const next = parseApplicationInspection(await inspectApplication(taskId, index), expectedId)
      if (active.current && request === generation.current) setValue(next)
    } catch (error) {
      if (active.current && request === generation.current)
        setError(error instanceof Error ? error.message : String(error))
    } finally {
      if (active.current && request === generation.current) {
        busy.current = false
        setPending(false)
      }
    }
  }
  useEffect(() => {
    active.current = true
    busy.current = false
    void read()
    return () => {
      active.current = false
      generation.current += 1
    }
    // The parent keys this observation by immutable task and row identity.
  }, [taskId, index, expectedId])
  useEffect(() => {
    if (!value) return
    const request = generation.current
    const timer = setTimeout(() => {
      if (active.current && request === generation.current) setStale(true)
    }, 60000)
    return () => clearTimeout(timer)
  }, [value])
  const labels = {
    observed: '发现主应用运行',
    notObserved: '未发现匹配主应用',
    unknown: '运行状态无法确认',
  }
  return (
    <section className="application-inspection" aria-label="应用状态检查">
      <p role="status">
        {!desktop || !taskId
          ? '请在桌面 App 中读取应用后检查状态。'
          : pending
            ? '正在检查本地应用状态…'
            : value
              ? `${stale ? '较早结果 · ' : ''}${labels[value.running.code]}`
              : '应用状态无法确认'}
      </p>
      {error && (
        <p className="error-text" role="alert">
          {error}
        </p>
      )}
      {value && (
        <>
          <p>
            <strong>
              {stale && '较早保护结果 · '}
              {value.protection.reasons.length
                ? '已命中客户端保护'
                : value.moleProtection.code === 'protected' ||
                    value.background.code === 'restricted'
                  ? 'Mole 要求保留'
                  : value.officialUninstaller.code === 'officialRequired'
                    ? '请使用官方卸载器'
                    : value.installations.copies.length > 0
                      ? '关联数据可能共用'
                      : '归属待确认'}
            </strong>
            {(value.protection.reasons.length > 0 ||
              value.moleProtection.code === 'protected' ||
              value.background.code === 'restricted' ||
              value.officialUninstaller.code === 'officialRequired') &&
              ' · 当前仅供查看'}
          </p>
          {value.protection.reasons.length > 0 && (
            <dl>
              <dt>保护原因</dt>
              <dd>
                {value.protection.reasons
                  .map(
                    (reason) =>
                      ({
                        systemLocation: '位于系统目录',
                        currentApplication: '当前客户端或同标识应用（未验证签名）',
                        clientDirectory: '与已保存保护目录重叠',
                      })[reason],
                  )
                  .join('；')}
              </dd>
              {value.protection.matchedPaths.length > 0 && (
                <>
                  <dt>保护目录</dt>
                  <dd className="mono">{value.protection.matchedPaths.join('；')}</dd>
                </>
              )}
            </dl>
          )}
          <dl>
            <dt>Mole 规则</dt>
            <dd>
              {value.moleProtection.code === 'protected'
                ? '命中 Mole 系统组件保护'
                : value.moleProtection.code === 'notMatched'
                  ? '本次未命中系统组件禁用规则'
                  : 'Mole 系统组件规则无法确认'}
            </dd>
            <dt>后台应用条件</dt>
            <dd>
              {value.background.code === 'notRequired'
                ? '未标记为后台专用应用'
                : value.background.code === 'directRoot'
                  ? `后台专用应用，位于 Mole 搜索目录（${
                      {
                        systemApplications: '系统 Applications',
                        userApplications: '用户 Applications',
                        systemInputMethods: '系统输入法',
                        userInputMethods: '用户输入法',
                        playCover: 'PlayCover',
                        externalApplications: '外接卷 Applications',
                      }[value.background.rootKind!]
                    }）`
                  : value.background.code === 'restricted'
                    ? '此后台应用受 Mole 目录范围限制'
                    : '后台应用条件无法确认'}
            </dd>
            <dt>卸载方式</dt>
            <dd>
              {value.officialUninstaller.code === 'officialRequired'
                ? `Mole 要求使用 ${value.officialUninstaller.vendor} 官方卸载器`
                : value.officialUninstaller.code === 'notMatched'
                  ? '本次未命中官方卸载器限制'
                  : '官方卸载器要求无法确认'}
            </dd>
          </dl>
          {value.background.reason && (
            <p>
              {
                {
                  outsideRoots:
                    '此后台应用未直接位于 Mole 搜索目录中，可能是辅助组件；请保留并通过所属应用处理。',
                  duplicateRoot:
                    '此卷目录与系统或用户应用目录是同一对象，Mole 会排除这个重复搜索位置。',
                  unreadableRoot:
                    '当前卷的 Applications 目录不可读，Mole 不将它列入搜索目录；请先检查访问权限后重试。',
                }[value.background.reason]
              }
            </p>
          )}
          {value.background.issue &&
            ['markerUnknown', 'rootsUnavailable', 'rootsChanged'].includes(
              value.background.issue,
            ) && (
              <p className="muted">
                {
                  {
                    markerUnknown: '后台专用标记格式无法识别，请保留并重新检查。',
                    rootsUnavailable: '应用目录、搜索位置或访问状态无法确认，请保留并重试。',
                    rootsChanged: '搜索目录或访问状态在检查期间发生变化，请重新检查。',
                  }[value.background.issue as 'markerUnknown' | 'rootsUnavailable' | 'rootsChanged']
                }
              </p>
            )}
          {value.officialUninstaller.code === 'officialRequired' && (
            <p>请按厂商说明操作；组织管理的应用请联系管理员。</p>
          )}
          {value.officialUninstaller.issue === 'nameUnavailable' && (
            <p className="muted">Mole 清单名称缺失或无法识别，请重新读取列表。</p>
          )}
          {(value.moleProtection.issue === 'rulesUnavailable' ||
            value.background.issue === 'rulesUnavailable' ||
            value.officialUninstaller.issue === 'rulesUnavailable') && (
            <p className="muted">内置规则资源未通过校验或无法读取，请检查引擎状态后重试。</p>
          )}
          <details className="application-installations">
            <summary>
              {stale ? '较早安装副本' : '安装副本'} ·{' '}
              {value.installations.code === 'unknown'
                ? '无法确认'
                : value.installations.copies.length > 0
                  ? `发现 ${value.installations.copies.length} 个其他登记位置`
                  : '未发现其他登记位置'}
              {value.installations.code === 'partial' && ' · 检查不完整'}
            </summary>
            <p>
              {value.installations.copies.length > 0
                ? '这些位置的本地标识相同，关联数据可能共用，请保留。'
                : '仍需保留关联数据；未发现其他登记位置不代表只有这一个安装副本。'}
            </p>
            {value.installations.issue && (
              <p className="muted">
                {
                  {
                    identityChanged: '本地标识已变化，请重新读取应用列表。',
                    identityUnavailable: '本地标识无法读取，请重新检查。',
                    rulesUnavailable: '规则资源无法确认，请检查引擎状态后重试。',
                    registryUnavailable: '系统登记位置无法读取，请保留并重试。',
                    limitReached: '登记位置超过 32 项，只核对了前 32 项，结果不完整。',
                    candidateUnavailable: '部分登记位置已失效、不可读或布局暂不支持，结果不完整。',
                    candidatesChanged: '部分位置在检查期间发生变化，已撤下对应结果，请重查。',
                  }[value.installations.issue]
                }
              </p>
            )}
            {value.installations.copies.map((copy) => (
              <p key={copy.path}>
                <span className="mono association-path">{copy.path}</span>
                <span className="muted">
                  版本 {copy.version ?? '未提供'}
                  {copy.build ? ` · 构建 ${copy.build}` : ''}
                </span>
              </p>
            ))}
            <p className="muted">
              仅核对系统登记的同标识位置，不包含完整目录、外接卷和安装收据扫描，也未核实不同标识应用之间的共享数据。没有认证签名或发布者。
            </p>
          </details>
          <details className="application-associations">
            <summary>
              {stale ? '较早关联位置' : '关联位置'} ·{' '}
              {value.associations.code === 'unknown'
                ? '无法确认'
                : `${value.associations.entries.filter((entry) => entry.status === 'present').length} 处发现 · ${value.associations.entries.filter((entry) => entry.status === 'unknown').length} 处无法确认`}
            </summary>
            <p>全部保留。位置命中不代表独占归属，缓存也可能含有个人数据。</p>
            {value.associations.code === 'unknown' ? (
              <p className="muted">
                {
                  {
                    identityChanged: '本地标识已变化，请重新读取应用列表。',
                    identityUnavailable: '本地标识无法读取，请保留并重新检查。',
                    rulesUnavailable: '规则资源无法确认，请检查引擎状态后重试。',
                    bundleIdUnsupported: '标识不符合 Mole 关联位置规则，暂不查询。',
                    homeUnavailable: '用户目录无法确认，请保留并重试。',
                  }[value.associations.issue!]
                }
              </p>
            ) : (
              <>
                <dl>
                  {value.associations.entries
                    .filter((entry) => entry.status !== 'notFound')
                    .map((entry) => (
                      <div key={entry.kind}>
                        <dt>
                          {
                            {
                              support: '应用数据',
                              cache: '缓存',
                              logs: '日志',
                              preferences: '设置',
                              savedState: '恢复状态',
                              container: '沙盒数据',
                              webKit: '网页数据',
                              httpStorage: '网络存储',
                            }[entry.kind]
                          }
                        </dt>
                        <dd>
                          {entry.status === 'present'
                            ? '发现位置 · 归属待确认'
                            : `无法确认 · ${{ unavailable: '请检查访问权限后重试', unsupported: '链接、挂载或文件类型暂不支持', changed: '检查期间发生变化，请重查' }[entry.issue!]}`}
                          <span className="mono association-path">~/{entry.relativePath}</span>
                        </dd>
                      </div>
                    ))}
                </dl>
                <p className="muted">
                  {value.associations.entries.filter((entry) => entry.status === 'notFound').length}{' '}
                  处常见位置本次未发现，不代表没有其他关联文件。
                </p>
              </>
            )}
            <p className="muted">
              仅查询精确标识的 8
              个常见位置，未测量大小；完整安装副本、共享数据及个人数据归属仍待核实。
            </p>
          </details>
          <details>
            <summary>规则来源</summary>
            <p className="muted">
              Mole find_app_files（精确标识位置子集） / should_protect_from_uninstall /
              official_uninstaller_vendor / uninstall_app_is_background_only /
              uninstall_app_is_directly_in_search_root ·{' '}
              {value.moleProtection.revision.slice(0, 12)}
              ；名称取自本次 Mole
              清单，厂商身份未认证。客户端目录与自身保护继续生效，未命中不代表可卸载。
            </p>
          </details>
          <dl>
            <dt>本地版本</dt>
            <dd>
              {value.version ?? '未提供'}
              {value.build && ` · 构建 ${value.build}`}
            </dd>
            {value.identity !== 'matches' && (
              <>
                <dt>本地 Bundle ID</dt>
                <dd className="mono">{value.bundleId ?? '无法读取'}</dd>
              </>
            )}
            <dt>标识比对</dt>
            <dd>
              {value.identity === 'matches'
                ? '与 Mole 清单一致，未验证签名'
                : value.identity === 'different'
                  ? '与清单不同'
                  : value.identity === 'unreported'
                    ? 'Mole 未提供标识，未比对'
                    : '本地标识无法确认'}
            </dd>
            {value.running.pids.length > 0 && (
              <>
                <dt>主应用 PID</dt>
                <dd>{value.running.pids.join('、')}</dd>
              </>
            )}
          </dl>
          <p className={value.identity === 'different' ? 'error-text' : undefined}>
            {value.running.reason}
          </p>
          {value.running.otherCopies > 0 && (
            <p>
              另有 {value.running.otherCopies}{' '}
              个同标识安装位置的主应用在运行，不能将其归到当前路径。
            </p>
          )}
          <p className="muted">
            检查于 {new Date(value.checkedAt).toLocaleTimeString('zh-CN', { hour12: false })} ·
            瞬时观察，辅助进程与文件占用未检查，不代表可卸载。{stale && '请重新检查。'}
          </p>
        </>
      )}
      {desktop && taskId && (
        <button className="button secondary compact" disabled={pending} onClick={() => void read()}>
          {pending ? '检查中…' : error ? '重试检查' : '重新检查状态'}
        </button>
      )}
    </section>
  )
}
