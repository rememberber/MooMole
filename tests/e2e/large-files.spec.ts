import { expect, test, type Page } from '@playwright/test'
import type { Analysis, LargeFile } from '../../src/types'
import { installTaskMock } from './fixtures/native-task'

const root = '/fixture/large'
const specialParent = `${root}/中文 空格/[*] $(literal) 'quote'/` + 'long-directory-'.repeat(8)
const file = (path: string, size: number): LargeFile => ({
  name: path.slice(path.lastIndexOf('/') + 1),
  path,
  size,
})
const analysis: Analysis = {
  path: root,
  scan_status: 'partial',
  total_size: 1024,
  entries: [],
  large_files: [
    file(`${root}/sub/movie.MP4`, 2 * 1024 ** 3),
    file(`${root}/a/shared.zip`, 1024 ** 3),
    file(`${root}/b/shared.zip`, 120 * 1024 ** 2),
    file(`${specialParent}/中文\n报告.pdf`, 105 * 1024 ** 2),
    file(`${root}/empty`, 0),
    file(`${root}/unknown`, -1),
    file(`${root}-other/out.pdf`, 100 * 1024 ** 2),
  ],
}
const library = (page: Page) => page.getByRole('region', { name: 'Mole 大文件列表', exact: true })
const start = (page: Page) => page.getByRole('button', { name: '开始分析', exact: true })
async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(start(page)).toBeEnabled()
}
async function open(page: Page, value = analysis) {
  await installTaskMock(page, 'analyze', undefined, undefined, value)
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录' }).fill(root)
  await start(page).click()
  await finish(page)
  const toggle = page
    .getByRole('group', { name: '分析结果视图' })
    .getByRole('button', { name: '大文件', exact: true })
  await toggle.focus()
  await page.keyboard.press('Enter')
  await expect(toggle).toHaveAttribute('aria-pressed', 'true')
}

test('large-file view uses the independent result, filters returned paths, and routes exact read-only actions', async ({
  page,
}) => {
  await open(page)
  const view = library(page)
  await expect(view.getByRole('article')).toHaveCount(7)
  await expect(view).toContainText('列表并不完整')
  await expect(view).toContainText('当前目录未完整测量')
  await expect(view.getByText('大小未知', { exact: true })).toBeVisible()
  await expect(view.getByText('0 B', { exact: true })).toBeVisible()
  await expect(view.getByRole('button', { name: /删除|移入废纸篓|清理文件/ })).toHaveCount(0)
  await view.getByRole('combobox', { name: '大文件类型' }).selectOption('video')
  await expect(view.getByRole('article')).toHaveCount(1)
  await view.getByRole('combobox', { name: '大文件类型' }).selectOption('archive')
  await expect(view.getByRole('article')).toHaveCount(2)
  await view.getByRole('combobox', { name: '大文件最小大小' }).selectOption(String(1024 ** 3))
  await expect(view.getByRole('article')).toHaveCount(1)
  await view.getByRole('combobox', { name: '大文件类型' }).selectOption('all')
  await view.getByRole('combobox', { name: '大文件最小大小' }).selectOption('0')
  await view.getByRole('textbox', { name: '搜索大文件' }).fill(`${root}/b/`)
  await expect(view.getByRole('article')).toHaveCount(1)
  await view
    .getByRole('button', { name: `在 Finder 中显示大文件 ${root}/b/shared.zip`, exact: true })
    .click()
  expect(await page.evaluate(() => sessionStorage.getItem('mock-reveal-path'))).toBe(
    `${root}/b/shared.zip`,
  )
  await view.getByRole('textbox', { name: '搜索大文件' }).fill('large-other')
  await expect(view.getByRole('article')).toHaveCount(1)
  for (const button of await view.getByRole('article').getByRole('button').all())
    await expect(button).toBeDisabled()
  await view.getByRole('textbox', { name: '搜索大文件' }).fill('$(literal)')
  await view.getByText('大文件范围与来源', { exact: true }).click()
  await expect(view.getByText(/不保证列出所有大文件/)).toBeVisible()
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/large-files-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await view
    .getByRole('button', { name: `分析 ${specialParent}/中文\n报告.pdf 所在目录`, exact: true })
    .click()
  const starts = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(
    starts
      .filter((v: { kind: string }) => v.kind === 'analyze')
      .map((v: { path: string }) => v.path),
  ).toEqual([root, specialParent])
  await expect(view.getByRole('button', { name: /分析 .* 所在目录/ })).toBeDisabled()
})

