import type { Snapshot } from '../types'
import { statusRate } from '../lib/status-metrics'

export function StatusRates({ snapshot, stale }: { snapshot: Snapshot; stale: boolean }) {
  const disk = snapshot.disk_io
  const network = snapshot.network
  return (
    <section className="card status-rates" aria-label="磁盘与网络速率">
      <div className="status-rate-group">
        <h2>
          磁盘读写 <span className="muted">{stale ? '最后报告值' : 'Mole 报告值'}</span>
        </h2>
        <dl className="status-rate-pair">
          <div>
            <dt>读取</dt>
            <dd>{statusRate(disk?.read_rate)}</dd>
          </div>
          <div>
            <dt>写入</dt>
            <dd>{statusRate(disk?.write_rate)}</dd>
          </div>
        </dl>
        <p className="muted">报告设备的合计速率，并非剩余空间或磁盘性能评分。</p>
        {(disk?.read_rate === 0 || disk?.write_rate === 0) && (
          <p className="muted">上游零值也可能来自初始化或采集失败，不能据此确认磁盘空闲。</p>
        )}
      </div>
      <div className="status-rate-group">
        <h2>
          网络接口 <span className="muted">{stale ? '最后报告值' : 'Mole 报告值'}</span>
        </h2>
        {network?.length ? (
          <table aria-label="网络接口速率">
            <thead>
              <tr>
                <th scope="col">接口</th>
                <th scope="col">接收</th>
                <th scope="col">发送</th>
              </tr>
            </thead>
            <tbody>
              {network.map((item, index) => (
                <tr key={`${item.name}:${index}`}>
                  <th scope="row">{item.name}</th>
                  <td>{statusRate(item.rx_rate_mbs)}</td>
                  <td>{statusRate(item.tx_rate_mbs)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="muted">
            {network == null
              ? '本次未提供网络接口数据。'
              : '本次没有返回网络接口，不能据此确认没有流量。'}
          </p>
        )}
        <p className="muted">最多 3 个接口；隧道与承载接口可能重复计量，分别显示，不相加。</p>
      </div>
      <details className="status-metric-scope">
        <summary>速率与趋势说明</summary>
        <p>速率按内置 Mole 的 1024² 字节换算为 MiB/s。沿用上游样本，不进行测速，也不推算总流量。</p>
        <p>
          趋势纵轴固定 0–100%，CPU 明确标为估算的样本不连入曲线。只保留本轮最多 5 分钟、120
          份已接收记录；未收到的中间记录不补点。缺失、跳号或超过 10
          秒的间隔断开显示，时钟回退重新开始。
        </p>
        <p>
          暂停保留最后结果；重新开始创建新一轮趋势，单次刷新不与旧趋势拼接。采样时间与接收时间不同，数据不保证持续有效；上游未逐指标报告采集成功状态。
        </p>
      </details>
    </section>
  )
}
