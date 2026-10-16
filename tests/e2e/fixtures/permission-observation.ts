import type { PermissionObservation } from '../../../src/types'

// Browser-only observations; never used to authorize filesystem operations.
export const clearPermissions: PermissionObservation = {
  version: 1,
  code: 'noRestrictionObserved',
  reason: 'fixture：本次有限检查未发现限制，不代表已经获准清理。',
  findings: [
    'volume',
    'parentAccess',
    'fileFlags',
    'stickyDirectory',
    'extendedAcl',
    'mountBoundary',
  ].map((kind) => ({
    kind,
    state: 'clear',
    reason: 'fixture：本次未发现该项限制。',
    osError: null,
  })),
}
export const restrictedPermissions: PermissionObservation = {
  ...clearPermissions,
  code: 'restricted',
  reason: 'fixture：系统拒绝父目录写入或搜索访问。',
  findings: clearPermissions.findings.map((f) =>
    f.kind === 'parentAccess'
      ? {
          ...f,
          state: 'restricted',
          reason: 'fixture：不能仅据此区分普通权限与隐私授权。',
          osError: 13,
        }
      : f,
  ),
}
export const unknownPermissions: PermissionObservation = {
  ...clearPermissions,
  code: 'unknown',
  reason: 'fixture：扩展访问控制尚未可靠确认。',
  findings: clearPermissions.findings.map((f) =>
    f.kind === 'extendedAcl'
      ? { ...f, state: 'unknown', reason: 'fixture：检测到 ACL，未计算删除或移入权限。' }
      : f,
  ),
}
