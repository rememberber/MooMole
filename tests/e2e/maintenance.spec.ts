import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

test('maintenance browsing and keyboard expansion never dispatch checks, authorization or mutations', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(() => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = async (command, args) => {
      const commands = JSON.parse(sessionStorage.getItem('catalog-commands') ?? '[]')
      commands.push(command)
      sessionStorage.setItem('catalog-commands', JSON.stringify(commands))
      return original(command, args)
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  const region = page.getByRole('region', { name: '维护项目说明' })
  await expect(region).toContainText('19 / 19 项')
  await expect(region).toContainText('尚未检查本机，执行暂未开放')
  await page.evaluate(() => sessionStorage.setItem('catalog-commands', '[]'))
  const search = page.getByRole('textbox', { name: '搜索维护项目' })
  await search.fill('DNS')
  await expect(region.locator('.maintenance-item')).toHaveCount(2)
  const dns = region
    .locator('.maintenance-item')
    .filter({ hasText: '系统 DNS 缓存与 mDNSResponder' })
  await dns.locator(':scope > summary').focus()
  await page.keyboard.press('Enter')
  await expect(dns.getByText('需要管理员授权。')).toBeVisible()
  await expect(dns.getByText(/本机适用性、有效保护规则与权限尚未检查/)).toBeVisible()
  await search.fill('文件系统验证')
  await region.locator('.maintenance-item > summary').click()
  await expect(region).toContainText('上游默认禁用')
  await expect(region.getByText(/无法用进程信号中断的内核 I\/O/)).toBeVisible()
  await search.fill('[missing]')
  await expect(region.getByText('没有匹配的维护项目')).toBeVisible()
  await search.fill('')
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('catalog-commands') ?? '[]')),
  ).toEqual([])
})

test('changed or unavailable engine retains only reference information', async ({ page }) => {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(() => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = async (command, args) => {
      const result = await original(command, args)
      return command === 'bootstrap'
        ? {
            ...(result as object),
            engine: {
              available: false,
              version: '2.0.0',
              revision: 'other',
              reason: 'fixture mismatch',
            },
          }
        : result
    }
  })
  await page.goto('/')
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  const region = page.getByRole('region', { name: '维护项目说明' })
  await expect(region.getByRole('alert')).toContainText('当前引擎与说明基线不匹配')
  await expect(region).toContainText('不能据此判断本机是否适用')
  await expect(region.locator('.maintenance-item')).toHaveCount(19)
})
