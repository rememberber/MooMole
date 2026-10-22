import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { demoSnapshot } from '../../src/lib/demo'

async function open(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.addInitScript((snapshot) => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = async (command, args = {}) => {
      if (command === 'bootstrap' || command === 'get_history') {
        const result = await original(command, args)
        const stream =
          sessionStorage.getItem('watch-state') === 'running'
            ? [
                {
                  id: sessionStorage.getItem('watch-id'),
                  kind: 'statuswatch',
                  state: 'running',
                  path: null,
                  createdAt: Date.now(),
                  finishedAt: null,
                  message: null,
                },
              ]
            : []
        if (command === 'bootstrap') {
          const initial = result as { history: unknown[] }
          return { ...initial, history: [...stream, ...initial.history] }
        }
        return [...stream, ...(result as unknown[])]
      }
      if (command === 'start_status_watch') {
        const ids = JSON.parse(sessionStorage.getItem('watch-starts') ?? '[]')
        ids.push(args.sessionId)
        sessionStorage.setItem('watch-starts', JSON.stringify(ids))
        sessionStorage.setItem('watch-id', String(args.sessionId))
        sessionStorage.setItem('watch-interval', String(args.intervalSeconds))
        const admissions = JSON.parse(sessionStorage.getItem('watch-admissions') ?? '[]')
        admissions.push(args)
        sessionStorage.setItem('watch-admissions', JSON.stringify(admissions))
        sessionStorage.setItem('watch-sequence', '1')
        sessionStorage.setItem('watch-state', 'running')
        return
      }
      if (command === 'stop_status_watch') {
        const ids = JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]')
        ids.push(args.sessionId)
        sessionStorage.setItem('watch-stops', JSON.stringify(ids))
        if (args.sessionId === sessionStorage.getItem('watch-id'))
          sessionStorage.setItem('watch-state', 'stopped')
        return
      }
      if (command === 'get_status_watch') {
        if (sessionStorage.getItem('watch-fail')) throw new Error('fixture 采集已中断')
        const sequence = Number(sessionStorage.getItem('watch-sequence'))
        return {
          version: 2,
          id: args.sessionId,
          state: sessionStorage.getItem('watch-state'),
          intervalSeconds: Number(sessionStorage.getItem('watch-interval')),
          sequence,
          receivedAt: Date.now(),
          error: null,
          data: {
            ...snapshot,
            collected_at: new Date().toISOString(),
            top_processes: [
              { pid: 123, name: `warm-${sequence}`, cpu: sequence, memory_bytes: 1024 },
            ],
            ...JSON.parse(sessionStorage.getItem('watch-metrics') ?? '{}'),
          },
        }
      }
      return original(command, args)
    }
  }, demoSnapshot)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '系统状态', exact: true }).click()
}
const start = (page: Page) => page.getByRole('button', { name: '持续采集', exact: true })
const process = (page: Page) => page.getByRole('region', { name: 'Mole 进程快照' })
test('single continuous session updates samples and keeps preferences; pause preserves last result', async ({
  page,
}) => {
  await open(page)
  await start(page).click()
  await expect(process(page)).toContainText('warm-1')
  await expect(page.getByRole('button', { name: '刷新快照', exact: true })).toBeDisabled()
  await process(page).getByRole('textbox', { name: '搜索进程' }).fill('warm')
  await process(page).getByRole('combobox', { name: '进程排序' }).selectOption('memory')
  await page.evaluate(() => sessionStorage.setItem('watch-sequence', '2'))
  await expect(process(page)).toContainText('warm-2')
  await expect(process(page).getByRole('combobox', { name: '进程排序' })).toHaveValue('memory')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/status-watch-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(page.getByText('采集已暂停，当前显示最后快照。')).toBeVisible()
  await expect(process(page)).toContainText('warm-2')
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts') ?? '[]').length),
  ).toBe(1)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').length),
  ).toBe(1)
  expect(
    await page.evaluate(
      () =>
        JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]')[0] ===
        sessionStorage.getItem('watch-id'),
    ),
  ).toBe(true)
})

