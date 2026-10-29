import { test, expect } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'

test('duplicate groups retain a copy, confirm exact IDs and finish in shared history', async ({
  page,
}) => {
  await installPersonalMock(page, false, true)
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '重复文件', exact: true })
    .click()
  const main = page.getByRole('main', { name: '重复文件内容' })
  await page.getByRole('button', { name: '查找重复文件', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.locator('article')).toHaveCount(3)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await expect(main.getByRole('button', { name: '移动到目录…', exact: true })).toHaveCount(0)
  await list
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/one.txt', exact: true })
    .check()
  await list
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/two.txt', exact: true })
    .check()
  await expect(
    list.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/keep.txt', exact: true }),
  ).toBeDisabled()
  await main.getByRole('searchbox', { name: '搜索个人文件' }).fill('one.txt')
  await expect(list.locator('article')).toHaveCount(3)
  await main.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理重复文件' })
  await expect(dialog).toContainText('这批保留的副本')
  await expect(dialog.getByText('/Users/demo/Downloads/keep.txt', { exact: true })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toContainText(
    '已移入 2 / 2 项',
  )
  await main.getByRole('button', { name: '查看处理历史' }).click()
  await page
    .getByRole('region', { name: '执行记录', exact: true })
    .getByRole('button')
    .filter({ hasText: '全部移入废纸篓' })
    .click()
  await expect(page.getByRole('dialog', { name: '逐项执行结果' })).toContainText('来源：重复文件')
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.find((c: { command: string }) => c.command === 'prepare_duplicate_trash').args
      .selectedIds,
  ).toEqual(['00000002-1234-4123-8123-123456789abc', '00000003-1234-4123-8123-123456789abc'])
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_duplicate_trash'),
  ).toHaveLength(1)
})
