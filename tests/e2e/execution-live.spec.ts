import { expect, test } from '@playwright/test'
import { installLiveExecutionMock } from './fixtures/execution-live'

async function open(page: import('@playwright/test').Page) {
  await page
    .getByRole('region', { name: '日志处理状态', exact: true })
    .getByRole('button', { name: '查看执行记录' })
    .click()
  await page
    .getByRole('region', { name: '执行记录', exact: true })
    .getByRole('button')
    .filter({ hasText: '进行中' })
    .click()
  return page.getByRole('dialog', { name: '逐项执行结果' })
}

test('reload follows existing execution and cancellation waits for durable result without replay', async ({
  page,
}) => {
  await page.clock.install()
  await page.setViewportSize({ width: 960, height: 800 })
  await installLiveExecutionMock(page)
  await page.goto('/')
  await expect(page.getByRole('region', { name: '日志处理状态', exact: true })).toBeVisible()
  await page.reload()
  const dialog = await open(page)
  await expect(dialog.getByText('正在跟踪本批次，逐项结果自动更新。')).toBeVisible()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/execution-live-${theme}.png`,
      animations: 'disabled',
    })
  }
  await dialog.getByRole('button', { name: '停止后续项目' }).click()
  await expect(dialog.getByRole('button', { name: '等待实际结果…' })).toBeDisabled()
  await expect(dialog).toContainText('进行中 · 已移入 1 / 2 项')
  await page.evaluate(() => sessionStorage.setItem('live-finished', 'yes'))
  await page.clock.fastForward(1600)
  await expect(dialog).toContainText('已取消 · 已移入 1 / 2 项')
  await expect(dialog.getByRole('button', { name: '停止后续项目' })).toHaveCount(0)
  await expect(page.getByRole('region', { name: '日志处理状态', exact: true })).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('live-calls') || '[]'))
  expect(calls.filter((c: { command: string }) => c.command === 'cancel_log_cleanup')).toHaveLength(
    1,
  )
  expect(
    calls.every((c: { command: string }) =>
      ['bootstrap', 'list_executions', 'get_execution', 'cancel_log_cleanup'].includes(c.command),
    ),
  ).toBe(true)
})

test('failed reads preserve last known items and filters then update after recovery', async ({
  page,
}) => {
  await page.clock.install()
  await installLiveExecutionMock(page)
  await page.goto('/')
  const dialog = await open(page)
  await dialog.getByRole('textbox', { name: '搜索执行路径' }).fill('中文')
  await dialog.getByRole('combobox', { name: '执行项目状态' }).selectOption('moved')
  await page.evaluate(() => {
    sessionStorage.setItem('live-detail-error', 'yes')
    sessionStorage.setItem('live-list-error', 'yes')
  })
  await page.clock.fastForward(1600)
  await expect(dialog.getByRole('alert')).toContainText('保留上次记录')
  await expect(dialog.getByRole('textbox', { name: '搜索执行路径' })).toHaveValue('中文')
  await expect(dialog.locator('.execution-item')).toHaveCount(1)
  await page.evaluate(() => {
    sessionStorage.removeItem('live-detail-error')
    sessionStorage.removeItem('live-list-error')
    sessionStorage.setItem('live-finished', 'yes')
  })
  await page.clock.fastForward(3100)
  await expect(dialog.getByRole('alert')).toHaveCount(0)
  await expect(dialog).toContainText('已取消 · 已移入 1 / 2 项')
  await expect(dialog.getByRole('combobox', { name: '执行项目状态' })).toHaveValue('moved')
})

test('failed cancellation remains a request error and never reports a terminal result', async ({
  page,
}) => {
  await page.clock.install()
  await installLiveExecutionMock(page)
  await page.goto('/')
  const dialog = await open(page)
  await page.evaluate(() => sessionStorage.setItem('live-cancel-error', 'yes'))
  await dialog.getByRole('button', { name: '停止后续项目' }).click()
  await expect(dialog.getByRole('alert')).toContainText('取消请求失败')
  await expect(dialog).toContainText('进行中 · 已移入 1 / 2 项')
  await expect(dialog.getByRole('button', { name: '停止后续项目' })).toBeEnabled()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('region', { name: '日志处理状态', exact: true })).toBeVisible()
})

test('receipt location sends only persisted IDs and preserves results on a missing location', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 800 })
  await installLiveExecutionMock(page)
  await page.addInitScript(() => sessionStorage.setItem('live-finished', 'yes'))
  await page.goto('/')
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await page
    .getByRole('region', { name: '执行记录', exact: true })
    .getByRole('button')
    .filter({ hasText: '已取消' })
    .click()
  const dialog = page.getByRole('dialog', { name: '逐项执行结果' })
  await dialog.locator('.execution-item').first().locator('summary').click()
  await expect(dialog.getByRole('button', { name: '在 Finder 中查看' })).toHaveCount(1)
  await page.evaluate(() => sessionStorage.setItem('live-reveal-error', 'yes'))
  await dialog.getByRole('button', { name: '在 Finder 中查看' }).click()
  await expect(dialog.getByRole('alert')).toContainText('原记录保持不变')
  await expect(dialog).toContainText('已取消 · 已移入 1 / 2 项')
  await page.evaluate(() => sessionStorage.removeItem('live-reveal-error'))
  await dialog.getByRole('button', { name: '在 Finder 中查看' }).click()
  await expect(
    dialog.getByText(
      '已请求 Finder 显示。请核对文件后使用系统提供的操作；记录不会自动标为已恢复。',
    ),
  ).toBeVisible()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('live-calls') || '[]'))
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/execution-location-${theme}.png`,
      animations: 'disabled',
    })
  }
  const reveals = calls.filter(
    (call: { command: string }) => call.command === 'reveal_execution_item',
  )
  expect(reveals).toHaveLength(2)
  expect(Object.keys(reveals[0]).sort()).toEqual(['command', 'itemId', 'runId'])
  await expect(dialog.getByRole('button', { name: /恢复文件|自动恢复/ })).toHaveCount(0)
})
