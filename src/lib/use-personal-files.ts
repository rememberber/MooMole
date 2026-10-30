import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import {
  personalTransport,
  readPersonalConfirmation,
  readPersonalSnapshot,
  type PersonalSnapshot,
  type PersonalTransport,
  type PersonalMovement,
} from './personal-files'
import type { ExecutionDetail } from '../types'

const idle: PersonalSnapshot = {
  phase: 'idle',
  requestId: null,
  runId: null,
  report: null,
  error: null,
}
export function usePersonalFiles(
  enabled: boolean,
  home: string,
  transport: PersonalTransport = personalTransport,
  initialDirectory = 'Downloads',
) {
  const [awake, setAwake] = useState(false),
    [epoch, setEpoch] = useState(0)
  const [root, setRootValue] = useState(''),
    [snapshot, setSnapshot] = useState(idle)
  const [result, setResult] = useState<ExecutionDetail | null>(null)
  const [starting, setStarting] = useState(false),
    [stopping, setStopping] = useState(false)
  const [error, setError] = useState(''),
    [trackingError, setTrackingError] = useState('')
  const lock = useRef(false),
    generation = useRef(0),
    rootTouched = useRef(false)
  const setRoot = (value: string) => {
    rootTouched.current = true
    setRootValue(value)
  }
  useEffect(() => {
    if (enabled) setAwake(true)
  }, [enabled])
  useEffect(() => {
    setRootValue((value) => value || (home ? `${home}/${initialDirectory}` : ''))
  }, [home, initialDirectory])
  useEffect(() => {
    if (!api.desktop || !awake) return
    let closed = false,
      timer: ReturnType<typeof setTimeout> | undefined
    const current = ++generation.current
    async function read() {
      try {
        const next = readPersonalSnapshot(await transport.read())
        if (closed || generation.current !== current) return
        if (next.report && !rootTouched.current) setRootValue(next.report.root)
        setSnapshot(next)
        if (next.runId) {
          const detail = await transport.detail(next.runId)
          if (detail.id !== next.runId || detail.origin !== (transport.origin ?? 'personalFiles'))
            throw Error('返回的个人文件批次不匹配。')
          if (closed || generation.current !== current) return
          setResult(detail)
        }
        setTrackingError('')
        if (!['scanning', 'processing'].includes(next.phase)) setStopping(false)
        if (['scanning', 'processing'].includes(next.phase))
          timer = setTimeout(() => void read(), 700)
      } catch (reason) {
        if (!closed && generation.current === current) {
          setTrackingError(
            `个人文件状态读取失败，正在接回原任务：${String(reason).replace(/^Error: /, '')}`,
          )
          timer = setTimeout(() => void read(), 2000)
        }
      }
    }
    void read()
    return () => {
      closed = true
      clearTimeout(timer)
    }
  }, [awake, epoch, transport])
  const running = starting || ['scanning', 'processing'].includes(snapshot.phase)
  async function dispatch(
    phase: 'scanning' | 'processing',
    launch: (requestId: string) => Promise<void>,
  ) {
    if (lock.current || running) return
    lock.current = true
    setStarting(true)
    setError('')
    setTrackingError('')
    setStopping(false)
    const requestId = crypto.randomUUID()
    ++generation.current
    try {
      await launch(requestId)
      setSnapshot({ ...idle, phase, requestId })
      setResult(null)
      setAwake(true)
      setEpoch((value) => value + 1)
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
      setEpoch((value) => value + 1)
    } finally {
      lock.current = false
      setStarting(false)
    }
  }
  async function prepare(ids: string[], movement?: PersonalMovement) {
    const report = snapshot.report
    if (!report || running) throw Error('请等待扫描结束后重新选择。')
    const raw = await transport.prepare(report.id, ids, movement)
    return readPersonalConfirmation(raw, report, ids, Date.now(), movement)
  }
  async function cancel() {
    if (!snapshot.requestId || stopping) return
    setStopping(true)
    try {
      await transport.cancel(snapshot.requestId)
      setEpoch((value) => value + 1)
    } catch (reason) {
      setError(String(reason).replace(/^Error: /, ''))
      setStopping(false)
    }
  }
  return {
    root,
    setRoot,
    snapshot,
    result,
    running,
    starting,
    stopping,
    error: error || trackingError || snapshot.error || '',
    start: () => dispatch('scanning', (id) => transport.start(root.trim(), id)),
    fromAi: (snapshotId: string, evidenceIds: string[]) =>
      dispatch('scanning', async (requestId) => {
        if (!transport.startFromAi || (transport.origin && transport.origin !== 'personalFiles'))
          throw Error('此入口不能接收 AI 个人文件线索。')
        const nextRoot = await transport.startFromAi(snapshotId, evidenceIds, requestId)
        if (
          typeof nextRoot !== 'string' ||
          !nextRoot.startsWith('/') ||
          nextRoot.length > 4096 ||
          /[\x00-\x1f\x7f]/.test(nextRoot)
        )
          throw Error('AI 文件整理目录无法核对。')
        setRoot(nextRoot)
      }),
    execute: (confirmationId: string, moving = false) =>
      dispatch('processing', (id) => transport.execute(confirmationId, id, moving)),
    prepare,
    discard: transport.discard,
    cancel,
  }
}
export type PersonalController = ReturnType<typeof usePersonalFiles>
