import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import { readMaintenanceReport, type MaintenanceReport } from './maintenance'
import {
  readMaintenanceReview,
  maintenanceLifetime,
  type MaintenanceId,
  type MaintenanceReview,
} from './maintenance-review'

export function useMaintenance(active: boolean) {
  const [report, setReport] = useState<MaintenanceReport | null>(null)
  const [reading, setReading] = useState(false)
  const [error, setError] = useState('')
  const [selected, setSelected] = useState<MaintenanceId[]>([])
  const [review, setReview] = useState<MaintenanceReview | null>(null)
  const [reviewing, setReviewing] = useState(false)
  const [reviewError, setReviewError] = useState('')
  const epoch = useRef(0)
  const running = useRef(false)
  const reviewRunning = useRef(false)
  useEffect(() => {
    if (!active) {
      epoch.current++
      running.current = false
      reviewRunning.current = false
      setReading(false)
      setSelected([])
      setReview(null)
      setReviewing(false)
      setReviewError('')
    }
    return () => {
      epoch.current++
    }
  }, [active])
  async function inspect() {
    if (running.current || reviewRunning.current || !active) return
    running.current = true
    const ticket = ++epoch.current
    setReading(true)
    setError('')
    setSelected([])
    setReview(null)
    setReviewError('')
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
  function select(id: MaintenanceId) {
    if (reading || reviewing || !report || !report.items.some((item) => item.id === id)) return
    setSelected((previous) =>
      previous.includes(id) ? previous.filter((value) => value !== id) : [...previous, id],
    )
    setReviewError('')
  }
  async function prepareReview() {
    if (!active || running.current || reviewRunning.current || !report || selected.length === 0)
      return
    if (Date.now() < report.checkedAt || Date.now() >= report.checkedAt + maintenanceLifetime) {
      setReviewError('维护检查已过期或时间变化，请重新检查后审阅。')
      return
    }
    const ticket = ++epoch.current
    reviewRunning.current = true
    const began = Date.now()
    setReviewing(true)
    setReviewError('')
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.prepareMaintenanceReview(report.observationId, selected),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => reject(new Error('维护条件审阅未及时返回，请稍后重试。')), 5_000)
        }),
      ])
      if (ticket === epoch.current) setReview(readMaintenanceReview(value, report, selected, began))
    } catch (error) {
      if (ticket === epoch.current) setReviewError(String(error))
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        reviewRunning.current = false
        setReviewing(false)
      }
    }
  }
  return {
    report,
    reading,
    error,
    inspect,
    selected,
    select,
    review,
    reviewing,
    reviewError,
    prepareReview,
    closeReview: () => setReview(null),
  }
}
