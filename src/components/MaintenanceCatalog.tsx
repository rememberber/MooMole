import { maintenanceCatalog, maintenanceItems, maintenanceSourceMatches } from '../lib/maintenance'
import type { Bootstrap } from '../types'

export function MaintenanceCatalog({
  query,
  engine,
}: {
  query: string
  engine: Bootstrap['engine']
}) {
  const items = maintenanceItems(query)
  const matches = maintenanceSourceMatches(engine.revision, engine.version)
  return (
    <section className="maintenance-catalog" aria-label="维护项目说明">
      <p className="muted" role="status">
        {items.length} / {maintenanceCatalog.items.length} 项 ·
        仅查看作用与影响，尚未检查本机，执行暂未开放。
      </p>
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
          {items.map((item) => (
            <details key={item.id} className="maintenance-item">
              <summary>
                <span className="maintenance-summary">
                  <strong>{item.name}</strong>
                  <span>{item.purpose}</span>
                </span>
                <span className="tag">{item.kind}</span>
              </summary>
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
                本机适用性、有效保护规则与权限尚未检查，不能判断为可执行或无须维护。
              </p>
              <details className="origin-details">
                <summary>技术来源</summary>
                <p>
                  Mole {maintenanceCatalog.version} · optimize / optimise ·{' '}
                  {maintenanceCatalog.revision}
                </p>
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
          ))}
        </div>
      )}
    </section>
  )
}
