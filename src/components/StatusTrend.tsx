import { statusTrend, type StatusSample } from '../lib/status-metrics'

function time(value: number) {
  return new Date(value).toLocaleTimeString('zh-CN', { hour12: false })
}
export function StatusTrend({
  samples,
  metric,
}: {
  samples: StatusSample[]
  metric: 'cpu' | 'memory'
}) {
  const segments = statusTrend(samples, metric)
  const label = metric === 'cpu' ? 'CPU 使用率' : '内存占用比例'
  const known = samples.filter((sample) => sample[metric] != null).length
  return (
    <div className="status-trend">
      {samples.length > 0 && (
        <span className="status-trend-scale">
          {metric === 'memory' ? '占用比例' : '报告使用率'} · 0–100%
        </span>
      )}
      {segments.length ? (
        <>
          <svg
            viewBox="0 0 320 64"
            preserveAspectRatio="none"
            role="img"
            aria-label={`${label}趋势，纵轴 0 至 100%，${known} 个有效记录，缺失、CPU 估算或间隔过长时断开显示`}
          >
            <path className="status-trend-guide" d="M0 32H320 M0 62H320" />
            {segments.map((segment, index) =>
              segment.length === 1 ? (
                <circle key={index} cx={segment[0].x} cy={segment[0].y} r="2" />
              ) : (
                <path
                  key={index}
                  vectorEffect="non-scaling-stroke"
                  d={segment
                    .map(
                      (point, i) => `${i ? 'L' : 'M'}${point.x.toFixed(2)} ${point.y.toFixed(2)}`,
                    )
                    .join(' ')}
                />
              ),
            )}
          </svg>
          <div className="status-trend-axis">
            <span>{time(samples[0].at)}</span>
            <span>{time(samples.at(-1)!.at)}</span>
          </div>
        </>
      ) : (
        <p className="muted">
          {samples.length ? '本轮尚无有效趋势数据' : '开始持续采集后显示趋势'}
        </p>
      )}
    </div>
  )
}
