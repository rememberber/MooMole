import { useCallback, useEffect, useRef, useState } from 'react'
import * as api from './api'
import type { AiAgent, AiSnapshot, AiTransport } from './ai-scan'

export function useAiScan(home: string, transport: AiTransport = api.aiTransport) {
  const [agents, setAgents] = useState<AiAgent[]>([])
  const [agentId, setAgentId] = useState('')
  const [root, setRoot] = useState('')
  const [consent, setConsent] = useState(false)
  const [snapshot, setSnapshot] = useState<AiSnapshot | null>(null)
  const [error, setError] = useState('')
  const [trackingError, setTrackingError] = useState('')
  const [detecting, setDetecting] = useState(false)
  const [starting, setStarting] = useState(false)
  const [stopping, setStopping] = useState(false)
  const startLock = useRef(false)
  const mounted = useRef(true)
  const detectLock = useRef(false)
  useEffect(() => {
    setRoot((value) => value || (home ? `${home}/Downloads` : ''))
  }, [home])
  useEffect(() => {
    setConsent(false)
  }, [root, agentId])
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  const detect = useCallback(async () => {
    if (!api.desktop || detectLock.current) return
    detectLock.current = true
    setDetecting(true)
    setError('')
    try {
      const result = await transport.detect()
      if (mounted.current) {
        setAgents(result)
        setAgentId((value) =>
          result.some((a) => a.id === value && a.path && a.supported)
            ? value
            : (result.find((a) => a.path && a.supported)?.id ?? ''),
        )
      }
    } catch (e) {
      if (mounted.current) setError(String(e))
    } finally {
      detectLock.current = false
      if (mounted.current) setDetecting(false)
    }
  }, [transport])
  useEffect(() => {
    if (!api.desktop) return
    let disposed = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const read = async () => {
      try {
        const value = await transport.read()
        if (disposed) return
        setSnapshot(value)
        setTrackingError('')
        if (value?.state === 'running') timer = setTimeout(() => void read(), 700)
        else if (value) {
          const remaining = (value.finishedAt ?? value.startedAt) + 15 * 60_000 - Date.now()
          timer = setTimeout(() => void read(), Math.max(1000, remaining + 50))
        }
      } catch (e) {
        if (!disposed) {
          setTrackingError(`状态读取失败，正在重试接回：${String(e)}`)
          timer = setTimeout(() => void read(), 2000)
        }
      }
    }
    void read()
    return () => {
      disposed = true
      clearTimeout(timer)
    }
  }, [transport, starting])
  const running = starting || snapshot?.state === 'running'
  useEffect(() => {
    if (!running) setStopping(false)
  }, [running])
  const ready = agents.some((a) => a.id === agentId && a.path && a.supported)
  async function start() {
    if (startLock.current || running || !ready || !consent || !root.trim()) return
    startLock.current = true
    setStarting(true)
    setError('')
    setSnapshot(null)
    try {
      const value = await transport.start(agentId, root.trim(), consent)
      if (mounted.current) setSnapshot(value)
    } catch (e) {
      if (mounted.current) setError(String(e).replace(/^Error: /, ''))
    } finally {
      startLock.current = false
      if (mounted.current) setStarting(false)
    }
  }
  async function cancel() {
    if (!snapshot || stopping) return
    setStopping(true)
    try {
      await transport.cancel(snapshot.id)
    } catch (e) {
      setError(String(e))
      setStopping(false)
    }
  }
  return {
    agents,
    agentId,
    setAgentId,
    root,
    setRoot,
    consent,
    setConsent,
    snapshot,
    error: error || trackingError,
    detecting,
    starting,
    stopping,
    running,
    ready,
    detect,
    start,
    cancel,
  }
}
export type AiScanController = ReturnType<typeof useAiScan>
