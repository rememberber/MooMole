import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { moleHistory } from './fixtures/mole-history'

test('Mole history task reconnects on reload and recovers its result without events', async ({
  page,
}) => {
  await installTaskMock(page, 'history', moleHistory)
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await page.getByRole('button', { name: '读取 Mole 历史', exact: true }).click()
  await expect(page.getByText('Mole 历史读取进行中', { exact: true })).toBeVisible()
  await page.reload()
  await expect(page.getByText('Mole 历史读取进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  const history = page.getByRole('region', { name: 'Mole CLI 原始历史' })
  await expect(history.getByText('显示 20 / 25 个会话')).toBeVisible()
  await expect(history.getByText('显示 20 / 25 条删除日志')).toBeVisible()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]')),
  ).toHaveLength(2)
  await expect(page.getByText('客户端检查任务', { exact: true })).toBeVisible()
  await expect(history.getByRole('button', { name: /恢复|执行清理/ })).toHaveCount(0)
})

test('Mole history keeps attribution and raw statuses and filters both lists independently', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/mole-history.tsx')).mount())
  const fixture = page.locator('#mole-history-test-fixture')
  await fixture.getByText('清理 · 2026-10-16 10:00:00', { exact: true }).click()
  await expect(fixture.getByText('结束时间：未记录结束，无法判断是否仍在运行')).toBeVisible()
  await fixture.getByText('清理 · 2026-10-16 10:00:02', { exact: true }).click()
  await expect(fixture.getByText('上游标记为归属不明确', { exact: false })).toBeVisible()
  await fixture.getByText('清理 · 2026-10-16 10:00:03', { exact: true }).click()
  await expect(fixture.getByText('缺少 run_id，仅保留命令级汇总', { exact: false })).toBeVisible()
  await fixture.getByRole('button', { name: '再显示 20 个会话' }).click()
  await expect(fixture.getByText('显示 25 / 25 个会话')).toBeVisible()
  await fixture.getByRole('combobox', { name: 'Mole 历史命令' }).selectOption('optimize')
  await expect(fixture.getByText('显示 1 / 1 个会话')).toBeVisible()
  await expect(fixture.getByText('显示 20 / 25 条删除日志')).toBeVisible()
  await fixture.getByRole('combobox', { name: 'Mole 删除日志状态' }).selectOption('future-status')
  await expect(fixture.getByText('显示 1 / 1 条删除日志')).toBeVisible()
  await fixture.locator('summary').getByText('/fixture/cache-1', { exact: true }).click()
  await expect(fixture.getByText('原始方式：future-mode', { exact: false })).toBeVisible()
  await expect(
    fixture.locator('summary').getByText('原始状态：future-status', { exact: true }),
  ).toBeVisible()
  await fixture.getByRole('combobox', { name: 'Mole 删除日志状态' }).selectOption('all')
  await fixture.getByRole('textbox', { name: '搜索 Mole 历史' }).fill('中文')
  await expect(fixture.getByText('显示 0 / 0 个会话')).toBeVisible()
  await fixture
    .locator('summary')
    .getByText('/fixture/中文\\n[*] $(literal)', { exact: true })
    .click()
  await expect(fixture.getByText('大小未知', { exact: false })).toBeVisible()
  await expect(fixture.locator('summary').getByText('仅预览', { exact: true })).toBeVisible()
  await page.screenshot({ path: 'test-results/mole-history.png', fullPage: true })
})

test('Mole history distinguishes missing empty and partially unreadable logs', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/mole-history.tsx')).mount())
  const fixture = page.locator('#mole-history-test-fixture')
  await fixture.getByRole('button', { name: '模拟部分受限' }).click()
  await expect(fixture.getByRole('alert')).toContainText('部分历史无法读取')
  await expect(fixture.getByText('显示 20 / 25 条删除日志')).toBeVisible()
  await fixture.getByText('日志来源与读取范围', { exact: true }).click()
  await expect(fixture.getByText('fixture 权限拒绝')).toBeVisible()
  await fixture.getByRole('button', { name: '模拟日志缺失' }).click()
  await expect(fixture.getByText('尚未找到 Mole 操作日志。')).toBeVisible()
  await expect(fixture.getByText('尚未找到 Mole 删除日志。')).toBeVisible()
  await fixture.getByRole('button', { name: '模拟空日志' }).click()
  await expect(fixture.getByText('当前日志中没有可识别的会话。')).toBeVisible()
  await expect(fixture.getByText('当前日志中没有可识别的删除记录。')).toBeVisible()
})

test('failed or cancelled Mole history reads can be retried without inventing results', async ({
  page,
}) => {
  await installTaskMock(page, 'history', moleHistory)
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  const history = page.getByRole('region', { name: 'Mole CLI 原始历史' })
  await history.getByRole('button', { name: '读取 Mole 历史', exact: true }).click()
  await page.evaluate(() => {
    sessionStorage.setItem('mock-history-error', 'fixture JSON 无法识别')
    sessionStorage.setItem('mock-done', 'yes')
  })
  await expect(page.getByText('fixture JSON 无法识别', { exact: true }).first()).toBeVisible()
  await expect(history.getByRole('heading', { name: 'CLI 会话' })).toHaveCount(0)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await history.getByRole('button', { name: '读取 Mole 历史', exact: true }).click()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(page.getByText('Mole 历史读取进行中', { exact: true })).toHaveCount(0)
  await expect(history.getByRole('heading', { name: 'CLI 会话' })).toHaveCount(0)
  await history.getByRole('button', { name: '读取 Mole 历史', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(history.getByRole('heading', { name: 'CLI 会话' })).toBeVisible()
})