test('sampling cadence changes only when paused and is bound to a fresh session', async ({
  page,
}) => {
  await open(page)
  const cadence = page.getByRole('combobox', { name: '采样周期', exact: true })
  await expect(cadence).toHaveValue('2')
  await cadence.selectOption('1')
  await start(page).click()
  await expect(page.getByText('持续采集中 · 约每 1 秒更新', { exact: true })).toBeVisible()
  await expect(cadence).toBeDisabled()
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await expect(cadence).toBeEnabled()
  await cadence.selectOption('5')
  await start(page).click()
  await expect(page.getByText('持续采集中 · 约每 5 秒更新', { exact: true })).toBeVisible()
  await expect(page.getByText('本轮保留 1 份已接收记录', { exact: false })).toBeVisible()
  const requests = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('watch-admissions') ?? '[]'),
  )
  expect(requests.map((r: { intervalSeconds: number }) => r.intervalSeconds)).toEqual([1, 5])
  expect(requests[0].sessionId).not.toBe(requests[1].sessionId)
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]')),
  ).toContain(requests[0].sessionId)
  await expect(cadence).toHaveValue('5')
})

test('leaving status and hiding the document stop capture without automatic resumption', async ({
  page,
}) => {
  await open(page)
  await start(page).click()
  await expect(process(page)).toContainText('warm-1')
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await expect
    .poll(() =>
      page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]').length),
    )
    .toBe(1)
  await page.getByRole('button', { name: '系统状态', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await start(page).click()
  await expect(page.getByRole('button', { name: '暂停采集', exact: true })).toBeEnabled()
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { value: true, configurable: true })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  await expect
    .poll(() =>
      page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]').length),
    )
    .toBe(2)
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { value: false, configurable: true })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  await expect(start(page)).toBeEnabled()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts') ?? '[]').length),
  ).toBe(2)
})

test('transport failure retains last sample and does not retry or label it live', async ({
  page,
}) => {
  await open(page)
  await start(page).click()
  await expect(process(page)).toContainText('warm-1')
  await page.evaluate(() => sessionStorage.setItem('watch-fail', 'yes'))
  await expect(page.getByRole('alert')).toContainText('fixture 采集已中断')
  await expect(process(page)).toContainText('warm-1')
  await expect(start(page)).toBeEnabled()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts') ?? '[]').length),
  ).toBe(1)
})

test('reload stops the old session without restarting or following it as a snapshot task', async ({
  page,
}) => {
  await open(page)
  await start(page).click()
  await expect(process(page)).toContainText('warm-1')
  const oldId = await page.evaluate(() => sessionStorage.getItem('watch-id'))
  await page.reload()
  await page.getByRole('button', { name: '系统状态', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts') ?? '[]').length),
  ).toBe(1)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-stops') ?? '[]')),
  ).toContain(oldId)
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '暂停采集', exact: true })).toHaveCount(0)
  await expect(process(page)).not.toContainText('warm-') // an ordinary startup snapshot is allowed
})

test('trends and interface rates preserve units, zero and missing values, pause history and restart boundaries', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() =>
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        collected_at: '2026-10-21T00:00:00Z',
        cpu: { usage: 0, logical_cpu: 12 },
        disk_io: { read_rate: 0, write_rate: 1.25 },
        network: [
          { name: 'utun9', rx_rate_mbs: 2, tx_rate_mbs: 0 },
          { name: 'en0', rx_rate_mbs: 2.5, tx_rate_mbs: 1.5 },
        ],
      }),
    ),
  )
  await start(page).click()
  const rates = page.getByRole('region', { name: '磁盘与网络速率' })
  const cpu = page.getByRole('img', { name: /CPU 使用率趋势/ })
  await expect(cpu).toBeVisible()
  await expect(rates).toContainText('1.25 MiB/s')
  await expect(rates).toContainText('不能据此确认磁盘空闲')
  await expect(rates.getByRole('table', { name: '网络接口速率' }).locator('tbody tr')).toHaveCount(
    2,
  )
  await expect(rates.getByRole('row', { name: /utun9/ })).toContainText('0 MiB/s')
  await expect(rates.getByRole('row', { name: /en0/ })).toContainText('2.5 MiB/s')
  await page.evaluate(() => {
    const value = JSON.parse(sessionStorage.getItem('watch-metrics')!)
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        ...value,
        collected_at: '2026-10-21T00:00:02Z',
        cpu: { usage: 100, logical_cpu: 12 },
      }),
    )
    sessionStorage.setItem('watch-sequence', '2')
  })
  await expect(cpu).toHaveAttribute('aria-label', /2 个有效记录/)
  await expect(cpu.locator('path:not(.status-trend-guide)')).toHaveCount(1)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/status-metrics-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => {
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        collected_at: '2026-10-21T00:00:04Z',
        cpu: { usage: 0, per_core_estimated: true },
        memory: { used_percent: -1 },
        disk_io: { read_rate: null, write_rate: -1 },
        network: [],
      }),
    )
    sessionStorage.setItem('watch-sequence', '3')
  })
  await expect(rates.getByRole('definition')).toHaveText(['未知', '未知'])
  await expect(page.getByText('— 个逻辑核心 · 当前为估算值', { exact: true })).toBeVisible()
  await expect(rates).toContainText('没有返回网络接口，不能据此确认没有流量')
  await expect(page.getByText('本轮保留 3 份已接收记录', { exact: false })).toBeVisible()
  await expect(page.getByRole('img', { name: /内存占用比例趋势/ })).toHaveAttribute(
    'aria-label',
    /2 个有效记录/,
  )
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(cpu).toHaveAttribute('aria-label', /2 个有效记录/)
  await expect(rates.getByText('最后报告值', { exact: true })).toHaveCount(2)
  await start(page).click()
  await expect(page.getByText('本轮保留 1 份已接收记录', { exact: false })).toBeVisible()
  await expect(cpu).toHaveCount(0)
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await page.getByRole('button', { name: '刷新快照', exact: true }).click()
  await expect(page.getByText(/本轮保留 \d+ 份已接收记录/)).toHaveCount(0)
})

