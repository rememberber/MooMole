import { useRef, useState } from 'react'
import { Modal } from './Modal'
import {
  appControlOutcome,
  controlApp,
  stopAppControl,
  type AppControlReport,
  type AppControlTarget,
} from '../lib/app-control'

export function ApplicationControlCenter({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: AppControlTarget | null
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  if (!target) return null
  return (
    <Control
      key={`${target.taskId}:${target.index}:${target.action}`}
      target={target}
      onClose={onClose}
      onActiveChange={onActiveChange}
      onOutcome={onOutcome}
    />
  )
}
function Control({
  target,
  onClose,
  onActiveChange,
  onOutcome,
}: {
  target: AppControlTarget
  onClose: () => void
  onActiveChange: (active: boolean) => void
  onOutcome: () => void
}) {
  const [pending, setPending] = useState(false),
    [report, setReport] = useState<AppControlReport | null>(null),
    [error, setError] = useState('')
  const request = useRef<string | null>(null),
    busy = useRef(false)
  const title =
    target.action === 'register'
      ? '重新注册应用'
      : target.action === 'quit'
        ? '正常退出应用'
        : '打开应用'
  async function run() {
    if (busy.current) return
    busy.current = true
    setPending(true)
    setError('')
    onActiveChange(true)
    const id = crypto.randomUUID()
    request.current = id
    try {
      setReport(await controlApp(target, id))
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
    } finally {
      setPending(false)
      onActiveChange(false)
      onOutcome()
      busy.current = false
    }
  }
  return (
    <Modal title={title} onClose={pending ? undefined : onClose}>
      <h2>{title}</h2>
      <strong data-i18n="off">{target.name}</strong>
      <p className="mono full-path">{target.path}</p>
      <p className="notice">
        {target.action === 'register'
          ? '更新所选安装副本的图标和文档/网址类型注册，可能改变“打开方式”候选；不清空系统注册库，不主动设置默认应用。不会启动、退出应用或重启 Finder，也不修改应用包。注册没有自动撤销，不能保证图标或打开方式问题一定修复。'
          : target.action === 'quit'
            ? '仅请求所选安装位置的应用正常退出，可能出现保存提示或被应用拒绝；不会强制结束，也不会关闭其他安装副本。'
            : '打开所选安装位置，已经运行时请求显示该副本。macOS 可能显示安全或权限提示，请根据实际提示处理；不会绕过系统保护。'}
      </p>
      {pending && (
        <p role="status">
          {target.action === 'register'
            ? '正在等待系统注册与位置核对。停止不能撤销已发出的请求，系统调用本身可能仍需等待返回。'
            : '正在等待应用响应。停止等待不会撤销已经发出的系统请求。'}
        </p>
      )}
      {report && (
        <section aria-label="应用操作结果">
          <strong>{appControlOutcome[report.outcome]}</strong>
          <p>{report.reason}</p>
          <p className="muted">
            {target.action === 'register'
              ? '请刷新应用列表，再检查原来的图标或“打开方式”候选；注册记录保存在操作历史中，不自动重试或回滚。'
              : '刷新应用列表后再检查运行状态或准备卸载。打开和退出记录会保存在操作历史中。'}
          </p>
          {target.action === 'register' && report.registrationStatus !== undefined && (
            <details>
              <summary>系统注册返回信息</summary>
              <p>系统状态：{report.registrationStatus}</p>
              <p>
                {report.registeredPathObserved === true
                  ? '已核对所选安装路径。'
                  : '所选安装路径尚未确认。'}
              </p>
            </details>
          )}
        </section>
      )}
      {error && (
        <p className="notice warning" role="alert">
          {error}
        </p>
      )}
      <div className="modal-actions">
        {pending ? (
          <button
            className="button secondary"
            onClick={() => {
              if (request.current)
                void stopAppControl(request.current).catch((e) => setError(String(e)))
            }}
          >
            停止等待
          </button>
        ) : (
          <button className="button secondary" onClick={onClose}>
            返回应用列表
          </button>
        )}
        {!pending && !report && !error && (
          <button className="button primary" onClick={() => void run()}>
            {target.action === 'register'
              ? '确认重新注册'
              : target.action === 'quit'
                ? '确认正常退出'
                : '确认打开应用'}
          </button>
        )}
      </div>
    </Modal>
  )
}
