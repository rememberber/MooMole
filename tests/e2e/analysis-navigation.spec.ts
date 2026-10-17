import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

const pathBar = (page: Page) => page.getByRole('textbox', { name: '扫描目录', exact: true })
const back = (page: Page) => page.getByRole('button', { name: '返回上次分析的目录', exact: true })
const forward = (page: Page) =>
  page.getByRole('button', { name: '前进到下一次分析的目录', exact: true })
const current = (page: Page) =>
  page.getByRole('region', { name: 'Mole 空间分析结果' }).locator('.analysis-path')

async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByRole('button', { name: '开始分析', exact: true })).toBeEnabled()
}
async function open(page: Page) {
  await installTaskMock(page, 'navigation')
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
}
async function start(page: Page, path: string) {
  await pathBar(page).fill(path)
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await expect(page.getByText('空间分析进行中', { exact: true })).toBeVisible()
}

test('successful folder visits support back and forward, reread each target, and survive page navigation', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/start')
  await finish(page)
  await expect(back(page)).toBeDisabled()
  await expect(forward(page)).toBeDisabled()
  await page.getByRole('button', { name: 'child 文件夹', exact: true }).click()
  await expect(current(page)).toHaveAttribute('title', '/fixture/start')
  await expect(page.getByRole('button', { name: '分析目录 /fixture', exact: true })).toBeDisabled()
  await expect(back(page)).toBeDisabled()
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture/start/child')
  await back(page).click()
  await expect(current(page)).toHaveAttribute('title', '/fixture/start/child')
  await expect(forward(page)).toBeDisabled()
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture/start')
  await expect(forward(page)).toBeEnabled()
  await forward(page).click()
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture/start/child')
  const starts = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(
    starts
      .filter((v: { kind: string }) => v.kind === 'analyze')
      .map((v: { path: string }) => v.path),
  ).toEqual(['/fixture/start', '/fixture/start/child', '/fixture/start', '/fixture/start/child'])
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(back(page)).toBeEnabled()
  await expect(current(page)).toHaveAttribute('title', '/fixture/start/child')
})

test('failure and cancellation leave the current visit and forward branch intact; a new success replaces it', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/a')
  await finish(page)
  await start(page, '/fixture/b')
  await finish(page)
  await back(page).click()
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture/a')
  await start(page, '/fixture/rejected')
  await page.evaluate(() => sessionStorage.setItem('mock-history-error', 'fixture 目录不可访问'))
  await finish(page)
  await expect(page.getByRole('alert')).toContainText('fixture 目录不可访问')
  await expect(current(page)).toHaveAttribute('title', '/fixture/a')
  await expect(forward(page)).toBeEnabled()
  await expect(forward(page)).toHaveAttribute('title', '前进：/fixture/b')
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await start(page, '/fixture/cancelled')
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(page.getByRole('button', { name: '开始分析', exact: true })).toBeEnabled()
  await expect(current(page)).toHaveAttribute('title', '/fixture/a')
  await expect(forward(page)).toHaveAttribute('title', '前进：/fixture/b')
  await start(page, '/fixture/new')
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture/new')
  await expect(forward(page)).toBeDisabled()
  await expect(back(page)).toHaveAttribute('title', '返回：/fixture/a')
})

test('result breadcrumbs use returned paths, navigate literal ancestors by keyboard, and reset on reload', async ({
  page,
}) => {
  await open(page)
  await start(page, '/fixture/alias')
  await finish(page)
  await expect(pathBar(page)).toHaveValue('/fixture/canonical')
  await expect(current(page)).toHaveAttribute('title', '/fixture/canonical')
  const ancestor = page.getByRole('button', { name: '分析目录 /fixture', exact: true })
  await ancestor.focus()
  await page.keyboard.press('Enter')
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', '/fixture')
  await expect(back(page)).toHaveAttribute('title', '返回：/fixture/canonical')
  const special = "/fixture/中文 空格/[*] $(literal) 'quote'/" + 'very-long-directory-'.repeat(8)
  await start(page, special)
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', special)
  const crumbs = page.getByRole('navigation', { name: '分析目录路径' })
  await expect(crumbs.locator('[aria-current="location"]')).toHaveText(special.split('/').at(-1)!)
  await expect(pathBar(page)).toHaveValue(special)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/analysis-navigation-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  const parent = "/fixture/中文 空格/[*] $(literal) 'quote'"
  await crumbs.getByRole('button', { name: `分析目录 ${parent}`, exact: true }).focus()
  await page.keyboard.press('Enter')
  await expect(current(page)).toHaveAttribute('title', special)
  await finish(page)
  await expect(current(page)).toHaveAttribute('title', parent)
  await expect(pathBar(page)).toHaveValue(parent)
  const starts = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(starts.at(-1).path).toBe(parent)
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByText('先选择一个文件夹', { exact: true })).toBeVisible()
  await expect(back(page)).toHaveCount(0)
})
