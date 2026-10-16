import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'

test('application search and sorting keep copies separate and unknown sizes distinct from zero', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/application-library.tsx')).mount(),
  )
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  const rows = library.getByRole('article')
  await expect(rows).toHaveCount(5)
  await expect(rows.nth(0)).toContainText('/Applications/Example.app')
  await expect(rows.nth(1)).toContainText('~/Applications/Example Beta.app')
  await expect(rows.nth(2)).toContainText('0B')
  await expect(rows.nth(3).getByText('大小未知', { exact: true })).toBeVisible()
  await library.getByRole('textbox', { name: '搜索应用' }).fill('ORG.EXAMPLE.SAME')
  await expect(rows).toHaveCount(2)
  await library.getByRole('textbox', { name: '搜索应用' }).fill('Example Beta.app')
  await expect(rows).toHaveCount(1)
  await library.getByRole('textbox', { name: '搜索应用' }).fill('中文 外接盘')
  await expect(rows).toHaveCount(1)
  await expect(rows.getByText('来源待确认', { exact: true })).toBeVisible()
  await rows.getByText('位置与来源', { exact: true }).click()
  await expect(rows.getByText(applications[4].path, { exact: true }).last()).toBeVisible()
  await rows.getByRole('button', { name: '在 Finder 中显示' }).click()
  await expect(page.getByLabel('fixture Finder 请求')).toHaveText(applications[4].path)
  await expect(rows.getByText('future-manager', { exact: true })).toBeVisible()
  await library.getByRole('textbox', { name: '搜索应用' }).fill('')
  await library.getByRole('combobox', { name: '应用来源' }).selectOption('Homebrew')
  await expect(rows).toHaveCount(1)
  await library.getByRole('combobox', { name: '应用来源' }).selectOption('all')
  await library.getByRole('combobox', { name: '应用排序' }).selectOption('name')
  const names = await rows.locator('.app-info strong').allTextContents()
  expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b, 'zh-CN')))
})

test('comparison is explicit, spans filters, clears on a new list and never offers uninstall', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/application-library.tsx')).mount(),
  )
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await library
    .getByRole('checkbox', { name: '对比 /Applications/Example.app', exact: true })
    .check()
  await expect(library.getByRole('checkbox', { name: '勾选当前显示的应用' })).toHaveJSProperty(
    'indeterminate',
    true,
  )
  await library.getByRole('checkbox', { name: '对比 /Applications/Steam.app', exact: true }).check()
  await expect(library.getByRole('status')).toHaveText(
    '已勾选 2 项 · 已知估算约 1 GB · 1 项大小未知',
  )
  await library.getByRole('combobox', { name: '应用来源' }).selectOption('Homebrew')
  await expect(library.getByRole('article')).toHaveCount(1)
  await expect(library.getByRole('status')).toContainText('2 项未显示')
  await expect(library.getByRole('checkbox', { name: /对比 .*Beta/ })).not.toBeChecked()
  await library.getByRole('checkbox', { name: '勾选当前显示的应用' }).check()
  await expect(library.getByRole('status')).toContainText('已勾选 3 项')
  await library.getByRole('combobox', { name: '应用来源' }).selectOption('all')
  await library.getByRole('checkbox', { name: '只看已勾选' }).check()
  await expect(library.getByRole('article')).toHaveCount(3)
  await library.getByRole('button', { name: '清空勾选' }).click()
  await expect(library.getByText('没有符合条件的应用。')).toBeVisible()
  await library.getByRole('button', { name: '清除筛选' }).click()
  await library.getByRole('checkbox', { name: '对比 /Applications/Steam.app', exact: true }).check()
  await expect(library.getByRole('status')).toContainText('所选大小均未知')
  await page.getByRole('button', { name: '模拟列表刷新成功' }).click()
  await expect(
    library.getByRole('checkbox', { name: '对比 /Applications/Steam.app', exact: true }),
  ).not.toBeChecked()
  await expect(library.getByRole('status')).toHaveText('勾选可对比占用，不会卸载。')
  await expect(library.getByRole('button', { name: /卸载|删除|执行/ })).toHaveCount(0)
  await library.getByText('列表范围与大小说明', { exact: true }).click()
  await expect(library.getByText('不是可释放空间', { exact: false })).toBeVisible()
  await page.getByRole('button', { name: '模拟空清单' }).click()
  await expect(library.getByText('Mole 没有返回应用。')).toBeVisible()
})

test('large application lists reveal bounded batches and page selection only selects visible rows', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/application-library.tsx')).mount(true),
  )
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(100)
  await library.getByRole('checkbox', { name: '勾选当前显示的应用' }).check()
  await expect(library.getByRole('status')).toContainText('已勾选 100 项')
  await library.getByRole('button', { name: '再显示 100 项' }).click()
  await expect(library.getByRole('article')).toHaveCount(115)
  await expect(library.getByRole('checkbox', { name: '勾选当前显示的应用' })).toHaveJSProperty(
    'indeterminate',
    true,
  )
  await library.getByRole('checkbox', { name: '只看已勾选' }).check()
  await expect(library.getByRole('article')).toHaveCount(100)
})

test('the app routes actual task results to the library and clears comparisons after refresh or navigation', async ({
  page,
}) => {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(5)
  const check = library.getByRole('checkbox', {
    name: '对比 /Applications/Example.app',
    exact: true,
  })
  await check.check()
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toBeVisible()
  await page.evaluate(() => {
    sessionStorage.setItem('mock-history-error', 'fixture 应用清单读取失败')
    sessionStorage.setItem('mock-done', 'yes')
  })
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toHaveCount(0)
  await expect(check).toBeChecked()
  await expect(library.getByRole('article')).toHaveCount(5)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(check).not.toBeChecked()
  await check.check()
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await expect(check).not.toBeChecked()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/application-library-${theme}.png`, fullPage: true })
  }
})
