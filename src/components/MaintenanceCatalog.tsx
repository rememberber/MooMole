import {
  maintenanceCatalog,
  maintenanceItems,
  maintenanceSourceMatches,
  maintenanceConclusion,
  type MaintenanceReport,
} from '../lib/maintenance'
import { dateTime } from '../lib/format'
import type { Bootstrap } from '../types'

export function MaintenanceCatalog({
  query,
  engine,
  report,
  reading = false,
  error = '',
}: {
  query: string
  engine: Bootstrap['engine']
  report?: MaintenanceReport | null
  reading?: boolean
  error?: string
}) {
  const items = maintenanceItems(query)
  const matches = maintenanceSourceMatches(engine.revision, engine.version)
  const snapshot = matches && engine.available ? report : null
  return (
    <section className="maintenance-catalog" aria-label="维护项目说明">
      <p className="muted" role="status">
        {items.length} / {maintenanceCatalog.items.length} 项 ·
        {snapshot ? '2 项已有设置观察，其余尚未检查。' : '可检查 2 项本机设置，其余仅提供说明。'}
        执行暂未开放。
      </p>
      {reading && (
        <p className="muted" role="status">
          正在读取两项设置及维护项目保护规则…
        </p>
      )}
      {error && (
        <p className="notice" role="alert">
          {error}
          {snapshot && ' 以下保留上次结果，本次未更新。'}
        </p>
      )}
      {snapshot && (
        <p className="muted">
          上次检查 {dateTime(snapshot.checkedAt)} · 仅代表当时设置，之后可能变化。
        </p>
      )}
      {(!engine.available || !matches) && (
        <p className="notice" role="alert">
          {!matches ? '当前引擎与说明基线不匹配。' : '当前引擎不可用。'}
          以下仅作为 Mole {maintenanceCatalog.version} 的功能参考，不能据此判断本机是否适用。
        </p>
      )}
      {items.length === 0 ? (
        <div className="empty-state">
          <h3>没有匹配的维护项目</h3>
          <p className="muted">可搜索用途、影响、权限或项目名称。</p>
        </div>
      ) : (
        <div className="maintenance-list">
          {items.map((item) => {
            const observed = snapshot?.items.find((result) => result.id === item.id)
            return (
              <details key={item.id} className="maintenance-item">
                <summary>
                  <span className="maintenance-summary">
                    <strong>{item.name}</strong>
                    <span>{item.purpose}</span>
                    {observed && (
                      <span className="maintenance-observation">
                        上次结果：{maintenanceConclusion(observed)}
                      </span>
                    )}
                  </span>
                  <span className="tag">{item.kind}</span>
                </summary>
                {observed && (
                  <section className="maintenance-observed" aria-label={`${item.name}设置观察`}>
                    <dl>
                      {observed.findings.map((finding) => (
                        <div key={finding.key}>
                          <dt>{finding.label}</dt>
                          <dd>
                            {finding.value === 'enabled'
                              ? '已启用'
                              : finding.value === 'disabled'
                                ? '未启用'
                                : '未知（未返回值或无法确认）'}
                            {finding.managed && ' · 由系统策略管理，保持现状'}
                          </dd>
                        </div>
                      ))}
                    </dl>
                    <p className="muted">
                      Mole 项目保护：
                      {observed.taskProtection === 'matched'
                        ? '命中维护跳过规则，应保留'
                        : observed.taskProtection === 'unknown'
                          ? '读取未确认，应保留'
                          : '未命中项目级跳过规则'}
                      。
                    </p>
                    <p className="muted">原始键值尚未保存，当前没有精确撤销能力。</p>
                    <details className="maintenance-paths">
                      <summary>文件保护与访问观察</summary>
                      {observed.paths.map((path) => (
                        <div className="maintenance-path" key={path.location}>
                          <p className="maintenance-location">~/{path.location}</p>
                          <p>
                            文件状态：
                            {path.state === 'present'
                              ? '存在'
                              : path.state === 'missing'
                                ? '未发现，未测试创建权限'
                                : '路径未确认'}
                            。
                          </p>
                          <p>
                            客户端保护：
                            {path.clientProtection === 'matched'
                              ? '命中，应保留'
                              : path.clientProtection === 'unknown'
                                ? '未确认，应保留'
                                : '未命中保护目录'}
                            。
                          </p>
                          <p>
                            Mole 路径规则：
                            {path.moleProtection === 'matched'
                              ? '命中，应保留'
                              : path.moleProtection === 'unknown'
                                ? '未确认，应保留'
                                : '本次未命中'}
                            。
                          </p>
                          <p>
                            文件访问：
                            {path.access === 'restricted'
                              ? '发现限制，不会自动提权或修改权限'
                              : path.access === 'noRestrictionObserved'
                                ? '有限检查未发现限制，不代表系统偏好服务可以写入'
                                : path.access === 'notChecked'
                                  ? '未检查'
                                  : '未知，不能假定可写'}
                            。
                          </p>
                        </div>
                      ))}
                      <p>
                        只观察当前用户的固定偏好文件；未检查其他主机/用户域，也不代表完整写入权限。
                      </p>
                    </details>
                  </section>
                )}
                <dl>
                  <div>
                    <dt>作用范围</dt>
                    <dd>{item.target}</dd>
                  </div>
                  <div>
                    <dt>执行前条件</dt>
                    <dd>{item.conditions}</dd>
                  </div>
                  <div>
                    <dt>权限要求</dt>
                    <dd>{item.permissions}</dd>
                  </div>
                  <div>
                    <dt>可能影响</dt>
                    <dd>{item.impact}</dd>
                  </div>
                  <div>
                    <dt>恢复限制</dt>
                    <dd>{item.recovery}</dd>
                  </div>
                </dl>
                <p className="muted maintenance-pending">
                  {observed
                    ? '这里只提供有限设置、保护与文件访问观察；实际写入、原值恢复及其他执行条件尚未验证，不能据此执行维护。'
                    : '本机适用性、有效保护规则与权限尚未检查，不能判断为可执行或无须维护。'}
                </p>
                <details className="origin-details">
                  <summary>技术来源</summary>
                  <p>
                    Mole {maintenanceCatalog.version} · optimize / optimise ·{' '}
                    {maintenanceCatalog.revision}
                  </p>
                  {observed && (
                    <>
                      <p>
                        设置来源：CFPreferences
                        有效偏好值及受管理标记；未返回值不等于未设置。维护规则来源：
                        {snapshot?.ruleSource === 'current'
                          ? 'whitelist_optimize'
                          : snapshot?.ruleSource === 'legacy'
                            ? 'whitelist_checks（只读兼容，未迁移）'
                            : snapshot?.ruleSource === 'defaults'
                              ? '固定版本空默认列表'
                              : '未知'}
                        。
                      </p>
                      <p>{observed.findings.map((f) => `${f.domain} / ${f.key}`).join('；')}</p>
                    </>
                  )}
                  <p>
                    lib/optimize/catalog.sh → lib/optimize/tasks.sh · {item.id} → {item.handler}
                  </p>
                  {item.id === 'fix_broken_configs' && (
                    <p>偏好设置校验与移除：lib/optimize/maintenance.sh。</p>
                  )}
                  <p>
                    上游目录并不保证本机可安全执行；当前页面不调用 optimize 或
                    --dry-run，也不发起授权。
                  </p>
                </details>
              </details>
            )
          })}
        </div>
      )}
    </section>
  )
}
