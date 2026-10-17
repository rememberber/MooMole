import { expect, test, type Page } from '@playwright/test'
import type { Analysis } from '../../src/types'
import { installTaskMock } from './fixtures/native-task'

const a: Analysis = {
  path: '/fixture/a',
  scan_status: 'complete',
  total_size: 1000,
  entries: Array.from({ length: 219 }, (_, i) => ({
    name: `item-${String(i).padStart(3, '0')}.bin`,
    path: `/fixture/a/item-${String(i).padStart(3, '0')}.bin`,
    size: i + 1,
    is_dir: false,
    scan_status: 'complete',
  })),
  large_files: Array.from({ length: 45 }, (_, i) => ({
    name: `video-${String(i).padStart(2, '0')}.mp4`,
    path: `/fixture/a/nested/video-${String(i).padStart(2, '0')}.mp4`,
    size: 200 * 1024 ** 2 + i,
  })),
}
const b: Analysis = {
  path: '/fixture/b',
  scan_status: 'complete',
  total_size: 8,
  entries: [
    { name: 'b-item', path: '/fixture/b/b-item', size: 8, is_dir: false, scan_status: 'complete' },
  ],
}
const back = (page: Page) => page.getByRole('button', { name: '返回上次分析的目录', exact: true })
const forward = (page: Page) =>
  page.getByRole('button', { name: '前进到下一次分析的目录', exact: true })
const directory = (page: Page) => page.getByRole('region', { name: '目录分页列表', exact: true })
const start = (page: Page) => page.getByRole('button', { name: '开始分析', exact: true })
async function result(page: Page, value: Analysis) {
  await page.evaluate(
    (v) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(v)),
    value,
  )
}
async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(start(page)).toBeEnabled()
}
async function visit(page: Page, value: Analysis) {
  await result(page, value)
  await page.getByRole('textbox', { name: '扫描目录' }).fill(value.path)
  await start(page).click()
  await finish(page)
}
async function open(page: Page) {
  await installTaskMock(page, 'analyze', undefined, undefined, a)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await visit(page, a)
}
async function setDirectoryContext(page: Page) {
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('item-')
  await page.getByRole('combobox', { name: '目录项目类型' }).selectOption('file')
  await page.getByRole('combobox', { name: '目录项目排序' }).selectOption('name')
  await page.getByRole('button', { name: '下一页', exact: true }).click()
  await page.getByRole('button', { name: '下一页', exact: true }).click()
  await directory(page)
    .getByRole('button', { name: '查看 /fixture/a/item-218.bin 详情', exact: true })
    .click()
}

