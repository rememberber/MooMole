import { useEffect, useRef, useState } from 'react'
import {
  readFolderSchedules,
  type FolderSchedulesView,
  type FolderScheduleTransport,
  type FolderScheduleEdit,
} from './folder-schedules'
export function useFolderSchedules(enabled: boolean, transport: FolderScheduleTransport) {
  const [view, setView] = useState<FolderSchedulesView | null>(null),
    [readingError, setReadingError] = useState(''),
    [actionError, setActionError] = useState(''),
    [pending, setPending] = useState(false),
    [epoch, setEpoch] = useState(0)
  const lock = useRef(false)
  useEffect(() => {
    if (!enabled) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    const load = async () => {
      let active = false
      try {
        const v = readFolderSchedules(await transport.read())
        if (live) {
          setView(v)
          setReadingError('')
          active = v.activeRequests.length > 0
        }
      } catch (e) {
        if (live) setReadingError(String(e))
      }
      if (live) timer = setTimeout(() => void load(), active ? 700 : 10000)
    }
    void load()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [enabled, transport, epoch])
  async function perform<T>(action: () => Promise<T>): Promise<T> {
    if (!enabled || lock.current) throw Error('请等待目录扫描设置操作结束。')
    lock.current = true
    setPending(true)
    setActionError('')
    try {
      return await action()
    } catch (e) {
      setActionError(String(e))
      throw e
    } finally {
      lock.current = false
      setPending(false)
      setEpoch((v) => v + 1)
    }
  }
  return {
    view,
    pending,
    error: actionError || readingError || view?.error || '',
    running: !!view?.activeRequests.length,
    configure: (edit: FolderScheduleEdit) => perform(() => transport.configure(edit)),
    remove: (id: string) => perform(() => transport.remove(id)),
    scan: (id: string) => perform(() => transport.scan(id, crypto.randomUUID())),
    cancel: (id: string) => perform(() => transport.cancel(id)),
    refresh: () => setEpoch((v) => v + 1),
  }
}
export type FolderSchedulesController = ReturnType<typeof useFolderSchedules>
