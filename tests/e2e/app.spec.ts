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
  await expect(page.getByText('清理执行接口尚未开放', { exact: false })).toBeVisible()
  await expect(page.getByRole('button', { name: '扫描清理预览' })).toBeDisabled()
  expect(errors).toEqual([])
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
