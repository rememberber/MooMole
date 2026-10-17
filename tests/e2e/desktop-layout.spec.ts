import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'

test('compact desktop shell puts actions in the toolbar and results above the fold without duplicate page chrome', async ({
  page,
}) => {
  await installTaskMock(
    page,
    'analyze',
    undefined,
    [
      {
        name: 'Fixture App',
        bundle_id: 'fixture.app',
        source: 'normal',
        uninstall_name: 'Fixture App',
        path: '/Applications/Fixture App.app',
        size: '1 GB',
      },
    ],
    analysis,
  )
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const name of ['概览', '空间分析', '清理', '应用列表', '系统状态', '操作历史']) {
    await page
      .getByRole('navigation', { name: '主导航' })
      .getByRole('button', { name, exact: true })
      .click()
    const toolbar = page.getByRole('banner', { name: `${name}工具栏` })
    await expect(toolbar).toBeVisible()
    await expect(page.getByRole('main', { name: `${name}内容` })).toBeVisible()
    await expect(
      page.locator('main h1, .eyebrow, .breadcrumb, .content-footer, .action-card'),
    ).toHaveCount(0)
    if (name !== '空间分析')
      await expect(page.getByRole('navigation', { name: '分析目录路径' })).toHaveCount(0)
    expect((await toolbar.boundingBox())!.height).toBeLessThanOrEqual(64)
    if (name === '空间分析') {
      await toolbar.getByRole('textbox', { name: '扫描目录' }).fill(analysis.path)
      await toolbar.getByRole('button', { name: '开始分析', exact: true }).click()
      await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
      await expect(toolbar.getByRole('button', { name: '开始分析', exact: true })).toBeEnabled()
      expect(
        (await page.getByRole('region', { name: 'Mole 空间分析结果' }).boundingBox())!.y,
      ).toBeLessThan(100)
      await expect(toolbar.getByRole('button', { name: '上一级', exact: true })).toBeEnabled()
      await expect(page.getByRole('navigation', { name: '分析目录路径' })).toBeVisible()
    }
    if (name === '应用列表') {
      await toolbar.getByRole('button', { name: '读取应用', exact: true }).click()
      await expect(page.getByText('Fixture App', { exact: true })).toBeVisible()
    }
    if (name === '概览') {
      const engine = page.getByRole('region', { name: 'Mole CLI 版本' })
      await expect(engine).toContainText('v1.59.1')
      expect((await engine.boundingBox())!.y).toBeLessThan(100)
    }
    for (const theme of ['light', 'dark']) {
      await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
      expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
        true,
      )
      await page.screenshot({
        path: `test-results/desktop-${name}-${theme}.png`,
        fullPage: true,
        animations: 'disabled',
      })
    }
  }
  await page.getByRole('button', { name: '设置', exact: true }).click()
  const about = page.getByRole('dialog', { name: '设置' })
  await about.getByRole('button', { name: '关于', exact: true }).click()
  await expect(about).toContainText('学习研究')
  await expect(about).toContainText('风险')
  await expect(about.getByRole('button', { name: '访问 Mole 官方产品', exact: true })).toBeVisible()
})

test('root navigation remains disabled and the compact path field works with the keyboard', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze', undefined, undefined, {
    ...analysis,
    path: '/',
    entries: [],
    total_size: 0,
    scan_status: 'unavailable',
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  const path = page.getByRole('textbox', { name: '扫描目录' })
  await path.fill('/')
  await path.press('Enter')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByRole('button', { name: '开始分析', exact: true })).toBeEnabled()
  await expect(page.getByRole('button', { name: '上一级', exact: true })).toBeDisabled()
  const crumbs = page.getByRole('navigation', { name: '分析目录路径' })
  await expect(crumbs.locator('[aria-current="location"]')).toHaveText('/')
  await expect(crumbs.getByRole('button')).toHaveCount(0)
  await expect(page.getByRole('region', { name: 'Mole 空间分析结果' })).toContainText('无法测量')
  await path.focus()
  await expect(path).toBeFocused()
  expect(await path.evaluate((node) => getComputedStyle(node.closest('form')!).outlineStyle)).toBe(
    'solid',
  )
})
