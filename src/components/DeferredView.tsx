import {
  Component,
  lazy,
  Suspense,
  useMemo,
  useState,
  type ComponentType,
  type ReactNode,
} from 'react'

class ViewBoundary extends Component<
  { children: ReactNode; onRetry: () => void; onClose?: () => void },
  { failed: boolean }
> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  render() {
    if (this.state.failed)
      return (
        <div className="empty-state" role="alert">
          <p>这部分界面暂时无法打开。</p>
          <p className="muted">可重试或切换其他功能；重试不会重新运行检查任务。</p>
          <button className="button secondary compact" onClick={this.props.onRetry}>
            重试打开
          </button>
          {this.props.onClose && (
            <button className="button secondary compact" onClick={this.props.onClose}>
              关闭
            </button>
          )}
        </div>
      )
    return this.props.children
  }
}

// A fresh lazy instance is needed after rejection: React retains the rejected
// loader promise on the previous instance. Parent task/session state stays put.
export function deferredView<P extends object>(
  load: () => Promise<{ default: ComponentType<P> }>,
  options: { onClose?: (props: P) => void } = {},
) {
  let ready: ComponentType<P> | undefined
  const loadView = async () => {
    const module = await load()
    ready = module.default
    return module
  }
  const initialView = lazy(loadView)
  return function DeferredView(props: P) {
    const [attempt, setAttempt] = useState(0)
    const View = useMemo(() => ready ?? (attempt === 0 ? initialView : lazy(loadView)), [attempt])
    const close = options.onClose ? () => options.onClose?.(props) : undefined
    return (
      <ViewBoundary key={attempt} onRetry={() => setAttempt((value) => value + 1)} onClose={close}>
        <Suspense
          fallback={
            <div role="status">
              <p className="muted">正在打开…</p>
              {close && (
                <button className="button secondary compact" onClick={close}>
                  取消打开
                </button>
              )}
            </div>
          }
        >
          <View {...props} />
        </Suspense>
      </ViewBoundary>
    )
  }
}
