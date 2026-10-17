import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { demoSnapshot } from '../../src/lib/demo'
import type { Snapshot } from '../../src/types'

const snapshot: Snapshot = {
  ...demoSnapshot,
  process_collected_at: '2026-10-17T01:00:00Z',
  process_stale: true,
  top_processes: [
    { pid: 11, name: 'Worker', cpu: 50 },
    { pid: 10, name: 'Worker', cpu: 120, memory_bytes: 1024 },
    { pid: 22, name: '中文 空格 [*] $(literal)'.repeat(12), cpu: 0, memory_bytes: 0 },
    { pid: 20, name: 'zed', cpu: null, memory_bytes: 5120 },
    { pid: 2, name: 'Beta', cpu: -1, memory_bytes: null },
  ],
}
const library = (page: Page) => page.getByRole('region', { name: 'Mole 进程快照' })
const rows = (page: Page) => library(page).locator('tbody tr')
const refresh = (page: Page) => page.getByRole('button', { name: '刷新快照', exact: true })
async function complete(page: Page, value: Snapshot) {
  await page.evaluate((value) => {
    sessionStorage.setItem('mock-status-result', JSON.stringify(value))
    sessionStorage.setItem('mock-done', 'yes')
  }, value)
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
}
async function open(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await complete(page, snapshot)
  await page.getByRole('button', { name: '系统状态', exact: true }).click()
}
async function start(page: Page) {
  await page.evaluate(() => sessionStorage.removeItem('mock-done'))
  await refresh(page).click()
  await expect(library(page).getByRole('status')).toHaveText('正在刷新，当前仍是上次快照。')
}

test('process search and sorting operate on returned samples, preserve unknowns and show scope in both themes', async ({
  page,
}) => {
  await open(page)
  const scope = library(page)
  await expect(rows(page)).toHaveCount(5)
  await expect(rows(page).nth(0)).toContainText('120.0%')
  await expect(rows(page).nth(1).getByRole('cell').last()).toHaveText('未知')
  await expect(rows(page).nth(2)).toContainText('0 B')
  await expect(scope).toContainText('复用的较早样本')
  await expect(scope).toContainText('并非全部进程')
  await scope.getByRole('textbox', { name: '搜索进程' }).fill('WORKER')
  await expect(rows(page)).toHaveCount(2)
  await scope.getByRole('textbox', { name: '搜索进程' }).fill('11')
  await expect(rows(page)).toHaveCount(1)
  await expect(rows(page).getByRole('cell').first()).toHaveText('11')
  await scope.getByRole('textbox', { name: '搜索进程' }).fill('nothing')
  await expect(scope).toContainText('当前样本中没有匹配的进程')
  await scope.getByRole('button', { name: '清除搜索' }).click()
  const sort = scope.getByRole('combobox', { name: '进程排序' })
  await sort.focus()
  await sort.selectOption('memory')
  await expect(rows(page).nth(0)).toContainText('zed')
  await expect(rows(page).nth(2)).toContainText('0 B')
  await expect(rows(page).nth(3).getByRole('cell').last()).toHaveText('未知')
  await sort.selectOption('pid')
  await expect(rows(page).nth(0).getByRole('cell').first()).toHaveText('2')
  await sort.selectOption('name')
  const names = await rows(page).getByRole('rowheader').allTextContents()
  expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b, 'zh-CN')))
  await scope.getByText('采样范围与指标说明', { exact: true }).click()
  await expect(scope).toContainText('最多 5 个进程样本')
  await expect(scope).toContainText('不能据此判断全机内存排名')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/process-snapshot-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').length),
  ).toBe(1)
  await expect(scope.getByRole('button', { name: /结束|终止|删除/ })).toHaveCount(0)
})

test('pending, failed and cancelled refreshes retain old samples and preferences; success replaces the sample', async ({
  page,
}) => {
  await open(page)
  const scope = library(page)
  await scope.getByRole('textbox', { name: '搜索进程' }).fill('Worker')
  await scope.getByRole('combobox', { name: '进程排序' }).selectOption('memory')
  await start(page)
  await expect(rows(page)).toHaveCount(2)
  await page.evaluate(() => sessionStorage.setItem('mock-history-error', 'fixture 无法采集进程'))
  await complete(page, { ...snapshot, top_processes: [] })
  await expect(page.getByRole('alert')).toContainText('fixture 无法采集进程')
  await expect(rows(page)).toHaveCount(2)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await start(page)
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(refresh(page)).toBeEnabled()
  await expect(rows(page)).toHaveCount(2)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-cancelled'))
  await start(page)
  await complete(page, {
    ...snapshot,
    process_stale: false,
    top_processes: [{ pid: 11, name: 'Worker new', cpu: 25, memory_bytes: 8192 }],
  })
  await expect(rows(page)).toHaveCount(1)
  await expect(rows(page)).toContainText('Worker new')
  await expect(rows(page)).toContainText('8 KB')
  await expect(scope.getByRole('textbox', { name: '搜索进程' })).toHaveValue('Worker')
  await expect(scope.getByRole('combobox', { name: '进程排序' })).toHaveValue('memory')
  await expect(scope).toContainText('采样时为新样本')
  await expect(scope).not.toContainText('复用的较早样本')
})

test('missing, empty and unreported freshness stay distinct without substituting snapshot time', async ({
  page,
}) => {
  await open(page)
  await start(page)
  await complete(page, {
    ...snapshot,
    process_collected_at: null,
    process_stale: null,
    top_processes: null,
  })
  await expect(library(page)).toContainText('此快照未提供进程列表')
  await expect(library(page)).toContainText('进程样本数未知')
  await expect(library(page)).toContainText('样本新鲜度未知')
  await expect(library(page)).toContainText('样本时间：未知')
  await expect(library(page).locator('time')).toHaveCount(0)
  await start(page)
  await complete(page, { ...snapshot, top_processes: [] })
  await expect(library(page)).toContainText('Mole 没有返回进程记录')
  await expect(library(page)).toContainText('显示 0 / 0 个已返回样本')
})
