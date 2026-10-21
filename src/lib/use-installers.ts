import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import {
  readInstallers,
  installerLocations,
  readInstallerZip,
  readInstallerConnection,
  type ConnectionObservations,
  type InstallersReport,
  type ZipObservations,
} from './installers'
export function useInstallers(active: boolean, home: string) {
  const locations = installerLocations(home)
  const [root, setRoot] = useState('')
  const [report, setReport] = useState<InstallersReport | null>(null)
  const [reading, setReading] = useState(false)
  const [choosing, setChoosing] = useState(false)
  const [error, setError] = useState('')
  const [checking, setChecking] = useState('')
  const [zipObservations, setZipObservations] = useState<ZipObservations>({})
  const [connectionObservations, setConnectionObservations] = useState<ConnectionObservations>({})
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
  function selectLocation(id: string) {
    if (!active || running.current || picking.current) return
    const selected = locations.find((location) => location.id === id)
    if (!selected) return
    epoch.current++
    setRoot(selected.path)
    setReport(null)
    setZipObservations({})
    setConnectionObservations({})
    setError('')
  }
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
        setConnectionObservations({})
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
    setConnectionObservations({})
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
  async function checkFile(path: string, kind: 'zip' | 'connection') {
    if (
      !active ||
      !report?.items.some(
        (item) =>
          item.path === path &&
          item.sharing === 'single' &&
          (kind === 'zip' ? item.extension === 'zip' : ['dmg', 'iso'].includes(item.extension)),
      ) ||
      running.current ||
      picking.current
    )
      return
    const label = kind === 'zip' ? 'ZIP' : '映像连接'
    running.current = true
    setChecking(path)
    if (kind === 'zip') setZipObservations((old) => ({ ...old, [path]: {} }))
    else setConnectionObservations((old) => ({ ...old, [path]: {} }))
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        kind === 'zip'
          ? api.inspectInstallerZip(root, path)
          : api.inspectInstallerConnection(root, path),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error(`${label}检查未及时结束，系统读取可能仍在结束，请稍后重试。`)),
            14000,
          )
        }),
      ])
      if (ticket === epoch.current) {
        if (kind === 'zip') {
          const result = readInstallerZip(value, root, path, began)
          setZipObservations((old) => ({ ...old, [path]: { report: result } }))
        } else {
          const result = readInstallerConnection(value, root, path, began)
          setConnectionObservations((old) => ({ ...old, [path]: { report: result } }))
        }
      }
    } catch (error) {
      if (ticket === epoch.current) {
        if (kind === 'zip')
          setZipObservations((old) => ({ ...old, [path]: { error: String(error) } }))
        else setConnectionObservations((old) => ({ ...old, [path]: { error: String(error) } }))
      }
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
    checkZip: (path: string) => checkFile(path, 'zip'),
    checkConnection: (path: string) => checkFile(path, 'connection'),
    connectionObservations,
    locations,
    selectLocation,
    locationId: locations.find((location) => location.path === root)?.id ?? '',
  }
}
