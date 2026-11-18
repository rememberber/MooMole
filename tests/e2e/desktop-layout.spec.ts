import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'

for (const [pageName, listName, firstName, secondName] of [
  ['应用列表', '应用管理类型', '已安装应用', '启动与后台'],
  ['文件整理', '文件整理类型', '普通文件', 'Live Photos'],
  ['软件清理', '软件内容类型', '缓存', '附件、下载与离线'],
]) {
  test(`${listName} supports keyboard navigation and explicit activation`, async ({ page }) => {
    await page.goto('/')
    await page.getByRole('button', { name: pageName, exact: true }).click()
    const list = page.getByRole('tablist', { name: listName, exact: true })
    const first = list.getByRole('tab', { name: firstName, exact: true })
    const second = list.getByRole('tab', { name: secondName, exact: true })
    await expect(first).toHaveAttribute('tabindex', '0')
    await expect(second).toHaveAttribute('tabindex', '-1')
    await expect(page.getByRole('tabpanel', { name: firstName, exact: true })).toBeVisible()
    await first.focus()
    await page.keyboard.press('ArrowLeft')
    await expect(second).toBeFocused()
    await expect(first).toHaveAttribute('aria-selected', 'true')
    await page.keyboard.press('Home')
    await expect(first).toBeFocused()
    await page.keyboard.press('End')
    await expect(second).toBeFocused()
    await page.keyboard.press('Enter')
    await expect(second).toHaveAttribute('aria-selected', 'true')
    const panel = page.getByRole('tabpanel', { name: secondName, exact: true })
    await expect(panel).toBeVisible()
    await page.keyboard.press('Tab')
    await expect(panel).toBeFocused()
    await page.keyboard.press('Shift+Tab')
    await expect(second).toBeFocused()
    await page.keyboard.press('ArrowRight')
    await expect(first).toBeFocused()
    await page.keyboard.press('Space')
    await expect(first).toHaveAttribute('aria-selected', 'true')
    await expect(panel).toHaveCount(0)
  })
}

test('the sticky toolbar adds its divider only while the page is scrolled', async ({ page }) => {
  await page.goto('/')
  const toolbar = page.getByRole('banner', { name: '概览工具栏' })
  await expect(toolbar).toHaveCSS('border-bottom-color', 'rgba(0, 0, 0, 0)')
  await page.locator('main').evaluate((main) => {
    main.style.minHeight = '300vh'
    window.scrollTo(0, 80)
  })
  await expect(toolbar).toHaveCSS('border-bottom-color', 'rgb(236, 238, 241)')
  expect((await toolbar.boundingBox())!.y).toBe(0)
  await page.evaluate(() => window.scrollTo(0, 0))
  await expect(toolbar).toHaveCSS('border-bottom-color', 'rgba(0, 0, 0, 0)')
})

for (const [locale, longLabel] of [
  ['zh-CN', '相似照片与视频'],
  ['en', 'Similar photos and videos'],
  ['ja', 'ソフトウェアのクリーンアップ'],
] as const) {
  test.describe(locale, () => {
    test.use({ locale })

    test('shows full sidebar labels at the default and minimum desktop sizes', async ({ page }) => {
      await page.goto('/')
      const nav = page.locator('.main-nav')
      const longItem = nav.getByRole('button', { name: longLabel, exact: true })
      await longItem.click()
      await expect(longItem).toHaveAttribute('aria-current', 'page')

      for (const [width, height] of [
        [1200, 820],
        [960, 680],
      ]) {
        await page.setViewportSize({ width, height })
        await expect(longItem.locator('span').first()).toBeVisible()
        const clippedLabels = await nav
          .locator('button > span:first-of-type')
          .evaluateAll((labels) =>
            labels
              .filter(
                (label) =>
                  label.scrollWidth > label.clientWidth || label.scrollHeight > label.clientHeight,
              )
              .map((label) => label.textContent),
          )
        expect(clippedLabels, `${locale} at ${width} × ${height}`).toEqual([])
      }
    })
  })
}

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
      const clean = page.getByRole('button', { name: '扫描可清理项' })
      await expect(clean).toBeVisible()
      expect((await clean.boundingBox())!.y).toBeLessThan(120)
      await expect(page.getByRole('region', { name: '所有磁盘空间' })).toContainText('启动磁盘')
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
