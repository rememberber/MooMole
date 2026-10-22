import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import {
  readInstallers,
  installerLocations,
  installerCatalog,
  readInstallerZip,
  readInstallerZipData,
  readInstallerImage,
  type ImageObservations,
  type ZipDataObservations,
  readInstallerConnection,
  readInstallerUsage,
  type UsageObservations,
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
  const [checkKind, setCheckKind] = useState('')
  const [usageObservations, setUsageObservations] = useState<UsageObservations>({})
  const [imageObservations, setImageObservations] = useState<ImageObservations>({})
  const [zipDataObservations, setZipDataObservations] = useState<ZipDataObservations>({})
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
      setCheckKind('')
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
    setZipDataObservations({})
    setImageObservations({})
    setConnectionObservations({})
    setUsageObservations({})
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
        setZipDataObservations({})
        setImageObservations({})
        setConnectionObservations({})
        setUsageObservations({})
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
    setZipDataObservations({})
    setImageObservations({})
    setConnectionObservations({})
    setUsageObservations({})
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
  async function checkFile(
    path: string,
    kind: 'zip' | 'connection' | 'usage' | 'zipData' | 'image',
  ) {
    const extensions = {
      zip: ['zip'],
      zipData: ['zip'],
      image: ['dmg'],
      connection: ['dmg', 'iso'],
      usage: installerCatalog.extensions,
    }[kind]
    if (
      !active ||
      !report?.items.some(
        (item) =>
          item.path === path && item.sharing === 'single' && extensions.includes(item.extension),
      ) ||
      (kind === 'zipData' &&
        (!zipObservations[path]?.report || zipObservations[path].report!.state === 'unknown')) ||
      running.current ||
      picking.current
    )
      return
    const expectedStamp = zipObservations[path]?.report?.fileStamp ?? ''
    const label = {
      zip: 'ZIP',
      connection: '映像连接',
      usage: '文件占用',
      zipData: 'ZIP 数据',
      image: 'DMG 校验',
    }[kind]
    running.current = true
    setChecking(path)
    setCheckKind(kind)
    if (kind === 'zip') {
      setZipObservations((old) => ({ ...old, [path]: {} }))
      setZipDataObservations((old) => {
        const next = { ...old }
        delete next[path]
        return next
      })
    } else if (kind === 'image') setImageObservations((old) => ({ ...old, [path]: {} }))
    else if (kind === 'zipData') setZipDataObservations((old) => ({ ...old, [path]: {} }))
    else if (kind === 'connection') setConnectionObservations((old) => ({ ...old, [path]: {} }))
    else setUsageObservations((old) => ({ ...old, [path]: {} }))
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        kind === 'zip'
          ? api.inspectInstallerZip(root, path)
          : kind === 'connection'
            ? api.inspectInstallerConnection(root, path)
            : kind === 'usage'
              ? api.inspectInstallerUsage(root, path)
              : kind === 'image'
                ? api.inspectInstallerImage(root, path)
                : api.inspectInstallerZipData(root, path, expectedStamp),
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
        } else if (kind === 'connection') {
          const result = readInstallerConnection(value, root, path, began)
          setConnectionObservations((old) => ({ ...old, [path]: { report: result } }))
        } else if (kind === 'usage') {
          const result = readInstallerUsage(value, root, path, began)
          setUsageObservations((old) => ({ ...old, [path]: { report: result } }))
        } else if (kind === 'image') {
          const result = readInstallerImage(value, root, path, began)
          setImageObservations((old) => ({ ...old, [path]: { report: result } }))
        } else {
          const result = readInstallerZipData(value, root, path, began, expectedStamp)
          setZipDataObservations((old) => ({ ...old, [path]: { report: result } }))
        }
      }
    } catch (error) {
      if (ticket === epoch.current) {
        if (kind === 'zip')
          setZipObservations((old) => ({ ...old, [path]: { error: String(error) } }))
        else if (kind === 'connection')
          setConnectionObservations((old) => ({ ...old, [path]: { error: String(error) } }))
        else if (kind === 'usage')
          setUsageObservations((old) => ({ ...old, [path]: { error: String(error) } }))
        else if (kind === 'image')
          setImageObservations((old) => ({ ...old, [path]: { error: String(error) } }))
        else {
          setZipDataObservations((old) => ({ ...old, [path]: { error: String(error) } }))
          setZipObservations((old) => {
            const next = { ...old }
            delete next[path]
            return next
          })
        }
      }
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        running.current = false
        setChecking('')
        setCheckKind('')
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
    checkKind,
    usageObservations,
    checkUsage: (path: string) => checkFile(path, 'usage'),
    imageObservations,
    checkImage: (path: string) => checkFile(path, 'image'),
    zipObservations,
    zipDataObservations,
    checkZipData: (path: string) => checkFile(path, 'zipData'),
    checkZip: (path: string) => checkFile(path, 'zip'),
    checkConnection: (path: string) => checkFile(path, 'connection'),
    connectionObservations,
    locations,
    selectLocation,
    locationId: locations.find((location) => location.path === root)?.id ?? '',
  }
}
