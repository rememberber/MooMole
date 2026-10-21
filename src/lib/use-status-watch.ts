import { useEffect, useRef, useState } from 'react'
import type { Snapshot } from '../types'
import * as api from './api'
import { followStatus, type StatusInterval } from './status-watch'
import { appendStatusSample, type StatusSample } from './status-metrics'

export function useStatusWatch(enabled: boolean, receive: (snapshot: Snapshot) => void) {
  const active = useRef<AbortController | null>(null)
  const [phase, setPhase] = useState<'idle' | 'starting' | 'running' | 'stopping'>('idle')
  const [error, setError] = useState('')
  const [receivedAt, setReceivedAt] = useState<number | null>(null)
  const [now, setNow] = useState(Date.now())
  const [stopped, setStopped] = useState(false)
  const [samples, setSamples] = useState<StatusSample[]>([])
  const [interval, setIntervalSeconds] = useState<StatusInterval>(2)
  function chooseInterval(value: StatusInterval) {
    if (!active.current && [1, 2, 5].includes(value)) setIntervalSeconds(value)
  }
  function clearHistory() {
    setSamples([])
    setStopped(false)
    setReceivedAt(null)
  }
  function pause() {
    if (active.current) {
      setPhase('stopping')
      active.current.abort()
    }
  }
  useEffect(() => {
    if (!enabled) pause()
    const hide = () => {
      if (document.hidden) pause()
    }
    const leave = () => pause()
    document.addEventListener('visibilitychange', hide)
    window.addEventListener('pagehide', leave)
    return () => {
      document.removeEventListener('visibilitychange', hide)
      window.removeEventListener('pagehide', leave)
    }
  }, [enabled])
  useEffect(() => {
    if (phase !== 'running') return
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [phase])
  useEffect(() => () => active.current?.abort(), [])
  async function start() {
    if (active.current || !enabled || document.hidden) return
    const controller = new AbortController()
    active.current = controller
    setPhase('starting')
    setError('')
    setReceivedAt(null)
    setStopped(false)
    setSamples([])
    try {
      let sequence = 0
      await followStatus(
        api.statusTransport,
        crypto.randomUUID(),
        controller.signal,
        (value) => {
          setPhase(value.state === 'running' ? 'running' : 'starting')
          if (value.data && value.sequence > sequence) {
            sequence = value.sequence
            receive(value.data)
            setSamples((current) => appendStatusSample(current, value))
            setReceivedAt(value.receivedAt)
            setNow(Date.now())
          }
        },
        interval,
      )
    } catch (error) {
      setError(String(error).replace(/^Error: /, ''))
    } finally {
      if (active.current === controller) {
        active.current = null
        setPhase('idle')
        setStopped(true)
      }
    }
  }
  const stale = receivedAt != null && (now - receivedAt > 10_000 || now < receivedAt)
  const message =
    error ||
    (phase === 'starting'
      ? '正在启动采集，等待第一份状态。'
      : phase === 'stopping'
        ? '正在停止采集，当前显示最后快照。'
        : phase === 'running'
          ? stale
            ? '暂未收到新状态，当前快照可能已过时。'
            : `持续采集中 · 约每 ${interval} 秒更新`
          : stopped
            ? '采集已暂停，当前显示最后快照。'
            : '快照仅反映采集当时；可开始持续采集。')
  return {
    phase,
    error,
    message,
    start,
    pause,
    samples,
    stale,
    clearHistory,
    interval,
    chooseInterval,
  }
}
