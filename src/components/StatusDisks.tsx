import type { Snapshot } from '../types'
import { percent } from '../lib/format'
import { diskBytes, diskReport } from '../lib/status-disks'
export function StatusDisks({ snapshot, stale }: { snapshot: Snapshot; stale: boolean }) {
  const disks = Array.isArray(snapshot.disks) ? snapshot.disks : null
  return (
    <section className="card status-disks" aria-label="Mole 磁盘空间">
      <header>
        <h2>磁盘空间</h2>
        <span className="muted">{stale ? '最后报告值' : 'Mole 报告值'} · 独立更新时间未知</span>
      </header>
      <p className="status-disk-summary muted">
        最多 3 条已筛选报告，不代表全部磁盘；不汇总共享空间。
      </p>
      {disks?.length ? (
        <table aria-label="磁盘容量报告">
          <thead>
            <tr>
              <th scope="col">挂载点</th>
              <th scope="col">报告已用</th>
              <th scope="col">报告总量</th>
              <th scope="col">余量（计算）</th>
              <th scope="col">已用比例</th>
            </tr>
          </thead>
          <tbody>
            {disks.map((raw, index) => {
              const disk = diskReport(raw)
              return (
                <tr key={index}>
                  <th scope="row">
                    <span>{disk.mount ?? '挂载点未知'}</span>
                    <small className="muted">
                      {disk.external === true
                        ? '外接线索（Mole）'
                        : disk.external === false
                          ? '未标为外接'
                          : '外接状态未知'}
                    </small>
                    {disk.inconsistent && (
                      <small className="muted">已用超过总量，本次无法计算余量。</small>
                    )}
                  </th>
                  <td>{diskBytes(disk.used)}</td>
                  <td>{diskBytes(disk.total)}</td>
                  <td>{diskBytes(disk.remaining)}</td>
                  <td>{disk.percent === null ? '未知' : percent(disk.percent)}</td>
                </tr>
              )
            })}
          </tbody>
        </table>
      ) : (
        <p className="empty-inline muted">
          {disks === null ? '本次未提供磁盘报告。' : '本次未返回磁盘记录，不代表没有已连接的磁盘。'}
        </p>
      )}
      <details className="status-metric-scope">
        <summary>磁盘空间口径</summary>
        <p>
          余量与比例只根据本次有效的报告总量、已用字节计算，不从占用百分比倒推大小；缺失、零总量、精度异常或字段不一致时保持未知。余量不是可清理量，也不保证当前可写入这些空间。
        </p>
        <p>
          固定 Mole 会筛掉部分系统/网络挂载、小于 1 GiB
          的卷，按设备和文件系统/容量线索去重后最多返回 3
          条；记录顺序不证明磁盘身份，也不代表完整连接列表。外接只是上游线索，未标为外接不证明是内置盘。
        </p>
        <p>
          APFS
          修正可能包含系统可清除空间，多个卷也可能共享容量池，不能把各行相加为整机总量。快速快照可沿用较早容量报告，未提供独立采样时间；此处不会访问挂载点或启动目录扫描。
        </p>
      </details>
    </section>
  )
}
