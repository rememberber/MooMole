import { Terminal } from 'lucide-react'
import type { Bootstrap } from '../types'

export function EngineSummary({
  engine,
  desktop,
}: {
  engine: Bootstrap['engine']
  desktop: boolean
}) {
  const version = desktop && engine.available ? engine.version : null
  return (
    <section className="engine-summary card" aria-label="Mole CLI 版本">
      <div className="engine-mark">
        <Terminal size={21} />
      </div>
      <div className="engine-description">
        <div className="engine-version">
          <span>内置 Mole CLI</span>
          <strong>{version ? `v${version}` : '版本待确认'}</strong>
        </div>
        <p>
          {desktop ? '随 MooMole 一起提供' : '浏览器预览 · 未加载本机引擎'}
          {' · '}源码基线 <span className="mono">{engine.revision.slice(0, 12)}</span>
        </p>
      </div>
      <span className="tag">
        {!desktop ? '界面预览' : engine.available ? '引擎就绪' : '引擎未就绪'}
      </span>
    </section>
  )
}
