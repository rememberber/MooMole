import { useEffect, useRef, useState } from 'react'
import {
  readScheduledFiles,
  type ScheduledFilesView,
  type ScheduledFilesTransport,
  type ScheduledFileRule,
  type ScheduledFileDraft,
} from './scheduled-files'
export function useScheduledFiles(enabled: boolean, transport: ScheduledFilesTransport) {
  const [view, setView] = useState<ScheduledFilesView | null>(null),
    [readingError, setReadingError] = useState(''),
    [actionError, setActionError] = useState(''),
    [pending, setPending] = useState(false),
    [epoch, setEpoch] = useState(0),
    busy = useRef(false)
  useEffect(() => {
    if (!enabled) return
    let live = true,
      timer: ReturnType<typeof setTimeout> | undefined
    async function read() {
      let active = false
      try {
        const v = readScheduledFiles(await transport.read())
        if (live) {
          setView(v)
          setReadingError('')
          active = v.activeRequests.length > 0
        }
      } catch (e) {
        if (live) setReadingError(String(e))
      }
      if (live) timer = setTimeout(() => void read(), active ? 700 : 10000)
    }
    void read()
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [enabled, transport, epoch])
  async function act<T>(action: () => Promise<T>) {
    if (!enabled || busy.current) throw Error('请等待定期处理设置操作结束。')
    busy.current = true
    setPending(true)
    setActionError('')
    try {
      return await action()
    } catch (e) {
      setActionError(String(e))
      throw e
    } finally {
      busy.current = false
      setPending(false)
      setEpoch((n) => n + 1)
    }
  }
  return {
    view,
    error: actionError || readingError || view?.error || '',
    pending,
    running: !!view?.activeRequests.length,
    refresh: () => setEpoch((n) => n + 1),
    preview: (id: string | null, r: ScheduledFileRule) => act(() => transport.preview(id, r)),
    discard: transport.discard,
    enable: (draft: ScheduledFileDraft) => act(() => transport.enable(draft)),
    pause: (id: string) => act(() => transport.pause(id)),
    remove: (id: string) => act(() => transport.remove(id)),
    run: (id: string) => act(() => transport.run(id, crypto.randomUUID())),
    cancel: (id: string) => act(() => transport.cancel(id)),
  }
}
export type ScheduledFilesController = ReturnType<typeof useScheduledFiles>
