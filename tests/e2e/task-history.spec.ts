import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { records } from './fixtures/task-history'

const history = (page: Page) => page.getByRole('region', { name: '客户端检查任务', exact: true })
async function mount(page: Page, many = false) {
  await page.goto('/')
  await page.evaluate(
    async (many) => (await import('/tests/e2e/fixtures/task-history.tsx')).mount(many),
    many,
  )
}
async function update(page: Page, mode: 'changed' | 'missing') {
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/task-history.tsx')).update(mode),
    mode,
  )
}

test('task history filters records and exposes complete diagnostics with keyboard and unknown-state boundaries', async ({
  page,
}) => {
  await mount(page)
  const list = history(page)
  await expect(list.getByRole('article')).toHaveCount(5)
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('权限拒绝')
  await expect(list.getByRole('article')).toHaveCount(1)
  await list.getByRole('combobox', { name: '检查任务状态' }).selectOption('failed')
  const opener = list.getByRole('article').getByRole('button')
  await opener.focus()
  await page.keyboard.press('Enter')
  const detail = page.getByRole('dialog', { name: '检查任务详情' })
  await expect(detail.getByText(records[0].path!, { exact: true })).toBeVisible()
  await expect(detail.getByText(records[0].message!, { exact: true })).toBeVisible()
  await expect(detail).toContainText('1.25 秒')
  await expect(detail).toContainText('不是清理或恢复凭证')
  await expect(detail.getByRole('button', { name: '继续跟踪' })).toHaveCount(0)
  await detail.getByText('记录信息', { exact: true }).click()
  await expect(detail).toContainText('任务 ID：failed-fixture')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    expect(await detail.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/task-history-detail-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.keyboard.press('Escape')
  await expect(detail).toHaveCount(0)
  await expect(opener).toBeFocused()
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('[*] $(literal)')
  await expect(list.getByRole('article')).toHaveCount(1)
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('status-fixture')
  await expect(list).toContainText('没有匹配的检查任务')
  await list.getByRole('button', { name: '清除筛选' }).click()
  await list.getByRole('combobox', { name: '检查任务状态' }).selectOption('unknown')
  await expect(list.getByRole('article')).toHaveCount(1)
  await expect(list).toContainText('未知检查（future-check）')
  await expect(list).toContainText('未知状态（future-phase）')
  await expect(list.getByRole('button', { name: '继续跟踪' })).toHaveCount(0)
  await list.getByRole('article').getByRole('button').click()
  await expect(detail).toContainText('未知')
  await expect(detail).not.toContainText('Invalid Date')
})

test('history refresh preserves filters on failure, updates open details, and never preserves removed records as evidence', async ({
  page,
}) => {
  await mount(page)
  const list = history(page)
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('/fixture/current')
  await list.getByRole('combobox', { name: '检查任务状态' }).selectOption('running')
  await page.getByRole('button', { name: '切换刷新失败' }).click()
  await list.getByRole('button', { name: '刷新检查任务' }).click()
  await expect(list.getByRole('alert')).toContainText('fixture history unavailable')
  await expect(list.getByRole('article')).toHaveCount(1)
  await list.getByRole('article').getByRole('button').first().click()
  const detail = page.getByRole('dialog', { name: '检查任务详情' })
  await expect(detail).toContainText('尚未记录结束')
  await update(page, 'changed')
  await expect(detail.locator('.task-detail-state')).toHaveText('未完成')
  await expect(detail).toContainText('fixture 目录读取失败')
  await expect(detail.getByRole('button', { name: '继续跟踪' })).toHaveCount(0)
  await update(page, 'missing')
  await expect(detail).toContainText('此任务已不在当前读取列表中')
  await expect(detail).not.toContainText('fixture 目录读取失败')
  await page.keyboard.press('Escape')
  await expect(list.getByRole('textbox', { name: '搜索检查任务' })).toBeFocused()
  await page.getByRole('button', { name: '切换刷新失败' }).click()
  await list.getByRole('button', { name: '刷新检查任务' }).click()
  await expect(list.getByRole('alert')).toHaveCount(0)
  await expect(list.getByRole('textbox', { name: '搜索检查任务' })).toHaveValue('/fixture/current')
  await expect(list.getByRole('combobox', { name: '检查任务状态' })).toHaveValue('running')
  await expect(list).toContainText('尚无客户端检查任务')
})

test('history limits mounted rows to batches and resetting search does not reveal all records', async ({
  page,
}) => {
  await mount(page, true)
  const list = history(page)
  await expect(list.getByRole('article')).toHaveCount(50)
  await list.getByRole('button', { name: '再显示 50 条' }).click()
  await expect(list.getByRole('article')).toHaveCount(100)
  await list.getByRole('button', { name: '再显示 50 条' }).click()
  await expect(list.getByRole('article')).toHaveCount(115)
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('bulk-114')
  await expect(list.getByRole('article')).toHaveCount(1)
  await list.getByRole('textbox', { name: '搜索检查任务' }).fill('')
  await expect(list.getByRole('article')).toHaveCount(50)
  await list.getByText('记录范围与保留说明', { exact: true }).click()
  await expect(list).toContainText('不代表所有历史')
})

test('App history reflects accepted and completed task states in an open detail without new task requests', async ({
  page,
}) => {
  await installTaskMock(page, 'new')
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  const list = history(page)
  await list.getByRole('article').getByRole('button').first().click()
  const detail = page.getByRole('dialog', { name: '检查任务详情' })
  await expect(detail.locator('.task-detail-state')).toHaveText('进行中')
  await expect(detail.getByRole('button', { name: '继续跟踪' })).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(detail.locator('.task-detail-state')).toHaveText('已完成')
  await expect(detail.getByRole('button', { name: '继续跟踪' })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await list.getByRole('button', { name: '刷新检查任务' }).click()
  await expect(list.getByRole('article')).toHaveCount(1)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').length),
  ).toBe(1)
})
