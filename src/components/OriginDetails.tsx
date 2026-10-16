import type { Provenance } from '../types'

export function OriginDetails({ value }: { value?: Provenance | null }) {
  if (!value || value.version !== 1 || !value.observations.length)
    return (
      <p className="origin-missing">
        这份候选没有可用的扫描调用来源，请重新扫描；位置与分类不能替代来源证据。
      </p>
    )
  return (
    <details className="origin-details">
      <summary>扫描调用来源 · {value.observations.length} 种</summary>
      <p>来自固定引擎记录候选时的观察；同一对象可能涉及多个模块，不代表已满足全部处理条件。</p>
      <ol>
        {value.observations.map((origin, index) => (
          <li key={index}>
            <p className="mono">{origin.functions.join(' → ')}</p>
            <p>
              逐项 guard：<code>{origin.deleteGuard || '未记录回调'}</code>
              <br />
              最终 guard：<code>{origin.finalGuard || '未记录回调'}</code>
            </p>
            <p className="mono">记录路径：{origin.path}</p>
          </li>
        ))}
      </ol>
    </details>
  )
}