test('unexpected larger lists page with bounded rows, and shorter new results keep the page valid', async ({
  page,
}) => {
  const files = Array.from({ length: 45 }, (_, i) =>
    file(`${root}/file-${String(i).padStart(2, '0')}.bin`, 100 + i),
  )
  await open(page, { ...analysis, large_files: files })
  const view = library(page)
  await expect(view.getByRole('article')).toHaveCount(20)
  await view.getByRole('combobox', { name: '大文件排序' }).selectOption('name')
  await view.getByRole('button', { name: '下一页', exact: true }).click()
  await expect(view.getByRole('region', { name: '大文件分页列表' })).toBeFocused()
  await view.getByRole('button', { name: '下一页', exact: true }).click()
  await expect(view.getByRole('article')).toHaveCount(5)
  await expect(view).toContainText('显示 41–45 / 45 条匹配记录')
  await page.evaluate(
    (value) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(value)),
    { ...analysis, large_files: files.slice(0, 2) },
  )
  await start(page).click()
  await finish(page)
  await expect(view.getByRole('article')).toHaveCount(2)
  await expect(view).toContainText('显示 1–2 / 2 条匹配记录')
  await view.getByRole('textbox', { name: '搜索大文件' }).fill('no-match')
  await expect(view.getByText('没有匹配的大文件。')).toBeVisible()
  await view.getByRole('button', { name: '清除大文件筛选' }).click()
  await expect(view.getByRole('article')).toHaveCount(2)
})

test('omitted, empty and malformed large-file payloads stay explicit even when directory scanning completed', async ({
  page,
}) => {
  await open(page, { ...analysis, scan_status: 'complete', large_files: undefined })
  for (const value of [undefined, [], [null, { size: 100 }]]) {
    if (value !== undefined) {
      await page.evaluate(
        (result) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(result)),
        { ...analysis, scan_status: 'complete', large_files: value as LargeFile[] },
      )
      await start(page).click()
      await finish(page)
    }
    const view = library(page)
    await expect(view.getByRole('article')).toHaveCount(0)
    if (value === undefined) {
      await expect(view).toContainText('本次结果没有提供大文件列表')
      await expect(view).toContainText('大文件返回数量未知')
    } else if (!value.length)
      await expect(view).toContainText('本次返回的大文件列表为空，不代表目录里没有其他大文件')
    else await expect(view).toContainText('2 条记录格式无法识别')
    await expect(view.getByRole('button', { name: /分析 .* 所在目录/ })).toHaveCount(0)
  }
})

test('failed and cancelled rereads preserve the old large-file list and filters; a successful path change returns to directory view', async ({
  page,
}) => {
  await open(page)
  const view = library(page)
  await view.getByRole('textbox', { name: '搜索大文件' }).fill(`${root}/b/`)
  await start(page).click()
  await expect(view.getByRole('article')).toHaveCount(1)
  await page.evaluate(() => {
    sessionStorage.setItem('mock-history-error', 'fixture 读取失败')
    sessionStorage.setItem('mock-done', 'yes')
  })
  await expect(start(page)).toBeEnabled()
  await expect(page.getByRole('alert')).toContainText('fixture 读取失败')
  await expect(view.getByRole('textbox', { name: '搜索大文件' })).toHaveValue(`${root}/b/`)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await start(page).click()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(view.getByRole('article')).toHaveCount(1)
  await page.evaluate(
    (value) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(value)),
    { ...analysis, path: `${root}/sub`, large_files: [] },
  )
  await page.getByRole('textbox', { name: '扫描目录' }).fill(`${root}/sub`)
  await start(page).click()
  await finish(page)
  await expect(view).toHaveCount(0)
  await expect(
    page
      .getByRole('group', { name: '分析结果视图' })
      .getByRole('button', { name: '目录内容', exact: true }),
  ).toHaveAttribute('aria-pressed', 'true')
})
