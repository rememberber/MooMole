import { getLocale } from '../i18n/locale'
import type { Snapshot } from '../types'
import { percent } from '../lib/format'
import { statusPercent } from '../lib/status-metrics'
import {
  batteryCapacity,
  batteryState,
  batteryTemperature,
  gpuName,
  positiveCount,
} from '../lib/status-hardware'
export function StatusHardware({ snapshot, stale }: { snapshot: Snapshot; stale: boolean }) {
  const gpus = snapshot.gpu?.filter((gpu) => gpuName(gpu.name) !== null)
  const temperature = batteryTemperature(snapshot.thermal?.battery_temp)
  return (
    <section className="card status-hardware" aria-label="硬件报告">
      <header>
        <h2>硬件报告</h2>
        <span className="muted">{stale ? '最后报告值' : 'Mole 报告值'} · 独立更新时间未知</span>
      </header>
      <div className="status-hardware-grid">
        <section aria-label="电池报告">
          <h3>电池</h3>
          {snapshot.batteries?.length ? (
            snapshot.batteries.map((battery, index) => {
              const charge = statusPercent(battery.percent)
              const capacity = batteryCapacity(battery.capacity)
              const cycles = positiveCount(battery.cycle_count)
              return (
                <div
                  className="status-hardware-device"
                  key={index}
                  role="group"
                  aria-label={`电池样本 ${index + 1}`}
                >
                  <p>
                    <strong>{charge === null ? '电量未知' : percent(charge)}</strong> ·{' '}
                    {batteryState(battery.status)}
                  </p>
                  <details>
                    <summary>
                      电池报告详情{snapshot.batteries!.length > 1 ? ` · 样本 ${index + 1}` : ''}
                    </summary>
                    <dl>
                      <div>
                        <dt>最大容量（可能为估算）</dt>
                        <dd>{capacity === null ? '未知' : `${capacity}%`}</dd>
                      </div>
                      <div>
                        <dt>循环次数</dt>
                        <dd>{cycles === null ? '未知' : cycles}</dd>
                      </div>
                    </dl>
                    <p className="muted">
                      容量可能来自上游估算，未提供来源区分；循环次数的零值也可能表示未取得数据，不显示为已确认的零次。
                    </p>
                  </details>
                </div>
              )
            })
          ) : (
            <p className="muted">
              {snapshot.batteries == null
                ? '本次未提供电池数据。'
                : '本次未返回电池记录，不能据此判断设备没有电池。'}
            </p>
          )}
        </section>
        <section aria-label="GPU 报告">
          <h3>GPU</h3>
          {gpus?.length ? (
            gpus.map((gpu, index) => {
              const usage = statusPercent(gpu.usage)
              const cores = positiveCount(gpu.core_count)
              return (
                <div className="status-hardware-device" key={index}>
                  <strong>{gpuName(gpu.name)}</strong>
                  <p>
                    使用率 {usage === null ? '未知' : percent(usage)}
                    {cores !== null && ` · ${cores} 个核心`}
                  </p>
                </div>
              )
            })
          ) : (
            <p className="muted">
              {snapshot.gpu == null ? '本次未提供 GPU 数据。' : '本次未提供可识别的 GPU 记录。'}
            </p>
          )}
          <p className="muted">报告零值不保证当前空闲；不可用时保持未知。</p>
        </section>
        <section aria-label="温度报告">
          <h3>温度</h3>
          <p>
            电池温度{' '}
            <strong>
              {temperature === null
                ? '未知'
                : temperature < 0.1
                  ? '< 0.1 °C'
                  : `${temperature.toLocaleString(getLocale(), { maximumFractionDigits: 1 })} °C`}
            </strong>
          </p>
          <p className="muted">当前内置 Mole 未提供 CPU/GPU 温度，不以电池温度代替。</p>
          {temperature === null && <p className="muted">零值、缺失或异常温度不作为测量结果。</p>}
        </section>
      </div>
      <details className="status-metric-scope">
        <summary>硬件指标说明</summary>
        <p>
          只展示内置 Mole
          的结构化报告，不另启查询、提权或安装工具。硬件数据可能来自较早的缓存，未提供独立更新时间；持续采集也不保证每项都重新采样。
        </p>
        <p>
          最多接收 8 条 GPU、4 条电池记录。电池样本按本次顺序显示，没有稳定设备
          ID；空记录不证明硬件不存在。未知使用率不会补成零，不据此判断设备健康或给出性能评分。
        </p>
        <p>
          最大容量可能为估算，GPU
          使用率的负值表示未取得有效报告；不自动授权、控制风扇、关闭应用或触发额外采样。
        </p>
      </details>
    </section>
  )
}
