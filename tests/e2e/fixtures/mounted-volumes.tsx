import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { MountedVolumePicker } from '../../../src/components/MountedVolumePicker'
export function mount(mode = 'normal') {
  const host = document.createElement('div')
  host.id = 'mounted-volume-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:20px;overflow:auto'
  document.body.append(host)
  const calls: { command: string; args: Record<string, unknown> }[] = []
  let reads = 0
  host.dataset.calls = '[]'
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        calls.push({ command, args })
        host.dataset.calls = JSON.stringify(calls)
        if (command === 'get_mounted_volumes') {
          reads++
          host.dataset.reads = String(reads)
          const createdAt = Date.now() - 1000
          return {
            version: 1,
            createdAt,
            expiresAt: createdAt + 120_000,
            complete: mode !== 'partial',
            volumes: [
              {
                id: '31d8c490-1245-414b-8d62-0c6aab303555',
                kind: 'startup',
                name: '',
                path: '/',
                fileSystem: 'apfs',
                readOnly: true,
                totalBytes: 100_000_000,
                availableBytes: 30_000_000,
                canAnalyze: true,
              },
              {
                id: '21d8c490-1245-414b-8d62-0c6aab303555',
                kind: 'account',
                name: '',
                path: '/Users/demo',
                fileSystem: 'apfs',
                readOnly: false,
                totalBytes: 100_000_000,
                availableBytes: 30_000_000,
                canAnalyze: true,
              },
              {
                id: '41d8c490-1245-414b-8d62-0c6aab303555',
                kind: 'mounted',
                name: '中文 外接盘',
                path: '/Volumes/中文 外接盘',
                fileSystem: 'apfs',
                readOnly: false,
                totalBytes: mode === 'unknown' ? null : 200_000_000,
                availableBytes:
                  mode === 'bad' ? 300_000_000 : mode === 'unknown' ? null : 60_000_000,
                canAnalyze: true,
              },
              {
                id: '51d8c490-1245-414b-8d62-0c6aab303555',
                kind: 'mounted',
                name: '只读镜像',
                path: '/Volumes/只读镜像',
                fileSystem: 'apfs',
                readOnly: true,
                totalBytes: 10_000_000,
                availableBytes: 0,
                canAnalyze: true,
              },
              {
                id: '61d8c490-1245-414b-8d62-0c6aab303555',
                kind: 'mounted',
                name: '暂不可读',
                path: '/Volumes/暂不可读',
                fileSystem: null,
                readOnly: null,
                totalBytes: null,
                availableBytes: null,
                canAnalyze: false,
              },
            ],
          }
        }
        if (command === 'resolve_mounted_volume') {
          if (mode === 'changed') throw Error('磁盘或挂载位置已变化，请刷新后重新选择。')
          if (mode === 'mismatch') return '/Volumes/Other'
          if (args.id === '31d8c490-1245-414b-8d62-0c6aab303555') return '/'
          if (args.id === '21d8c490-1245-414b-8d62-0c6aab303555') return '/Users/demo'
          return args.id === '51d8c490-1245-414b-8d62-0c6aab303555'
            ? '/Volumes/只读镜像'
            : '/Volumes/中文 外接盘'
        }
        throw Error(command)
      },
    },
  })
  function Fixture() {
    const [signal, setSignal] = useState(0)
    return (
      <>
        <button onClick={() => setSignal((n) => n + 1)}>选择磁盘</button>
        <MountedVolumePicker
          openSignal={signal}
          enabled
          onAnalyze={(path) => {
            host.dataset.selected = path
          }}
        />
      </>
    )
  }
  createRoot(host).render(<Fixture />)
}
