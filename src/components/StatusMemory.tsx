import type { Snapshot } from '../types'
import { bytes } from '../lib/format'
import { statusMemory } from '../lib/status-memory'
export function StatusMemory({ memory, stale }: { memory: Snapshot['memory']; stale: boolean }) {
  const info = statusMemory(memory)
  const amount = (value: number | null) => (value === null ? '未知' : bytes(value))
  const reportedZero = info.swapUsed === 0 || info.swapTotal === 0
  return (
    <div className="status-memory" role="group" aria-label="内存压力与交换空间">
      <p className="status-memory-pressure" data-pressure={info.pressure.level}>
        <span>内存压力</span>
        <strong>{info.pressure.label}</strong>
        <span className="muted">{stale ? '最后报告值' : 'Mole 报告值'} · 更新时间未知</span>
      </p>
      {info.pressure.level === 'unknown' ? (
        <p className="muted">本次未提供可识别压力，不能用占用比例推断。</p>
      ) : (
        info.pressure.level !== 'normal' && (
          <p>可查看下方进程样本，保存工作后按需关闭暂时不用的应用。</p>
        )
      )}
      <details className="status-memory-details">
        <summary>内存指标说明</summary>
        <dl>
          <div>
            <dt>可用内存（报告值）</dt>
            <dd>{amount(info.available)}</dd>
          </div>
          <div>
            <dt>交换已用（报告值）</dt>
            <dd>{amount(info.swapUsed)}</dd>
          </div>
          <div>
            <dt>交换总量（报告值）</dt>
            <dd>{amount(info.swapTotal)}</dd>
          </div>
        </dl>
        {info.inconsistentSwap && <p>交换字段不一致，本次无法确认。</p>}
        {reportedZero && <p>交换字段的零值也可能来自采集失败，不能据此确认未使用交换空间。</p>}
        <p>
          内存占用比例与压力是不同指标。压力沿用 Mole 的 normal / warn / critical
          报告，可能复用较早结果，未提供独立更新时间；正常也不保证当前仍正常。
        </p>
        <p>
          可用内存不是磁盘空间或可清理量；交换空间是系统报告的磁盘交换用量，不代表当前交换速率或性能评分。缺失、异常或无法精确表示的字节数保持未知，不补算压力、不自动释放内存。
        </p>
      </details>
    </div>
  )
}