test('rate trends separate directions and interfaces, keep missing gaps and retain historical curves on pause', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() =>
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        collected_at: '2026-10-21T00:00:01Z',
        disk_io: { read_rate: 0, write_rate: 4 },
        network: [
          { name: 'en0', rx_rate_mbs: 2, tx_rate_mbs: 4 },
          { name: 'utun9', rx_rate_mbs: 100, tx_rate_mbs: 0 },
        ],
      }),
    ),
  )
  await start(page).click()
  const disk = page.getByRole('img', { name: /^磁盘读写趋势/ })
  const en0 = page.getByRole('img', { name: /^en0 网络趋势/ })
  const selector = page.getByRole('combobox', { name: '趋势网络接口', exact: true })
  await expect(disk).toHaveAttribute('aria-label', /纵轴 0 至 5 MiB\/s/)
  await expect(en0).toHaveAttribute('aria-label', /纵轴 0 至 5 MiB\/s/)
  await expect(selector).toHaveValue('en0')
  await selector.selectOption('utun9')
  await expect(page.getByRole('img', { name: /^utun9 网络趋势/ })).toHaveAttribute(
    'aria-label',
    /纵轴 0 至 100 MiB\/s/,
  )
  await selector.selectOption('en0')
  const advance = async (
    sequence: number,
    network: unknown[],
    disk_io = { read_rate: sequence, write_rate: sequence },
  ) => {
    await page.evaluate(
      ({ sequence, network, disk_io }) => {
        sessionStorage.setItem(
          'watch-metrics',
          JSON.stringify({ collected_at: `2026-10-21T00:00:0${sequence}Z`, network, disk_io }),
        )
        sessionStorage.setItem('watch-sequence', String(sequence))
      },
      { sequence, network, disk_io },
    )
    await expect(
      page.getByText(`本轮保留 ${sequence} 份已接收记录`, { exact: false }),
    ).toBeVisible()
  }
  await advance(2, [{ name: 'utun9', rx_rate_mbs: 100, tx_rate_mbs: 0 }])
  await expect(selector).toHaveValue('en0')
  await expect(page.getByText('本次未返回 en0；保留本轮已接收曲线，缺失处断开。')).toBeVisible()
  await expect(en0).toHaveAttribute('aria-label', /接收 1 个有效记录/)
  await advance(3, [{ name: 'en0', rx_rate_mbs: 4, tx_rate_mbs: null }])
  await advance(4, [{ name: 'en0', rx_rate_mbs: 6, tx_rate_mbs: 2 }])
  await expect(en0).toHaveAttribute('aria-label', /接收 3 个有效记录，发送 2 个有效记录/)
  await expect(en0.locator('g[data-series="接收"] circle')).toHaveCount(1)
  await expect(en0.locator('g[data-series="接收"] path')).toHaveCount(1)
  await expect(en0.locator('g[data-series="发送"] circle')).toHaveCount(2)
  await expect(en0.locator('g[data-series="发送"] path')).toHaveCount(0)
  await expect(disk).toHaveAttribute('aria-label', /读取 4 个有效记录，写入 4 个有效记录/)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/status-rate-trends-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(en0).toHaveAttribute('aria-label', /接收 3 个有效记录/)
  await expect(
    page.getByRole('region', { name: '磁盘与网络速率' }).getByText('最后报告值', { exact: true }),
  ).toHaveCount(2)
  await start(page).click()
  await expect(page.getByText('本轮保留 1 份已接收记录', { exact: false })).toBeVisible()
  await expect(en0).toHaveAttribute('aria-label', /接收 1 个有效记录，发送 1 个有效记录/)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts')!).length),
  ).toBe(2)
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
})

