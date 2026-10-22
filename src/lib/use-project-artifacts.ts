import { useEffect, useRef, useState } from 'react'
import * as api from './api'
import { initialProjectFilters, type ProjectArtifactFilters } from './project-artifact-view'
import { readProjectRoots, type ProjectRootsReport } from './project-roots'
import {
  readProjectArtifacts,
  readProjectMeasurement,
  type ProjectArtifactsReport,
  type ProjectMeasurement,
} from './project-artifacts'
export function useProjectArtifacts(active: boolean, home: string) {
  const [loadingRoots, setLoadingRoots] = useState(false)
  const [roots, setRoots] = useState<ProjectRootsReport | null>(null)
  const [rootsError, setRootsError] = useState('')
  const [filters, setFilters] = useState<ProjectArtifactFilters>(initialProjectFilters)
  const [root, setRoot] = useState('')
  const [report, setReport] = useState<ProjectArtifactsReport | null>(null)
  const [reading, setReading] = useState(false)
  const [choosing, setChoosing] = useState(false)
  const [error, setError] = useState('')
  const [measuring, setMeasuring] = useState('')
  const [measurements, setMeasurements] = useState<
    Record<string, { report?: ProjectMeasurement; error?: string }>
  >({})
  const epoch = useRef(0),
    running = useRef(false),
    picking = useRef(false)
  useEffect(() => {
    if (!active) {
      epoch.current++
      running.current = false
      picking.current = false
      setLoadingRoots(false)
      setMeasuring('')
      setReading(false)
      setChoosing(false)
    }
    return () => {
      epoch.current++
    }
  }, [active])
  function selectRoot(path: string) {
    if (!active || running.current || picking.current || !roots?.paths.includes(path)) return
    epoch.current++
    setRoot(path)
    setFilters(initialProjectFilters)
    setReport(null)
    setMeasurements({})
    setError('')
  }
  async function loadRoots() {
    if (!active || !home || running.current || picking.current) return
    running.current = true
    setLoadingRoots(true)
    setRoots(null)
    setRootsError('')
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.readProjectRoots(),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('Mole 目录读取未及时返回，系统读取可能仍在结束，请稍后重试。')),
            8000,
          )
        }),
      ])
      if (ticket === epoch.current) setRoots(readProjectRoots(value, home, began))
    } catch (error) {
      if (ticket === epoch.current) setRootsError(String(error))
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        running.current = false
        setLoadingRoots(false)
      }
    }
  }
  async function choose() {
    if (!active || running.current || picking.current) return
    picking.current = true
    setChoosing(true)
    setError('')
    const ticket = ++epoch.current
    try {
      const selected = await api.chooseDirectory()
      if (ticket === epoch.current && selected) {
        setRoot(selected.replace(/\/+$/, '') || '/')
        setFilters(initialProjectFilters)
        setReport(null)
        setMeasurements({})
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
    setMeasurements({})
    setError('')
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.inspectProjectArtifacts(root),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('工程扫描未及时返回，系统读取可能仍在结束，请稍后重试。')),
            8000,
          )
        }),
      ])
      if (ticket === epoch.current) {
        setReport(readProjectArtifacts(value, root, began))
        setFilters((current) => ({ ...current, page: 1 }))
      }
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
  async function measure(path: string) {
    if (
      !active ||
      !report?.items.some((item) => item.path === path) ||
      running.current ||
      picking.current
    )
      return
    running.current = true
    setMeasuring(path)
    setMeasurements((old) => ({ ...old, [path]: { report: old[path]?.report } }))
    const ticket = ++epoch.current,
      began = Date.now()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const value = await Promise.race([
        api.measureProjectArtifact(root, path),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(new Error('测量未及时返回，系统读取可能仍在结束，请稍后重试。')),
            8000,
          )
        }),
      ])
      if (ticket === epoch.current) {
        const result = readProjectMeasurement(value, root, path, began)
        setMeasurements((old) => ({ ...old, [path]: { report: result } }))
      }
    } catch (error) {
      if (ticket === epoch.current)
        setMeasurements((old) => ({
          ...old,
          [path]: { report: old[path]?.report, error: String(error) },
        }))
    } finally {
      clearTimeout(timer)
      if (ticket === epoch.current) {
        running.current = false
        setMeasuring('')
      }
    }
  }
  return {
    root,
    filters,
    setFilters,
    report,
    reading,
    choosing,
    error,
    choose,
    scan,
    measuring,
    measurements,
    measure,
    roots,
    rootsError,
    loadingRoots,
    loadRoots,
    selectRoot,
  }
}
