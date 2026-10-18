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
              {value.protection.reasons.length ? '已命中客户端保护' : '归属待确认'}
            </strong>
            {value.protection.reasons.length > 0 && ' · 当前仅供查看'}
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
          <p className="muted">
            {value.protection.reasons.length === 0 && '未命中本次客户端保护规则。'}
            Mole 卸载规则、共享数据和个人数据归属尚未检查。
          </p>
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