test('memory pressure stays separate from occupancy, preserves unknown freshness and exposes counters on demand', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() =>
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        memory: {
          total: 8192,
          used: 8192,
          used_percent: 100,
          pressure: 'normal',
          available: 1024,
          swap_used: 0,
          swap_total: 0,
        },
      }),
    ),
  )
  await start(page).click()
  const memory = page.getByRole('group', { name: '内存压力与交换空间', exact: true })
  await expect(memory).toContainText('正常')
  await expect(memory).toContainText('Mole 报告值 · 更新时间未知')
  await expect(memory.getByText('可用内存（报告值）', { exact: true })).not.toBeVisible()
  await memory.getByText('内存指标说明', { exact: true }).click()
  await expect(memory).toContainText('1 KB')
  await expect(memory.locator('dd')).toHaveText(['1 KB', '0 B', '0 B'])
  await expect(memory).toContainText('零值也可能来自采集失败')
  await expect(memory).toContainText('正常也不保证当前仍正常')
  await expect(memory).not.toContainText('可查看下方进程样本')
  for (const [sequence, pressure, label] of [
    [2, 'warn', '偏高'],
    [3, 'critical', '很高'],
  ] as const) {
    await page.evaluate(
      ({ sequence, pressure }) => {
        sessionStorage.setItem(
          'watch-metrics',
          JSON.stringify({ memory: { used_percent: 0, pressure } }),
        )
        sessionStorage.setItem('watch-sequence', String(sequence))
      },
      { sequence, pressure },
    )
    await expect(memory.locator('.status-memory-pressure strong')).toHaveText(label)
    await expect(memory).toContainText('保存工作后按需关闭暂时不用的应用')
    await expect(memory.locator('dd')).toHaveText(['未知', '未知', '未知'])
  }
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/status-memory-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
  await page.getByRole('button', { name: '暂停采集', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(memory).toContainText('很高')
  await expect(memory).toContainText('最后报告值 · 更新时间未知')
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('watch-starts')!).length),
  ).toBe(1)
})

test('missing, unsupported and contradictory memory fields cannot become healthy or zero claims', async ({
  page,
}) => {
  await open(page)
  await page.evaluate(() =>
    sessionStorage.setItem('watch-metrics', JSON.stringify({ memory: null })),
  )
  await start(page).click()
  const memory = page.getByRole('group', { name: '内存压力与交换空间', exact: true })
  await expect(memory.locator('.status-memory-pressure strong')).toHaveText('未知')
  await expect(memory).toContainText('不能用占用比例推断')
  await memory.getByText('内存指标说明', { exact: true }).click()
  await expect(memory.locator('dd')).toHaveText(['未知', '未知', '未知'])
  await expect(memory).not.toContainText('0 B')
  await page.evaluate(() => {
    sessionStorage.setItem(
      'watch-metrics',
      JSON.stringify({
        memory: {
          total: 1024,
          available: 2048,
          used_percent: 1,
          pressure: 'unexpected',
          swap_used: 2048,
          swap_total: 1024,
        },
      }),
    )
    sessionStorage.setItem('watch-sequence', '2')
  })
  await expect(memory).toContainText('交换字段不一致，本次无法确认')
  await expect(memory.locator('.status-memory-pressure strong')).toHaveText('未知')
  await expect(memory.locator('dd')).toHaveText(['未知', '未知', '未知'])
  await expect(memory).not.toContainText('unexpected')
  await page.evaluate(() => sessionStorage.setItem('watch-fail', 'yes'))
  await expect(page.getByRole('alert')).toContainText('fixture 采集已中断')
  await expect(start(page)).toBeEnabled()
  await expect(memory).toContainText('最后报告值 · 更新时间未知')
})
