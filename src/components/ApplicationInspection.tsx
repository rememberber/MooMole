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
  const protectedAssociations = value
    ? [...value.associations.entries, ...value.extensionData.entries].filter(
        (entry) => entry.protectionReasons.length > 0,
      ).length
    : 0
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
                    : protectedAssociations > 0
                      ? '关联位置受保护'
                      : value.installations.copies.length > 0 ||
                          value.associations.entries.some(
                            (entry) =>
                              entry.kind === 'groupContainer' && entry.status === 'present',
                          )
                        ? '关联数据可能共用'
                        : '归属待确认'}
            </strong>
            {(value.protection.reasons.length > 0 ||
              value.moleProtection.code === 'protected' ||
              value.background.code === 'restricted' ||
              value.officialUninstaller.code === 'officialRequired' ||
              protectedAssociations > 0) &&
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
              {stale ? '较早共享关联' : '共享关联'} ·{' '}
              {value.installations.code === 'unknown'
                ? '无法确认'
                : value.installations.copies.length > 0
                  ? `发现 ${value.installations.copies.length} 个其他位置`
                  : '本次未发现其他关联位置'}
              {value.installations.code === 'partial' && ' · 检查不完整'}
            </summary>
            <p>
              {value.installations.copies.length > 0
                ? 'Mole 提示这些应用可能共用关联数据，请保留。'
                : '仍需保留关联数据；本次未命中不代表数据独占。'}
            </p>
            {value.installations.issue && (
              <p className="muted">
                {
                  {
                    identityChanged: '本地标识已变化，请重新读取应用列表。',
                    identityUnavailable: '本地标识无法读取，请重新检查。',
                    rulesUnavailable: '规则资源无法确认，请检查引擎状态后重试。',
                    registryUnavailable: '系统登记来源无法读取，已保留其他有效结果，请重试。',
                    listUnavailable: '本次 Mole 清单已不可用，已保留其他有效结果，请重新读取列表。',
                    limitReached: '候选或命中达到上限，结果不完整；请保留关联数据。',
                    receiptsIncomplete: '安装收据未能完整核对，请保留并查看下方原因。',
                    volumesIncomplete: '卷内安装位置检查未完整完成，请保留并查看下方原因。',
                    rootsIncomplete: '安装目录扫描未完整完成，请保留并查看下方原因。',
                    candidateUnavailable: '部分候选位置已失效、不可读或布局暂不支持，结果不完整。',
                    candidatesChanged: '部分位置在检查期间发生变化，已撤下对应结果，请重查。',
                  }[value.installations.issue]
                }
              </p>
            )}
            {value.installations.roots.code !== 'notChecked' && (
              <p className="muted">
                已探查 {value.installations.roots.rootCount} 个常用安装根
                {value.installations.roots.issue &&
                  ` · ${{ unavailable: '部分目录无法访问或布局不支持', changed: '目录在检查期间发生变化，请重查', limitReached: '扫描达到数量或时间上限' }[value.installations.roots.issue]}`}
                {value.installations.roots.firstUncertainPath && (
                  <span className="mono association-path">
                    {value.installations.roots.firstUncertainPath}
                  </span>
                )}
              </p>
            )}
            {value.installations.volumes.code !== 'notChecked' && (
              <p className="muted">
                已探查 {value.installations.volumes.rootCount} 个卷入口
                {value.installations.volumes.issue &&
                  ` · ${{ unavailable: '部分卷无法访问或布局不支持', changed: '挂载或卷目录发生变化，请重查', limitReached: '卷扫描达到数量或时间上限', unsupportedVolume: '网络卷或自动挂载位置暂不支持，结果不完整' }[value.installations.volumes.issue]}`}
                {value.installations.volumes.firstUncertainPath && (
                  <span className="mono association-path">
                    {value.installations.volumes.firstUncertainPath}
                  </span>
                )}
              </p>
            )}
            {value.installations.receipts.code !== 'notChecked' && (
              <p className="muted">
                已核对 {value.installations.receipts.checkedCount} 份非 Apple 安装收据
                {value.installations.receipts.issue &&
                  ` · ${{ unavailable: '部分收据未能完整读取，请重试', changed: '安装收据在检查期间变化，请重查', limitReached: '收据核对达到数量或时间上限', unsupportedReceipt: '部分收据格式或路径暂不支持' }[value.installations.receipts.issue]}`}
              </p>
            )}
            {value.installations.copies.map((copy) => (
              <p key={copy.path}>
                <span className="mono association-path">{copy.path}</span>
                <span className="muted">
                  {
                    {
                      sameIdentifier: '同标识位置',
                      relatedIdentifier: '派生标识，需保留关联数据',
                      channelName: '渠道包名命中，需保留关联数据',
                    }[copy.relation]
                  }{' '}
                  · 版本 {copy.version ?? '未提供'}
                  {copy.build ? ` · 构建 ${copy.build}` : ''}
                </span>
              </p>
            ))}
            <p className="muted">
              核对 Mole 清单前 64 项、系统登记前 32 项，以及常用安装根和本地卷各最多 128
              个候选；最多显示 32 个命中。卷内仅检查直属 App 与 Applications 下最多 3 层；收据只补查
              /usr/local、/opt 下最多 128
              个候选。网络卷与其他安装位置尚未完整覆盖，未命中不代表数据独占。
            </p>
          </details>
          <details className="application-associations">
            <summary>
              {stale ? '较早关联位置' : '关联位置'} ·{' '}
              {value.associations.code === 'unknown'
                ? '无法确认'
                : `${value.associations.entries.filter((entry) => entry.status === 'present').length} 处发现 · ${value.associations.entries.filter((entry) => entry.status === 'unknown').length} 处无法确认`}
              {value.associations.entries.some((entry) => entry.protectionReasons.length > 0) &&
                ` · ${value.associations.entries.filter((entry) => entry.protectionReasons.length > 0).length} 处受保护`}
              {value.associations.discovery.code === 'partial' && ' · 组与扩展检查不完整'}
            </summary>
            <p>全部保留。位置命中不代表独占归属，缓存也可能含有个人数据。</p>
            {value.associations.discovery.code !== 'notChecked' && (
              <p className="muted">
                已检查 {value.associations.discovery.rootCount} 个应用组与扩展目录
                {value.associations.discovery.issue &&
                  ` · ${{ unavailable: '部分目录无法完整读取，请重试', unsupported: '部分名称、链接或目录布局暂不支持', changed: '目录在检查期间变化，请重查', limitReached: '扫描达到数量或时间上限' }[value.associations.discovery.issue]}`}
                {value.associations.discovery.firstUncertainRoot && (
                  <span className="mono association-path">
                    ~/{value.associations.discovery.firstUncertainRoot}
                  </span>
                )}
              </p>
            )}
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
                    .filter(
                      (entry) => entry.status !== 'notFound' || entry.protectionReasons.length > 0,
                    )
                    .map((entry) => (
                      <div key={entry.relativePath}>
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
                              webContent: '网页内容',
                              httpCookies: '网络 Cookie',
                              cookies: 'Cookie',
                              scripts: '应用脚本',
                              inputMethod: '输入法扩展',
                              autosave: '自动保存',
                              syncedPreferences: '同步设置',
                              crashReports: '崩溃报告',
                              groupContainer: '应用组容器',
                              derivedScripts: '派生应用脚本',
                              derivedContainer: '派生沙盒数据',
                              fileProvider: '文件提供程序',
                            }[entry.kind]
                          }
                        </dt>
                        <dd>
                          {entry.status === 'present'
                            ? '发现位置 · 归属待确认'
                            : entry.status === 'notFound'
                              ? '本次未发现'
                              : `无法确认 · ${{ unavailable: '请检查访问权限后重试', unsupported: '链接、挂载或文件类型暂不支持', changed: '检查期间发生变化，请重查' }[entry.issue!]}`}
                          {[
                            'groupContainer',
                            'derivedScripts',
                            'derivedContainer',
                            'fileProvider',
                          ].includes(entry.kind) && (
                            <span className="association-path muted">
                              目录名规则命中，可能由多个应用共用，保留
                            </span>
                          )}
                          {entry.protectionReasons.length > 0 && (
                            <span className="association-path">
                              {entry.protectionReasons
                                .map(
                                  (reason) =>
                                    ({
                                      systemLocation: '系统位置，保留',
                                      currentApplication: '客户端自身保护，保留',
                                      clientDirectory: '已保存保护目录覆盖，保留',
                                    })[reason],
                                )
                                .join('；')}
                            </span>
                          )}
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
              仅查询精确标识的 16 个常见位置，另检查应用组与扩展的 4 个目录各一层、合计最多 64
              个候选，未测量大小。目录名不证明扩展身份或数据独占；已取得的扩展标识与数据位置见下方，完整归属仍待核实。
            </p>
          </details>
          <details className="application-embedded" aria-label="嵌入扩展">
            <summary>
              {stale && '较早'}
              嵌入扩展 ·{' '}
              {value.embedded.code === 'notChecked'
                ? '未检查'
                : `${value.embedded.bundles.length} 个位置`}
              {(value.embedded.code === 'partial' ||
                ['partial', 'unknown'].includes(value.extensionData.code)) &&
                ' · 检查不完整'}
            </summary>
            <p>仅核对包内标识；签名、运行状态和数据归属未验证，关联数据继续保留。</p>
            {value.embedded.issue && (
              <p className="muted">
                {
                  {
                    identityChanged: '应用标识与清单不同，请重新读取列表。',
                    identityUnavailable: '主应用标识无法确认，请重试。',
                    rulesUnavailable: 'Mole 规则无法确认，请检查内置引擎后重试。',
                    bundleIdUnsupported: '主应用标识格式暂不支持。',
                    unavailable: '部分组件无法完整读取，请重试。',
                    unsupported: '部分链接、布局或身份文件暂不支持。',
                    changed: '组件在检查期间变化，已撤下本次扩展标识，请重查。',
                    limitReached: '检查达到数量、深度或时间上限，结果不完整。',
                  }[value.embedded.issue]
                }
              </p>
            )}
            {value.embedded.bundles.length > 0 && (
              <dl>
                {value.embedded.bundles.map((bundle) => (
                  <div key={bundle.relativePath}>
                    <dt>
                      {{ appex: '应用扩展', xpc: 'XPC 服务', loginItem: '登录项' }[bundle.kind]}
                    </dt>
                    <dd>
                      <span className="mono association-path">{bundle.bundleId}</span>
                      <span className="mono association-path">{bundle.relativePath}</span>
                    </dd>
                  </div>
                ))}
              </dl>
            )}
            {value.embedded.code === 'observed' && value.embedded.bundles.length === 0 && (
              <p className="muted">本次范围内未发现符合规则的扩展标识，不代表不存在其他组件。</p>
            )}
            {value.embedded.code !== 'notChecked' && (
              <p className="muted">
                已查看 {value.embedded.plists} 份身份文件所在位置。仅检查选中 App 的 Contents 内最多
                12 层、128 份身份文件与 32 个扩展位置；不包含主标识与 Sparkle 共享服务。
              </p>
            )}
            <div className="extension-data" aria-label="扩展关联数据">
              <p>
                <strong>
                  扩展数据 ·{' '}
                  {value.extensionData.code === 'notChecked'
                    ? '未检查'
                    : value.extensionData.code === 'unknown'
                      ? '无法确认'
                      : value.extensionData.identifiers.length === 0
                        ? '未取得可查询标识'
                        : `${value.extensionData.entries.filter((entry) => entry.status === 'present').length} 处发现 · ${value.extensionData.entries.filter((entry) => entry.status === 'unknown').length} 处无法确认`}
                </strong>
                {value.extensionData.entries.some((entry) => entry.protectionReasons.length > 0) &&
                  ` · ${value.extensionData.entries.filter((entry) => entry.protectionReasons.length > 0).length} 处受保护`}
              </p>
              {value.extensionData.issue && (
                <p className="muted">
                  {
                    {
                      identityChanged: '主标识已变化，未查询扩展数据。',
                      identityUnavailable: '主标识无法确认，未查询扩展数据。',
                      rulesUnavailable: '规则无法确认，未查询扩展数据。',
                      bundleIdUnsupported: '主标识格式暂不支持。',
                      homeUnavailable: '用户目录无法确认，请保留并重试。',
                      embeddedIncomplete: '仅查询本次已取得的扩展标识，组件检查仍不完整。',
                      embeddedChanged: '扩展身份在检查期间变化，已撤下数据位置，请重查。',
                      embeddedUnavailable: '扩展身份未能复核，已撤下数据位置，请重试。',
                      limitReached: '扩展数据检查达到数量或时间上限，范围不完整。',
                      byHostIncomplete: '部分设备设置文件无法完整确认，请查看下方原因。',
                    }[value.extensionData.issue]
                  }
                </p>
              )}
              {value.extensionData.byHost.code === 'partial' && (
                <p className="muted">
                  ~/Library/Preferences/ByHost ·{' '}
                  {
                    {
                      unavailable: '无法完整读取，请重试',
                      unsupported: '部分名称、链接或布局暂不支持',
                      changed: '检查期间变化，请重查',
                      limitReached: '扫描达到数量或时间上限',
                    }[value.extensionData.byHost.issue!]
                  }
                </p>
              )}
              <dl>
                {value.extensionData.entries
                  .filter(
                    (entry) => entry.status !== 'notFound' || entry.protectionReasons.length > 0,
                  )
                  .map((entry) => (
                    <div key={entry.relativePath}>
                      <dt>
                        {
                          {
                            scripts: '扩展脚本',
                            fileProvider: '文件提供程序',
                            cache: '扩展缓存',
                            container: '扩展沙盒',
                            httpStorage: '扩展网络存储',
                            httpCookies: '扩展 Cookie',
                            preferences: '扩展设置',
                            webKit: '扩展网页数据',
                            byHost: '设备设置',
                          }[entry.sources[0].kind]
                        }
                      </dt>
                      <dd>
                        {entry.status === 'present'
                          ? '发现位置 · 归属待确认'
                          : entry.status === 'notFound'
                            ? '本次未发现'
                            : `无法确认 · ${{ unavailable: '请检查访问权限后重试', unsupported: '链接、挂载或文件类型暂不支持', changed: '检查期间发生变化，请重查' }[entry.issue!]}`}
                        {entry.protectionReasons.length > 0 && (
                          <span className="association-path">
                            {entry.protectionReasons
                              .map(
                                (reason) =>
                                  ({
                                    systemLocation: '系统位置，保留',
                                    currentApplication: '客户端自身保护，保留',
                                    clientDirectory: '已保存保护目录覆盖，保留',
                                  })[reason],
                              )
                              .join('；')}
                          </span>
                        )}
                        <span className="mono association-path">
                          {[...new Set(entry.sources.map((source) => source.bundleId))].join('、')}
                        </span>
                        <span className="mono association-path">~/{entry.relativePath}</span>
                      </dd>
                    </div>
                  ))}
              </dl>
              {['observed', 'partial'].includes(value.extensionData.code) && (
                <p className="muted">
                  {value.extensionData.identifiers.length === 0
                    ? '没有取得可查询的扩展标识，不代表不存在扩展数据。'
                    : `本次查询 ${value.extensionData.identifiers.length} 个扩展标识，${value.extensionData.entries.filter((entry) => entry.status === 'notFound').length} 处位置未发现。`}
                </p>
              )}
              <p className="muted">
                全部保留。仅检查已取得标识的前 8 个、各 8 类精确位置与一层 ByHost
                设置文件；不测量大小，不证明数据独占或可卸载。
              </p>
            </div>
          </details>
          <details>
            <summary>规则来源</summary>
            <p className="muted">
              Mole uninstall_bundles_share_remnants（共享保留） / find_app_files（精确标识位置子集）
              / should_protect_from_uninstall / official_uninstaller_vendor /
              uninstall_app_is_background_only / uninstall_app_is_directly_in_search_root ·{' '}
              {value.moleProtection.revision.slice(0, 12)}
              ；官方卸载器名称取自本次 Mole
              清单，共享保留使用已核对路径的包名，厂商身份未认证。客户端目录与自身保护继续生效，未命中不代表可卸载。
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
