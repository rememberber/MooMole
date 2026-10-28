import type { CleanupPreparation, ReviewPlan } from '../types'

export function cleanupPreparation(plan: ReviewPlan, now: number) {
  const report = plan.cleanupPreparation
  if (report == null) return null
  const invalid = {
    valid: false,
    summary: '清理前检查结果无法核对',
    next: '请重新复查 Mole 规则；旧结果不能用于确认处理范围。',
    items: new Map<string, CleanupPreparation['items'][number]>(),
  }
  if (
    plan.readOnly !== true ||
    plan.requiresMoleRecheck !== true ||
    report.version !== 2 ||
    report.planId !== plan.id ||
    !['userLogFiles', 'userCachesAndLogs'].includes(report.scope) ||
    report.method !== 'trash' ||
    report.readOnly !== true ||
    !Array.isArray(report.items) ||
    report.items.some((item) => item == null || typeof item !== 'object') ||
    report.items.length !== plan.items.length ||
    report.items.length > 500 ||
    plan.itemCount !== plan.items.length ||
    new Set(report.items.map((item) => item.id)).size !== report.items.length ||
    !Number.isSafeInteger(report.checkedAt) ||
    !Number.isSafeInteger(report.expiresAt) ||
    !plan.ruleCheckedAt ||
    report.checkedAt < plan.ruleCheckedAt ||
    report.checkedAt - plan.ruleCheckedAt >= 30_000 ||
    report.checkedAt < plan.checkedAt ||
    report.checkedAt < plan.createdAt ||
    report.expiresAt > plan.expiresAt ||
    report.expiresAt > report.checkedAt + 30_000 ||
    report.expiresAt > plan.ruleCheckedAt + 30_000 ||
    report.expiresAt <= report.checkedAt ||
    report.items.some((item, index) => {
      const source = plan.items[index]
      const rule = source.moleRule
      return (
        item.id !== source.id ||
        !['observed', 'authorizationRequired', 'retained', 'notChecked'].includes(item.code) ||
        typeof item.reason !== 'string' ||
        !item.reason.trim() ||
        ((item.code === 'observed' || item.code === 'authorizationRequired') &&
          (source.state !== (item.code === 'observed' ? 'unchanged' : 'activityUnknown') ||
            source.permissions?.version !== 1 ||
            source.permissions.code !== 'noRestrictionObserved' ||
            rule?.code !== 'passed' ||
            rule.module?.version !== 2 ||
            !['userLogs', 'userCache'].includes(rule.module.binding) ||
            rule.module.scope !== rule.module.binding ||
            (report.scope === 'userLogFiles' && rule.module.binding !== 'userLogs') ||
            (rule.module.binding === 'userCache' &&
              source.inventoryEntries != null &&
              (source.descendantRules?.version !== 1 ||
                source.descendantRules.code !== 'checked')) ||
            rule.module.code !== 'checked' ||
            rule.activity?.version !== 1 ||
            rule.activity.code !== (item.code === 'observed' ? 'noOpenHandles' : 'unknown')))
      )
    }) ||
    !Number.isSafeInteger(report.observedCount) ||
    report.observedCount < 0 ||
    !Number.isSafeInteger(report.authorizationCount) ||
    report.authorizationCount < 0 ||
    report.observedCount + report.authorizationCount > 20 ||
    report.observedCount !== report.items.filter((item) => item.code === 'observed').length ||
    report.authorizationCount !==
      report.items.filter((item) => item.code === 'authorizationRequired').length
  )
    return invalid
  if (now < report.checkedAt || now >= report.expiresAt || now >= plan.expiresAt)
    return {
      ...invalid,
      summary: '清理前检查已失效',
      next: '请重新复查 Mole 规则；准备结果只保留 30 秒，检查不会清理文件。',
    }
  return {
    valid: true,
    summary: report.authorizationCount
      ? `${report.observedCount + report.authorizationCount} 项可核对处理范围，其中 ${report.authorizationCount} 项仍需授权检查 · ${report.items.length - report.observedCount - report.authorizationCount} 项保留`
      : report.observedCount
        ? `${report.observedCount} 项完成本轮准备检查 · ${report.items.length - report.observedCount} 项保留`
        : '本轮所选项目全部保留',
    next: report.authorizationCount
      ? '下一步：核对固定范围并明确确认，再由本人进行当次系统授权；占用仍未知的文件保留。'
      : report.observedCount
        ? '下一步：逐项核对项目用途与影响。准备结果不代表清理授权，实际处理前仍会重新检查。'
        : '下一步：查看逐项保留原因；不要根据文件名或大小扩大处理范围。',
    items: new Map(report.items.map((item) => [item.id, item])),
  }
}
