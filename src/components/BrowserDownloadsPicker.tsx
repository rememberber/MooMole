import { useEffect, useRef, useState } from 'react'
import {
  browserHistoryPath,
  downloadBrowsers,
  listBrowserProfiles,
  type BrowserProfile,
  type DownloadBrowser,
} from '../lib/browser-downloads'

export function BrowserDownloadsPicker({
  home,
  value,
  onChange,
  disabled,
}: {
  home: string
  value: BrowserProfile | null
  onChange: (value: BrowserProfile | null) => void
  disabled: boolean
}) {
  const [browser, setBrowser] = useState<DownloadBrowser>(value?.browser ?? 'chrome')
  const [profiles, setProfiles] = useState<Awaited<ReturnType<typeof listBrowserProfiles>> | null>(
    null,
  )
  const [reading, setReading] = useState(false),
    [error, setError] = useState('')
  const [epoch, setEpoch] = useState(0)
  const loadedKey = useRef('')
  useEffect(() => {
    if (disabled || !home) return
    const key = JSON.stringify([browser, home, epoch])
    if (loadedKey.current === key) return
    let live = true
    setProfiles(null)
    setError('')
    setReading(true)
    onChange(null)
    void listBrowserProfiles(browser, home)
      .then((result) => {
        if (!live) return
        loadedKey.current = key
        setProfiles(result)
        // Default is the historical source. Other configurations always require
        // an explicit selection; no automatic first-profile fallback.
        if (browser !== 'firefox' && result.profiles.some((p) => p.selection.profile === 'Default'))
          onChange({ browser, profile: 'Default' })
      })
      .catch((reason) => {
        if (live) setError(String(reason).replace(/^Error: /, ''))
      })
      .finally(() => {
        if (live) setReading(false)
      })
    return () => {
      live = false
    }
  }, [browser, home, epoch, onChange, disabled])
  return (
    <div className="browser-downloads-picker">
      <div className="personal-scope">
        <label className="software-source">
          浏览器
          <select
            aria-label="浏览器"
            value={browser}
            disabled={disabled || reading}
            onChange={(e) => {
              onChange(null)
              setProfiles(null)
              setBrowser(e.target.value as DownloadBrowser)
            }}
          >
            {downloadBrowsers.map((b) => (
              <option key={b.id} value={b.id}>
                {b.label}
              </option>
            ))}
          </select>
        </label>
        <label className="software-source">
          配置目录
          <select
            aria-label="浏览器配置目录"
            value={value?.profile ?? ''}
            disabled={disabled || reading || !profiles?.profiles.length}
            onChange={(e) => onChange(e.target.value ? { browser, profile: e.target.value } : null)}
          >
            <option value="">选择配置目录</option>
            {profiles?.profiles.map((p) => (
              <option key={p.selection.profile} value={p.selection.profile}>
                {p.selection.profile}
              </option>
            ))}
          </select>
        </label>
        <button
          className="button secondary compact"
          disabled={disabled || reading}
          onClick={() => setEpoch((v) => v + 1)}
        >
          {reading ? '正在检查配置…' : '重新检测配置'}
        </button>
      </div>
      {value && (
        <p className="muted mono browser-history-path">{browserHistoryPath(home, value)}</p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {profiles && !profiles.profiles.length && (
        <p className="muted" role="status">
          未发现可读取的已支持配置；这不代表浏览器没有下载文件。
        </p>
      )}
      {profiles && !profiles.complete && (
        <p className="warning" role="status">
          配置检查未完整覆盖，部分配置未读取；未知范围保持原样。
        </p>
      )}
      <p className="muted">
        仅检测所选浏览器的标准配置目录，不读取账户名称。Firefox
        需手动选择配置；访客、无痕、系统配置、其他浏览器及自定义位置尚未支持。
      </p>
    </div>
  )
}
