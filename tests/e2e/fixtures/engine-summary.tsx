import { useState } from 'react'
import { createRoot } from 'react-dom/client'
import { EngineSummary } from '../../../src/components/EngineSummary'
import { demoBootstrap } from '../../../src/lib/demo'

function Fixture() {
  const [available, setAvailable] = useState(true)
  return (
    <>
      <EngineSummary engine={{ ...demoBootstrap.engine, available, version: '1.65.0' }} desktop />
      <button onClick={() => setAvailable(false)}>模拟引擎缺失</button>
    </>
  )
}

export function mount() {
  const host = document.createElement('div')
  host.id = 'engine-test-fixture'
  host.style.cssText =
    'position:fixed;inset:0;z-index:100;background:var(--background);padding:30px'
  document.body.append(host)
  createRoot(host).render(<Fixture />)
}
