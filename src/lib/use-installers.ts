import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import {
  readInstallers,
  readInstallerZip,
  type InstallersReport,
  type ZipObservations,
} from './installers'
export function useInstallers(active: boolean) {
  const [root, setRoot] = useState('')
  const [report, setReport] = useState<InstallersReport | null>(null)
  const [reading, setReading] = useState(false)
  const [choosing, setChoosing] = useState(false)
  const [error, setError] = useState('')
  const [checking, setChecking] = useState('')
  const [zipObservations, setZipObservations] = useState<ZipObservations>({})
  const epoch = useRef(0),
    running = useRef(false),
    picking = useRef(false)
  useEffect(() => {
    if (!active) {
      epoch.current++
      running.current = false
      picking.current = false
      setChecking('')
      setReading(false)
      setChoosing(false)
    }
    return () => {
      epoch.current++
    }
  }, [active])
  async function choose() {
    if (!active || running.current || picking.current) return
    picking.current = true
    setChoosing(true)
    setError('')
    const ticket = ++epoch.current
    try {
      const path = await api.chooseDirectory()
      if (ticket === epoch.current && path) {
        setRoot(path.replace(/\/+$/, '') || '/')
        setReport(null)
        setZipObservations({})
      }
    } catch (error) {
      if (ticket === epoch.current) setError(String(error))
    } finally {
      if (ticket === epoch.current) {
        picking.current = false
        setChoosing(false)
      }
    }
  }
  async function scan() {
    if (!active || !root || running.current || picking.current) return
    running.current = true
    setReading(true)
    setZipObservations({})
    setError('')
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.inspectInstallers(root),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('扫描未及时返回，系统读取可能仍在结束，请稍后重试。')),
            8000,
          )
        }),
      ])
      if (ticket === epoch.current) setReport(readInstallers(value, root, began))
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
  async function checkZip(path: string) {
    if (
      !active ||
      !report?.items.some(
        (item) => item.path === path && item.extension === 'zip' && item.sharing === 'single',
      ) ||
      running.current ||
      picking.current
    )
      return
    running.current = true
    setChecking(path)
    setZipObservations((old) => ({ ...old, [path]: {} }))
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.inspectInstallerZip(root, path),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('ZIP 检查未及时结束，系统读取可能仍在结束，请稍后重试。')),
            14000,
          )
        }),
      ])
      if (ticket === epoch.current) {
        const result = readInstallerZip(value, root, path, began)
        setZipObservations((old) => ({ ...old, [path]: { report: result } }))
      }
    } catch (error) {
      if (ticket === epoch.current)
        setZipObservations((old) => ({ ...old, [path]: { error: String(error) } }))
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        running.current = false
        setChecking('')
      }
    }
  }
  return {
    root,
    report,
    reading,
    choosing,
    error,
    choose,
    scan,
    checking,
    zipObservations,
    checkZip,
  }
}
