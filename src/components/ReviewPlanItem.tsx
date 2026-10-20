import type { CleanupPreparation, ReviewPlan } from '../types'
import { bytes } from '../lib/format'
import { ImpactDetails, unknownImpact } from './ImpactDetails'
import { OriginDetails } from './OriginDetails'
import { PermissionEvidence, permissionSummary } from './PermissionEvidence'

const states: Record<string, string> = {
  unchanged: '文件信息未见变化',
  changed: '目标已变化',
  protected: '当前已保护',
  unavailable: '无法确认身份',
  expired: '计划已过期',
  versionMismatch: '版本已变化',
  moleProtected: 'Mole 规则保护',
  active: '使用中或无法确认空闲',
  ruleUnavailable: '规则复查未完成',
  pendingRules: '规则复查中',
  pendingLocal: '本地复核中',
  moduleRetained: '模块要求保留或重新确认',
  moduleUnsupported: '模块规则尚未适配',
  activityBusy: '发现文件占用',
  activityUnknown: '文件占用尚未确认',
  activityUnavailable: '附加占用检查未完成',
  descendantProtected: '目录后代受保护',
  descendantUnavailable: '目录后代保护尚未确认',
  contentsChanged: '目录内容已变化',
  inventoryUnavailable: '目录内容无法确认',
  inventoryMissing: '需要重新生成目录计划',
}

export function ReviewPlanItem({
  item,
  preparation,
}: {
  item: ReviewPlan['items'][number]
  preparation?: CleanupPreparation['items'][number]
}) {
  const permission = permissionSummary(item.permissions)
  const name = item.path.split('/').filter(Boolean).at(-1) ?? item.path
  return (
    <article className="plan-item">
      <div className="plan-item-heading">
        <h3>{name}</h3>
        <strong>{bytes(item.size)}</strong>
      </div>
      <p className="mono full-path">{item.path}</p>
      <div className="plan-item-status">
        <span className={`tag ${item.state === 'unchanged' ? '' : 'plan-problem'}`}>
          {states[item.state] ?? '需要重新确认'}
        </span>
        {permission.problem && <span className="tag plan-problem">{permission.label}</span>}
        {preparation && (
          <span className="tag">
            {preparation.code === 'observed' ? '日志准备已核对' : '日志暂不处理'}
          </span>
        )}
      </div>
      {item.state !== 'unchanged' && <p className="plan-item-reason">{item.reason}</p>}
      {preparation && <p className="plan-item-reason">{preparation.reason}</p>}
      <p className="plan-item-impact">可能影响：{(item.impact ?? unknownImpact).consequence}</p>
      <details className="plan-item-details">
        <summary>检查详情与用途</summary>
        {item.state === 'unchanged' && <p>{item.reason}</p>}
        <PermissionEvidence value={item.permissions} />
        {item.inventoryEntries != null && (
          <p>
            已记录目录内 {item.inventoryEntries}{' '}
            个子项的元数据，用于复核深层内容变化；不读取文件正文。
          </p>
        )}
        <OriginDetails value={item.provenance} />
        <ImpactDetails impact={item.impact} />
        {item.moleRule && (
          <>
            <p className="rule-evidence">
              {item.moleRule.source}：{item.moleRule.reason}
            </p>
            <div className="module-evidence" aria-label="模块规则复查">
              <strong>
                {item.moleRule.module?.scope === 'userCache'
                  ? '用户缓存模块'
                  : item.moleRule.module?.scope === 'userLogs'
                    ? '用户日志模块'
                    : '其他模块范围'}
              </strong>
              <p>
                {item.moleRule.module?.reason ??
                  '这份结果没有模块检查证据，请重新复查；路径通过不代表模块条件已满足。'}
              </p>
              {item.moleRule.module && (
                <details>
                  <summary>检查来源与版本</summary>
                  <p>
                    {item.moleRule.module.source} · 规则 v{item.moleRule.module.version}
                  </p>
                </details>
              )}
            </div>
            <div className="module-evidence" aria-label="附加占用检查">
              <strong>附加占用检查</strong>
              <p>
                {item.moleRule.activity?.version === 1 &&
                [
                  'busy',
                  'noOpenHandles',
                  'unknown',
                  'budgetExhausted',
                  'checkFailed',
                  'notChecked',
                ].includes(item.moleRule.activity.code)
                  ? item.moleRule.activity.reason
                  : '这份结果缺少可识别的占用证据，请重新复查；不能视为空闲。'}
              </p>
              {item.moleRule.activity && (
                <details>
                  <summary>观察来源与版本</summary>
                  <p>
                    {item.moleRule.activity.source} · 观察 v{item.moleRule.activity.version}
                  </p>
                </details>
              )}
            </div>
          </>
        )}
        {item.descendantRules && (
          <div className="module-evidence" aria-label="目录后代路径保护">
            <strong>目录后代路径保护</strong>
            <p>
              {item.descendantRules.version === 1 &&
              ['checked', 'protected', 'unavailable', 'notChecked', 'notApplicable'].includes(
                item.descendantRules.code,
              )
                ? item.descendantRules.reason
                : '缺少可识别的后代路径保护证据，请重新复查。'}
            </p>
            {item.descendantRules.path && (
              <p className="mono full-path">{item.descendantRules.path}</p>
            )}
            <details>
              <summary>检查范围与来源</summary>
              <p>
                已核对 {item.descendantRules.checkedEntries} 项后代路径结果 · 观察 v
                {item.descendantRules.version}
              </p>
              <p>{item.descendantRules.source ?? '客户端有界枚举 · Mole 路径级规则'}</p>
              <p>
                根项目和后代共用最多 500 条规则输入；后代不作为新候选，不表示全部模块条件已覆盖。
              </p>
            </details>
          </div>
        )}
      </details>
    </article>
  )
}
