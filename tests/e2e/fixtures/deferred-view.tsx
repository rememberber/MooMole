import { useImperativeHandle, useRef, useState, type Ref } from 'react'
import { createRoot } from 'react-dom/client'
import { deferredView } from '../../../src/components/DeferredView'
let finish: ((fail: boolean) => void) | undefined
let loads = 0
function Content({
  value,
  ref,
}: {
  value: number
  ref?: Ref<{ capture: () => number }>
  onClose: () => void
}) {
  useImperativeHandle(ref, () => ({ capture: () => value }), [value])
  return <p>子界面结果 {value}</p>
}
const Deferred = deferredView(
  async () => {
    loads++
    await new Promise<void>((resolve, reject) => {
      finish = (fail) => (fail ? reject(new Error('fixture view load rejected')) : resolve())
    })
    return { default: Content }
  },
  { onClose: (props) => props.onClose() },
)
function Fixture() {
  const [value, setValue] = useState(0)
  const [visible, setVisible] = useState(true)
  const [capture, setCapture] = useState<number | undefined>()
  const child = useRef<{ capture: () => number }>(null)
  return (
    <section aria-label="延迟界面测试">
      <p>父状态 {value}</p>
      <p>引用结果 {capture ?? '未读取'}</p>
      <button onClick={() => setValue((v) => v + 1)}>更新父状态</button>
      <button onClick={() => finish?.(true)}>拒绝加载</button>
      <button onClick={() => finish?.(false)}>完成加载</button>
      <button onClick={() => setVisible((v) => !v)}>切换子界面</button>
      <button onClick={() => setCapture(child.current?.capture())}>读取子界面状态</button>
      {visible && <Deferred onClose={() => setVisible(false)} value={value} ref={child} />}
    </section>
  )
}
export const loadCount = () => loads
export function mount() {
  const host = document.createElement('div')
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px'
  document.body.append(host)
  createRoot(host).render(<Fixture />)
}
