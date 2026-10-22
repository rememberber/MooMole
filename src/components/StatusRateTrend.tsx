import { statusRate, statusRateTrend, type StatusSample } from '../lib/status-metrics'

export function StatusRateTrend({
  samples,
  metric,
  name = '',
}: {
  samples: StatusSample[]
  metric: 'disk' | 'network'
  name?: string
}) {
  const { upper, series } = statusRateTrend(samples, metric, name)
  const label = metric === 'disk' ? '磁盘读写' : `${name} 网络`
  const hasData = series.some((s) => s.count > 0)
  const time = (at: number) => new Date(at).toLocaleTimeString('zh-CN', { hour12: false })
  return (
    <div className="status-trend status-rate-trend">
      {hasData ? (
        <>
          <span className="status-trend-scale">
            速率 · 0–{statusRate(upper)} · 纵轴随本轮记录调整
          </span>
          <div className="status-rate-legend">
            {series.map((s, index) => (
              <span key={s.label}>
                <i className={`status-rate-line status-rate-line-${index}`} aria-hidden="true" />
                {s.label} · {s.count} 份
              </span>
            ))}
          </div>
          <svg
            viewBox="0 0 320 64"
            preserveAspectRatio="none"
            role="img"
            aria-label={`${label}趋势，纵轴 0 至 ${statusRate(upper)}，${series.map((s) => `${s.label} ${s.count} 个有效记录`).join('，')}，缺失、跳号或间隔过长时断开显示`}
          >
            <path className="status-trend-guide" d="M0 32H320 M0 62H320" />
            {series.map((s, index) => (
              <g
                key={s.label}
                className={`status-rate-series-${index}`}
                aria-hidden="true"
                data-series={s.label}
              >
                {s.segments.map((segment, i) =>
                  segment.length === 1 ? (
                    <circle
                      key={i}
                      cx={segment[0].x}
                      cy={segment[0].y}
                      r={index === 0 ? 2.5 : 1.5}
                    />
                  ) : (
                    <path
                      key={i}
                      vectorEffect="non-scaling-stroke"
                      d={segment
                        .map(
                          (point, j) =>
                            `${j ? 'L' : 'M'}${point.x.toFixed(2)} ${point.y.toFixed(2)}`,
                        )
                        .join(' ')}
                    />
                  ),
                )}
              </g>
            ))}
          </svg>
          <div className="status-trend-axis">
            <span>{time(samples[0].at)}</span>
            <span>{time(samples.at(-1)!.at)}</span>
          </div>
        </>
      ) : (
        <p className="muted">
          {samples.length ? '本轮尚无有效速率趋势数据' : '开始持续采集后显示趋势'}
        </p>
      )}
    </div>
  )
}
