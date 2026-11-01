import { useEffect, useRef, useState } from 'react'
import type { AppEntry } from '../types'
import { applicationUsage, type AppUsageReport } from './application-usage'
import { useNow } from './use-now'
export function useApplicationUsage(
  apps: AppEntry[],
  taskId: string | null | undefined,
  refreshing: boolean,
  protectionKey: string,
) {
  const [saved, setSaved] = useState<{
    apps: AppEntry[]
    taskId: string
    protectionKey: string
    report: AppUsageReport
  } | null>(null)
  const [pending, setPending] = useState(false),
    [stopping, setStopping] = useState(false),
    [error, setError] = useState('')
  const current = useRef({ apps, taskId, refreshing, protectionKey }),
    active = useRef<string | null>(null),
    alive = useRef(true)
  current.current = { apps, taskId, refreshing, protectionKey }
  useNow()
  const now = Date.now()
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
      if (active.current) void applicationUsage.cancel(active.current).catch(() => {})
    }
  }, [])
  useEffect(() => {
    if (active.current) void applicationUsage.cancel(active.current).catch(() => {})
  }, [apps, taskId, refreshing, protectionKey])
  const report =
    saved?.apps === apps && saved?.taskId === taskId && saved?.protectionKey === protectionKey
      ? saved.report
      : null
  const fresh = !!report && !refreshing && now >= report.checkedAt && now < report.expiresAt
  async function read() {
    if (active.current || !taskId || refreshing || !apps.length) return
    const requestId = crypto.randomUUID(),
      source = apps,
      task = taskId
    const originalProtection = protectionKey
    active.current = requestId
    setPending(true)
    setStopping(false)
    setError('')
    setSaved(null)
    try {
      const report = await applicationUsage.read(
        task,
        requestId,
        apps.slice(0, 512).map((_, i) => i),
        apps,
      )
      if (
        alive.current &&
        current.current.apps === source &&
        current.current.taskId === task &&
        current.current.protectionKey === originalProtection &&
        !current.current.refreshing
      )
        setSaved({ apps: source, taskId: task, protectionKey: originalProtection, report })
    } catch (e) {
      if (alive.current && current.current.apps === source && current.current.taskId === task)
        setError(String(e).replace(/^Error: /, ''))
    } finally {
      active.current = null
      if (alive.current) {
        setPending(false)
        setStopping(false)
      }
    }
  }
  async function cancel() {
    if (!active.current) return
    setStopping(true)
    try {
      await applicationUsage.cancel(active.current)
    } catch (e) {
      setError(String(e))
      setStopping(false)
    }
  }
  return { report, fresh, pending, stopping, error, read, cancel }
}
