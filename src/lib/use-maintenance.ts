import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import { readMaintenanceReport, type MaintenanceReport } from './maintenance'

export function useMaintenance(active: boolean) {
  const [report, setReport] = useState<MaintenanceReport | null>(null)
  const [reading, setReading] = useState(false)
  const [error, setError] = useState('')
  const epoch = useRef(0)
  const running = useRef(false)
  useEffect(() => {
    if (!active) {
      epoch.current++
      running.current = false
      setReading(false)
    }
    return () => {
      epoch.current++
    }
  }, [active])
  async function inspect() {
    if (running.current || !active) return
    running.current = true
    const ticket = ++epoch.current
    setReading(true)
    setError('')
    const began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.inspectMaintenance(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error('维护检查未及时返回，请稍后重试。')), 7_000)
        }),
      ])
      if (ticket === epoch.current) setReport(readMaintenanceReport(value, began))
    } catch (error) {
      if (ticket === epoch.current) setError(String(error))
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        running.current = false
        setReading(false)
      }
    }
  }
  return { report, reading, error, inspect }
}
