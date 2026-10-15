import { expect, test } from '@playwright/test'

test('browser preview labels its data and cannot execute real tasks', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto('/')
  await expect(page.getByText('浏览器预览', { exact: true })).toBeVisible()
  await expect(page.getByRole('heading', { name: '把空间，还给重要的事。' })).toBeVisible()
  await expect(page.getByRole('button', { name: '刷新状态' })).toBeDisabled()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByText('部分目录未完整测量', { exact: false })).toBeVisible()
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('Projects')
  await expect(page.getByRole('button', { name: 'Projects 文件夹' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Library 文件夹' })).toHaveCount(0)
  await page.getByRole('button', { name: '清理', exact: true }).click()
  await expect(page.getByText('当前只提供扫描与审阅', { exact: false })).toBeVisible()
  await expect(page.getByRole('button', { name: '重新扫描', exact: true })).toBeDisabled()
  expect(errors).toEqual([])
})

test('structured preview supports selection, subset totals, filters and diagnostics', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '清理', exact: true }).click()
  const review = page.getByRole('region', { name: 'Mole 清理项目审阅' })
  await expect(review.getByText('3 个候选路径', { exact: false })).toBeVisible()
  const parent = review.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp',
    exact: true,
  })
  const child = review.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
    exact: true,
  })
  await child.check()
  await expect(review.getByRole('status')).toHaveText('已勾选 1 项 · 512 MB')
  await parent.check()
  await expect(review.getByRole('status')).toHaveText('已勾选 2 项 · 1.5 GB')
  await review.getByRole('combobox', { name: '清理项目状态' }).selectOption('unknown')
  await expect(parent).toHaveCount(0)
  await review
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Logs/ExampleApp', exact: true })
    .check()
  await expect(review.getByRole('status')).toHaveText('已勾选 3 项 · 1.5 GB，另有 1 项大小未知')
  await review.getByRole('combobox', { name: '清理项目状态' }).selectOption('all')
  await review.getByRole('textbox', { name: '搜索清理项目' }).fill('Images')
  await expect(child).toBeVisible()
  await expect(parent).toHaveCount(0)
  await review.locator('.review-detail > summary').click()
  await expect(review.getByText('完整预览中，大小已计入父目录', { exact: false })).toBeVisible()
  await expect(review.getByRole('button', { name: '保护此目录' })).toBeDisabled()
  await review.getByText('查看 Mole 原始日志', { exact: true }).click()
  await expect(review.getByLabel('Mole 清理预览日志')).toContainText('没有扫描本机文件')
  await page.screenshot({ path: 'test-results/clean-review.png', fullPage: true })
})

test('settings modal supports theme changes, keyboard dismissal and about copy', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: false }).click()
  const dialog = page.getByRole('dialog', { name: '设置', exact: true })
  await expect(dialog).toBeVisible()
  await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('dark')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await dialog.getByRole('button', { name: '关于', exact: true }).click()
  await expect(dialog.getByText('本项目仅供学习和研究使用', { exact: false })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '访问 Mole 官方产品' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
})

test('protecting a selected child immediately excludes it and its parent from review', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => {
    // Test fixture is served by the development server and never bundled in the app.
    const fixture = await import('/tests/e2e/fixtures/clean-review.tsx')
    fixture.mount()
  })
  const fixture = page.locator('#review-test-fixture')
  const parent = fixture.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp',
    exact: true,
  })
  const child = fixture.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
    exact: true,
  })
  await parent.check()
  await child.check()
  await fixture.getByText('Images', { exact: true }).click()
  await fixture.getByRole('button', { name: '保护此目录' }).click()
  await expect(parent).toBeDisabled()
  await expect(child).toBeDisabled()
  await expect(fixture.getByRole('status')).toHaveText('已勾选 0 项 · 0 B')
  await fixture.getByRole('combobox', { name: '清理项目状态' }).selectOption('protected')
  await expect(fixture.locator('.review-item')).toHaveCount(2)
  await expect(fixture.getByRole('checkbox', { name: '勾选本页可审阅项目' })).toBeDisabled()
})

test('all primary pages render without horizontal overflow', async ({ page }) => {
  await page.goto('/')
  for (const name of ['空间分析', '清理', '应用列表', '系统状态', '操作历史', '概览']) {
    await page
      .getByRole('navigation', { name: '主导航' })
      .getByRole('button', { name, exact: true })
      .click()
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true)
  }
})

test('review plan dialog saves selected scope and explains changed identities on recheck', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => {
    const fixture = await import('/tests/e2e/fixtures/clean-review.tsx')
    fixture.mount()
  })
  const fixture = page.locator('#review-test-fixture')
  await expect(fixture.getByRole('button', { name: '保存审阅计划' })).toBeDisabled()
  await fixture
    .getByRole('checkbox', {
      name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
      exact: true,
    })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await expect(dialog.getByText('1 个项目', { exact: true })).toBeVisible()
  await expect(
    dialog.getByText('/Users/demo/Library/Caches/ExampleApp/Images', { exact: true }),
  ).toBeVisible()
  await expect(dialog.getByText('当前未开放执行', { exact: false })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.getByText('本次没有需要重新确认的项目。')).toBeVisible()
  await dialog.getByRole('button', { name: '重新复核' }).click()
  await expect(dialog.getByText('目标已变化', { exact: true })).toBeVisible()
  await expect(dialog.getByText('文件身份已变化，需要重新扫描。')).toBeVisible()
  await page.screenshot({ path: 'test-results/review-plan.png', fullPage: true })
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
})
