import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

const notice = (page: Page) => page.getByRole('region', { name: '空间分析任务' })
const path = (page: Page) => page.getByRole('textbox', { name: '扫描目录', exact: true })
const startButton = (page: Page) => page.getByRole('button', { name: '开始分析', exact: true })
const result = (page: Page) => page.getByRole('region', { name: 'Mole 空间分析结果' })
async function open(page: Page) {
  await installTaskMock(page, 'navigation')
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
}
async function start(page: Page, target: string) {
  await path(page).fill(target)
  await startButton(page).click()
  await expect(notice(page)).toContainText('空间分析进行中')
}
async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(startButton(page)).toBeEnabled()
}
async function starts(page: Page) {
  return page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').filter(
      (v: { kind: string }) => v.kind === 'analyze',
    ),
  )
}

test('pending analysis binds the submitted path, keeps old results, and waits for confirmed cancellation', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/previous')
  await finish(page)
  const target = '/fixture/中文 空格/[*] $(literal)/' + 'long-directory-'.repeat(8)
  await start(page, target)
  await path(page).fill('/fixture/edited-input')
  await expect(notice(page)).toContainText(target)
  await expect(notice(page)).not.toContainText('/fixture/edited-input')
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/previous')
  const progress = notice(page).getByRole('progressbar')
  await expect(progress).not.toHaveAttribute('value')
  await expect(progress).not.toHaveAttribute('aria-valuenow')
  await expect(page.getByRole('button', { name: '取消', exact: true })).toHaveCount(1)
  await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight))
  const cancel = page
    .getByRole('banner', { name: '空间分析工具栏' })
    .getByRole('button', { name: '取消', exact: true })
  await expect(cancel).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-cancel-mode', 'defer'))
  await cancel.click()
  await expect(notice(page)).toContainText('正在取消空间分析')
  await expect(page.getByRole('button', { name: '正在取消…', exact: true })).toBeDisabled()
  await expect(notice(page)).not.toContainText('空间分析已取消')
  await page.setViewportSize({ width: 960, height: 780 })
  await page.evaluate(() => window.scrollTo(0, 0))
  await page.screenshot({
    path: 'test-results/analysis-task-pending.png',
    fullPage: true,
    animations: 'disabled',
  })
  await page.evaluate(() => {
    sessionStorage.setItem('mock-history-cancelled', 'yes')
    sessionStorage.setItem('mock-done', 'yes')
  })
  await expect(notice(page)).toContainText('空间分析已取消')
  await expect(page.getByRole('alert')).toHaveCount(0)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/previous')
  await page.evaluate(() => window.scrollTo(0, 0))
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/analysis-task-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await notice(page).getByRole('button', { name: '关闭分析任务提示' }).click()
  await expect(notice(page)).toHaveCount(0)
})

test('failure remains failure regardless of prose; retry uses the failed history target rather than edited input', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/a')
  await finish(page)
  await start(page, '/fixture/b')
  await finish(page)
  await page.getByRole('button', { name: '返回上次分析的目录' }).click()
  await expect(notice(page)).toContainText('空间分析进行中')
  await path(page).fill('/fixture/different')
  await page.evaluate(() =>
    sessionStorage.setItem('mock-history-error', 'fixture 文案包含已取消，但本次状态是失败'),
  )
  await finish(page)
  await expect(notice(page).getByRole('alert')).toContainText('空间分析未完成')
  await expect(notice(page)).not.toContainText('空间分析已取消')
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/b')
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  const retry = notice(page).getByRole('button', { name: '重新分析此目录' })
  await retry.focus()
  await page.keyboard.press('Enter')
  await expect(notice(page)).toContainText('空间分析进行中')
  expect((await starts(page)).at(-1).path).toBe('/fixture/a')
  await finish(page)
  await expect(notice(page)).toHaveCount(0)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/a')
  await expect(page.getByRole('button', { name: '前进到下一次分析的目录' })).toHaveAttribute(
    'title',
    '前进：/fixture/b',
  )
})

test('a rejected cancellation keeps tracking; a completion that wins the cancellation race still updates results', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/race')
  await page.evaluate(() => sessionStorage.setItem('mock-cancel-mode', 'reject'))
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(notice(page).getByRole('alert')).toContainText('fixture cancel request rejected')
  await expect(notice(page)).toContainText('空间分析进行中')
  await expect(page.getByRole('button', { name: '取消', exact: true })).toBeEnabled()
  await page.evaluate(() => sessionStorage.setItem('mock-cancel-mode', 'defer'))
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(notice(page)).toContainText('正在取消空间分析')
  await finish(page)
  await expect(notice(page)).toHaveCount(0)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/race')
})

test('late cancellation rejection cannot affect a newer task', async ({ page }) => {
  await open(page)
  await start(page, '/fixture/old')
  await page.evaluate(() => sessionStorage.setItem('mock-cancel-mode', 'late-reject'))
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(notice(page)).toContainText('正在取消空间分析')
  await finish(page)
  await start(page, '/fixture/new')
  await page.evaluate(() => sessionStorage.setItem('mock-cancel-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('mock-cancel-rejected')))
    .toBe('yes')
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  )
  await expect(notice(page)).toContainText('空间分析进行中')
  await expect(page.getByRole('alert')).toHaveCount(0)
  await expect(notice(page)).not.toContainText('fixture old cancel')
  await finish(page)
})

test('reload resumes the recorded path; loss of observation stays unconfirmed and continues from history without a new task', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/resume')
  await path(page).fill('/fixture/not-the-request')
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(notice(page)).toContainText('请求目录：/fixture/resume')
  expect(await starts(page)).toHaveLength(1)
  await page.evaluate(() => sessionStorage.setItem('mock-snapshot-error', 'yes'))
  await expect(notice(page)).toContainText('分析任务状态待确认', { timeout: 10_000 })
  await expect(notice(page)).toContainText('本机任务可能仍在运行')
  await expect(notice(page).getByRole('button', { name: '重新分析此目录' })).toHaveCount(0)
  await notice(page).getByRole('button', { name: '查看操作历史' }).click()
  await page.evaluate(() => sessionStorage.removeItem('mock-snapshot-error'))
  await page.getByRole('button', { name: '继续跟踪', exact: true }).click()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(notice(page)).toContainText('空间分析进行中')
  await expect(notice(page)).toContainText('/fixture/resume')
  expect(await starts(page)).toHaveLength(1)
  await finish(page)
  await expect(notice(page)).toHaveCount(0)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/resume')
})
