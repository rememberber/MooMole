import { useEffect, useState } from 'react'
import { Modal } from './Modal'
import { maintenanceCatalog } from '../lib/maintenance'
import { maintenanceOutcomeText, type MaintenanceReview } from '../lib/maintenance-review'
import { dateTime } from '../lib/format'

export function MaintenanceReviewDialog({
  review,
  onClose,
  onRecheck,
}: {
  review: MaintenanceReview
  onClose: () => void
  onRecheck: () => void
}) {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [])
  const stale = now < review.checkedAt || now >= review.expiresAt
  return (
    <Modal title="维护条件审阅" onClose={onClose} className="maintenance-review">
      <h2>维护条件审阅</h2>
      <p className="muted">
        只审阅所选 {review.items.length} 项的条件与影响，不修改设置，也不生成可执行任务。
      </p>
      <p className="muted">检查时间 {dateTime(review.checkedAt)} · 检查结果保留 60 秒供审阅</p>
      <p role={stale ? 'alert' : 'status'}>
        {stale
          ? '检查已过期或时间变化，请重新检查；以下仅为较早的审阅结果。'
          : '原始键值与恢复证据尚未保存，并发变更仍可能被覆盖；执行暂未开放。'}
      </p>
      {review.items.map((item) => {
        const metadata = maintenanceCatalog.items.find((value) => value.id === item.id)!
        return (
          <section
            className="maintenance-review-item"
            key={item.id}
            aria-label={`${metadata.name}条件审阅`}
          >
            <h3>{metadata.name}</h3>
            <p className="muted">{metadata.impact}</p>
            <dl>
              {item.keys.map((key) => (
                <div key={key.key}>
                  <dt>{key.label}</dt>
                  <dd>
                    设置观察：
                    {key.current === 'enabled'
                      ? '已启用'
                      : key.current === 'disabled'
                        ? '未启用'
                        : '未知'}
                    <br />
                    维护意图：
                    {key.intent === 'enableSetting' ? '开启此设置' : '移除覆盖值，恢复系统默认行为'}
                    <br />
                    <strong>{maintenanceOutcomeText[key.outcome]}</strong>
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        )
      })}
      <div className="modal-actions">
        <button className="button secondary" onClick={onClose}>
          关闭
        </button>
        <button className="button secondary" onClick={onRecheck}>
          返回并重新检查
        </button>
      </div>
    </Modal>
  )
}
