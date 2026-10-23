import { StrictMode, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Modal } from '../../../src/components/Modal'
import { focusReturnTarget } from '../../../src/lib/focus-return'
import { ExecutionHistory } from '../../../src/components/ExecutionHistory'
import { cancelled } from './execution-history'

let clearExecutionRecords: () => void = () => {}
export const removeExecutionRecords = () => clearExecutionRecords()

function Fixture() {
  const [visible, setVisible] = useState(false)
  const [child, setChild] = useState(false)
  const [replacement, setReplacement] = useState(false)
  const [required, setRequired] = useState(false)
  const [entryState, setEntryState] = useState('normal')
  const [executionRecords, setExecutionRecords] = useState([cancelled])
  clearExecutionRecords = () => setExecutionRecords([])
  const entry = useRef<HTMLButtonElement>(null)
  const fallback = useRef<HTMLButtonElement>(null)
  const returnFocus = () => focusReturnTarget(entry.current, fallback.current)
  return (
    <section aria-label="弹窗焦点测试">
      <input aria-label="其他输入" />
      {entryState !== 'missing' && (
        <button
          ref={entry}
          hidden={entryState === 'hidden'}
          disabled={entryState === 'disabled'}
          onClick={() => setVisible(true)}
        >
          打开测试弹窗
        </button>
      )}
      <button ref={fallback}>备用入口</button>
      <button onClick={() => setRequired(true)}>打开必读说明</button>
      {visible && (
        <Modal title="测试弹窗" onClose={() => setVisible(false)} returnFocus={returnFocus}>
          <h2>测试弹窗</h2>
          <button onClick={() => setChild(true)}>打开内层弹窗</button>
          {['hidden', 'disabled', 'missing'].map((state) => (
            <button key={state} onClick={() => setEntryState(state)}>
              入口设为 {state}
            </button>
          ))}
          <button
            onClick={() => {
              setVisible(false)
              setReplacement(true)
            }}
          >
            切换到另一弹窗
          </button>
          {child && (
            <Modal title="内层弹窗" onClose={() => setChild(false)}>
              <h2>内层弹窗</h2>
            </Modal>
          )}
        </Modal>
      )}
      {replacement && (
        <Modal title="另一弹窗" onClose={() => setReplacement(false)}>
          <h2>另一弹窗</h2>
        </Modal>
      )}
      {required && (
        <Modal title="必读说明">
          <h2>必读说明</h2>
          <button onClick={() => setRequired(false)}>已阅读说明</button>
        </Modal>
      )}
      <ExecutionHistory
        initialRecords={executionRecords}
        loadRecords={async () => executionRecords}
        loadDetail={async () => {
          if (sessionStorage.getItem('focus-detail-fail')) throw new Error('fixture read failed')
          return cancelled
        }}
      />
    </section>
  )
}
export function mount() {
  const host = document.createElement('div')
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px;overflow:auto'
  document.body.append(host)
  createRoot(host).render(
    <StrictMode>
      <Fixture />
    </StrictMode>,
  )
}
