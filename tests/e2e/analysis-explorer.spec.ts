import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'

test('space map links to exact item details, keeps partial and unknown explicit, and supports keyboard', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(),
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  const map = explorer.getByRole('group', { name: '当前筛选的空间图' })
  await expect(map.getByRole('button')).toHaveCount(3)
  await expect(explorer.getByText('部分目录未完整测量', { exact: false })).toBeVisible()
  await expect(explorer.getByText('1 项无法测量、2 项已测量为零', { exact: false })).toBeVisible()
  const tile = map.getByRole('button', { name: /^查看 Projects/ })
  await tile.focus()
  await page.keyboard.press('Enter')
  const detail = explorer.getByRole('region', { name: '选中项目详情' })
  await expect(detail).toBeFocused()
  await expect(detail).toContainText('尚未完整测量')
  await expect(tile).toHaveAttribute('aria-pressed', 'true')
  await expect(explorer.locator('.analysis-row-selected')).toContainText('Projects')
  await detail.getByRole('button', { name: '分析此文件夹' }).click()
  await expect(page.getByLabel('fixture 路径请求')).toHaveText('analyze:/fixture/analysis/Projects')
  await explorer
    .getByRole('region', { name: '目录分页列表' })
    .getByRole('button', { name: '查看 /fixture/analysis/无法读取 详情', exact: true })
    .click()
  await expect(detail).toContainText('无法测量')
  await expect(detail).not.toContainText('0 B')
  await explorer.getByRole('textbox', { name: '筛选目录内容' }).fill('$(literal)')
  await expect(detail).toHaveCount(0)
  await expect(map.getByRole('button')).toHaveCount(1)
  await map.getByRole('button').click()
  await detail.getByRole('button', { name: '在 Finder 中显示' }).click()
  await expect(page.getByLabel('fixture 路径请求')).toHaveText(`reveal:${analysis.entries[5].path}`)
  await expect(detail.getByRole('button', { name: '分析此文件夹' })).toHaveCount(0)
})

test('filters preserve unknown and zero rows, empty states and readonly measurement boundaries', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(),
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  const rows = explorer.getByRole('region', { name: '目录分页列表' })
  await explorer.getByRole('combobox', { name: '目录项目类型' }).selectOption('unknown')
  await expect(rows.getByRole('article')).toHaveCount(1)
  await expect(rows).toContainText('无法测量')
  await expect(explorer.getByRole('group', { name: '当前筛选的空间图' })).toHaveCount(0)
  await explorer.getByRole('combobox', { name: '目录项目类型' }).selectOption('file')
  await expect(rows.getByRole('article')).toHaveCount(3)
  await expect(rows).toContainText('0 B')
  await explorer.getByRole('textbox', { name: '筛选目录内容' }).fill('not-found')
  await expect(explorer.getByText('没有匹配的项目。')).toBeVisible()
  await explorer.getByRole('button', { name: '清除筛选' }).click()
  await expect(rows.getByRole('article')).toHaveCount(6)
  await explorer.getByText('测量范围与图表说明', { exact: true }).click()
  await expect(
    explorer.getByText('不是实时快照、可释放空间或清理许可', { exact: false }),
  ).toBeVisible()
  await page.getByRole('button', { name: '模拟根目录不可读' }).click()
  await expect(
    explorer.getByText('没有可展示的子项；部分内容无法读取，请检查目录权限。'),
  ).toBeVisible()
  await expect(explorer.getByRole('button', { name: /删除|清理|移入废纸篓/ })).toHaveCount(0)
})

test('pagination reaches items beyond 200 and map selection locates their page with bounded rows', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(true),
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  const rows = explorer.getByRole('region', { name: '目录分页列表' })
  await expect(rows.getByRole('article')).toHaveCount(100)
  await explorer.getByRole('combobox', { name: '目录项目排序' }).selectOption('name')
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button', { name: /^查看 item-204/ })
    .click()
  await expect(explorer.getByText('显示 201–205 / 205 个匹配项目 · 第 3 / 3 页')).toBeVisible()
  await expect(rows.getByRole('article')).toHaveCount(5)
  await expect(explorer.locator('.analysis-row-selected')).toContainText('item-204')
  await explorer.getByRole('button', { name: '上一页', exact: true }).click()
  await expect(rows).toBeFocused()
  await expect(rows.getByRole('article')).toHaveCount(100)
  await expect(explorer.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
  await expect(
    explorer.getByRole('group', { name: '当前筛选的空间图' }).getByRole('button'),
  ).toHaveCount(12)
  await expect(explorer.getByText('其他 193 项合并绘制。', { exact: false })).toBeVisible()
  await page.getByRole('button', { name: '模拟较短结果' }).click()
  await expect(rows.getByRole('article')).toHaveCount(2)
  await expect(explorer.getByText('显示 1–2 / 2 个匹配项目 · 第 1 / 1 页')).toBeVisible()
})

test('new observations clear selected details and charts fit minimum window in both themes', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(),
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button')
    .first()
    .click()
  await page.getByRole('button', { name: '模拟重新读取成功' }).click()
  await expect(explorer.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await explorer.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({ path: `test-results/analysis-map-${theme}.png`, fullPage: true })
  }
})

test('native task results drive the map and folder navigation uses the selected path', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze', undefined, undefined, analysis)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录' }).fill('/fixture/analysis')
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await expect(
    explorer.getByRole('group', { name: '当前筛选的空间图' }).getByRole('button'),
  ).toHaveCount(3)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/analysis-page-${theme}.png`, fullPage: true })
  }
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button', { name: /^查看 Projects/ })
    .click()
  await explorer.getByRole('button', { name: '分析此文件夹' }).click()
  const starts = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(starts.at(-1).path).toBe('/fixture/analysis/Projects')
  await expect(page.getByRole('region', { name: '空间分析任务' })).toContainText(
    '上次成功结果仍保留',
  )
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(explorer.getByRole('region', { name: '选中项目详情' })).toHaveCount(0)
})
