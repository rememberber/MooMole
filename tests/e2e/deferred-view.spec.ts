import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function fixture(page: Page) {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/deferred-view.tsx')).mount())
  return page.getByRole('region', { name: '延迟界面测试' })
}
test('a rejected view retries a fresh loader while parent state and refs remain usable', async ({
  page,
}) => {
  const region = await fixture(page)
  await expect(region.getByRole('status')).toContainText('正在打开…')
  await region.getByRole('button', { name: '更新父状态' }).click()
  await region.getByRole('button', { name: '拒绝加载' }).click()
  await expect(region.getByRole('alert')).toContainText('这部分界面暂时无法打开')
  await expect(region).toContainText('父状态 1')
  await region.getByRole('button', { name: '重试打开' }).click()
  await expect(region.getByRole('status')).toContainText('正在打开…')
  expect(
    await page.evaluate(async () =>
      (await import('/tests/e2e/fixtures/deferred-view.tsx')).loadCount(),
    ),
  ).toBe(2)
  await region.getByRole('button', { name: '完成加载' }).click()
  await expect(region).toContainText('子界面结果 1')
  await region.getByRole('button', { name: '读取子界面状态' }).click()
  await expect(region).toContainText('引用结果 1')
  await region.getByRole('button', { name: '切换子界面' }).click()
  await region.getByRole('button', { name: '更新父状态' }).click()
  await region.getByRole('button', { name: '切换子界面' }).click()
  await expect(region).toContainText('子界面结果 2')
  await expect(region.getByRole('status')).toHaveCount(0)
  expect(
    await page.evaluate(async () =>
      (await import('/tests/e2e/fixtures/deferred-view.tsx')).loadCount(),
    ),
  ).toBe(2)
})
test('an unseen view loads only when opened, and returning to it never starts an installer scan', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  const requests: string[] = []
  page.on('request', (request) => requests.push(new URL(request.url()).pathname))
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  const starts = await page.evaluate(() => sessionStorage.getItem('mock-starts'))
  const installer = '/src/components/Installers.tsx'
  expect(requests.filter((path) => path === installer)).toHaveLength(0)
  expect(requests.filter((path) => path === '/src/components/SettingsModal.tsx')).toHaveLength(0)
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(page.getByRole('heading', { name: '选择要查找安装包的目录' })).toBeVisible()
  expect(requests.filter((path) => path === installer)).toHaveLength(1)
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(page.getByRole('heading', { name: '选择要查找安装包的目录' })).toBeVisible()
  expect(requests.filter((path) => path === installer)).toHaveLength(1)
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('mock-starts'))).toBe(starts)
})

test('a pending or failed dialog can close without resetting parent state', async ({ page }) => {
  const region = await fixture(page)
  await region.getByRole('button', { name: '更新父状态' }).click()
  await region.getByRole('button', { name: '取消打开' }).click()
  await expect(region.getByRole('status')).toHaveCount(0)
  await expect(region).toContainText('父状态 1')
  await region.getByRole('button', { name: '切换子界面' }).click()
  await region.getByRole('button', { name: '拒绝加载' }).click()
  await expect(region.getByRole('alert')).toBeVisible()
  await region.getByRole('button', { name: '关闭', exact: true }).click()
  await expect(region.getByRole('alert')).toHaveCount(0)
  await expect(region).toContainText('父状态 1')
  await region.getByRole('button', { name: '完成加载' }).click()
  await expect(region).not.toContainText('子界面结果')
  expect(
    await page.evaluate(async () =>
      (await import('/tests/e2e/fixtures/deferred-view.tsx')).loadCount(),
    ),
  ).toBe(1)
})