test('back rereads data and restores filters, the selected path, page and list scroll without restoring stale size', async ({
  page,
}) => {
  await open(page)
  await setDirectoryContext(page)
  const scroll = await directory(page).evaluate((node) => node.scrollTop)
  expect(scroll).toBeGreaterThan(0)
  await visit(page, b)
  await expect(page.getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('')
  const changed = {
    ...a,
    entries: a.entries.map((entry) =>
      entry.path.endsWith('218.bin')
        ? { ...entry, size: 8192, scan_status: 'partial' as const }
        : entry,
    ),
    scan_status: 'partial' as const,
  }
  await result(page, changed)
  await back(page).click()
  await expect(page.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
  await finish(page)
  await expect(page.getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('item-')
  await expect(page.getByRole('combobox', { name: '目录项目类型' })).toHaveValue('file')
  await expect(page.getByRole('combobox', { name: '目录项目排序' })).toHaveValue('name')
  await expect(page.getByText('显示 201–219 / 219 个匹配项目 · 第 3 / 3 页')).toBeVisible()
  const detail = page.getByRole('region', { name: '选中项目详情' })
  await expect(detail).toContainText('item-218.bin')
  await expect(detail).toContainText('8 KB+')
  await expect(detail).toContainText('尚未完整测量')
  expect(await directory(page).evaluate((node) => node.scrollTop)).toBeCloseTo(scroll, 0)
  await expect(detail).not.toBeFocused()
})

test('large-file preferences survive view switches, page navigation and history; shorter results clamp pages', async ({
  page,
}) => {
  await open(page)
  const views = page.getByRole('group', { name: '分析结果视图' })
  await views.getByRole('button', { name: '大文件', exact: true }).click()
  const large = page.getByRole('region', { name: 'Mole 大文件列表', exact: true })
  const rows = large.getByRole('region', { name: '大文件分页列表' })
  await large.getByRole('textbox', { name: '搜索大文件' }).fill('video-')
  await large.getByRole('combobox', { name: '大文件类型' }).selectOption('video')
  await large
    .getByRole('combobox', { name: '大文件最小大小' })
    .selectOption(String(100 * 1024 ** 2))
  await large.getByRole('combobox', { name: '大文件排序' }).selectOption('name')
  await large.getByRole('button', { name: '下一页', exact: true }).click()
  await rows.evaluate((node) => (node.scrollTop = 150))
  await views.getByRole('button', { name: '目录内容', exact: true }).click()
  await views.getByRole('button', { name: '大文件', exact: true }).click()
  await expect(large).toContainText('显示 21–40 / 45 条匹配记录')
  expect(await rows.evaluate((node) => node.scrollTop)).toBe(150)
  const savedScroll = await page.evaluate(() => {
    window.scrollTo(0, 400)
    return window.scrollY
  })
  expect(savedScroll).toBeGreaterThan(0)
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.evaluate(() => window.scrollTo(0, 0))
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(large.getByRole('textbox', { name: '搜索大文件' })).toHaveValue('video-')
  expect(await page.evaluate(() => window.scrollY)).toBe(savedScroll)
  expect(await rows.evaluate((node) => node.scrollTop)).toBe(150)
  await visit(page, b)
  await result(page, { ...a, large_files: a.large_files!.slice(0, 2) })
  await back(page).click()
  await finish(page)
  await expect(views.getByRole('button', { name: '大文件', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  )
  await expect(large.getByRole('combobox', { name: '大文件类型' })).toHaveValue('video')
  await expect(large.getByRole('combobox', { name: '大文件最小大小' })).toHaveValue(
    String(100 * 1024 ** 2),
  )
  await expect(large.getByRole('combobox', { name: '大文件排序' })).toHaveValue('name')
  await expect(large).toContainText('显示 1–2 / 2 条匹配记录')
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByText('先选择一个文件夹', { exact: true })).toBeVisible()
  await expect(back(page)).toHaveCount(0)
})

test('failed and cancelled history visits retain live preferences; missing selection uses fresh unknown results', async ({
  page,
}) => {
  await open(page)
  await setDirectoryContext(page)
  await visit(page, b)
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('old b filter')
  const changed: Analysis = {
    ...a,
    scan_status: 'partial',
    entries: [{ ...a.entries[0], size: 0, scan_status: 'unavailable' }],
  }
  await result(page, changed)
  await back(page).click()
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('b-')
  await page.evaluate(() => sessionStorage.setItem('mock-history-error', 'fixture back failed'))
  await finish(page)
  await expect(page.getByRole('alert')).toContainText('fixture back failed')
  await expect(page.getByRole('textbox', { name: '扫描目录' })).toHaveValue(b.path)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await back(page).click()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(start(page)).toBeEnabled()
  await expect(page.getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('b-')
  await back(page).click()
  await finish(page)
  await expect(
    page.getByText('上次选中的项目未在本次结果或当前筛选中找到，已取消选中。'),
  ).toBeVisible()
  await expect(page.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
  await expect(page.getByText('显示 1–1 / 1 个匹配项目 · 第 1 / 1 页')).toBeVisible()
  await expect(directory(page)).toContainText('无法测量')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/analysis-context-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await result(page, b)
  await forward(page).click()
  await finish(page)
  await expect(page.getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('b-')
})

test('restored selection follows its fresh page after reordering and same-path refresh clears the old selection', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('button', { name: '查看 item-218.bin · 219 B', exact: true }).click()
  await visit(page, b)
  await result(page, {
    ...a,
    entries: a.entries.map((entry) =>
      entry.path.endsWith('218.bin') ? { ...entry, size: 0 } : entry,
    ),
  })
  await back(page).click()
  await finish(page)
  await expect(page.getByText('显示 201–219 / 219 个匹配项目 · 第 3 / 3 页')).toBeVisible()
  await expect(page.getByRole('region', { name: '选中项目详情' })).toContainText('0 B')
  await start(page).click()
  await finish(page)
  await expect(page.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
  await expect(page.getByText('显示 201–219 / 219 个匹配项目 · 第 3 / 3 页')).toBeVisible()
})

test('canonical history targets use the returned path context even when it matches the current directory', async ({
  page,
}) => {
  await open(page)
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('item-')
  await visit(page, b)
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('b-')
  await directory(page)
    .getByRole('button', { name: '查看 /fixture/b/b-item 详情', exact: true })
    .click()
  await result(page, {
    ...b,
    scan_status: 'partial',
    entries: [{ ...b.entries[0], size: 40, scan_status: 'partial' }],
  })
  await back(page).click()
  await finish(page)
  await expect(page.getByRole('textbox', { name: '扫描目录' })).toHaveValue(b.path)
  await expect(page.getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('b-')
  await expect(page.getByRole('region', { name: '选中项目详情' })).toContainText('40 B+')
  await expect(back(page)).toBeDisabled()
  await expect(forward(page)).toBeEnabled()
})
