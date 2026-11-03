import { installTaskMock } from './fixtures/native-task'
import { expect, test, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/storage-notifications.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: '低空间系统提醒' })
}
const calls = (page: Page) =>
  page
    .locator('#storage-notifications-fixture')
    .getAttribute('data-calls')
    .then((v) => JSON.parse(v ?? '[]') as { command: string; value?: boolean }[])
test('opening only reads; requesting permission and opting in are independent and tests require separate consent', async ({
  page,
}) => {
  const region = await mount(page)
  await expect(region).toContainText('低空间系统提醒已关闭')
  await expect(region.getByRole('button', { name: '开启低空间提醒' })).toBeDisabled()
  expect((await calls(page)).every((c) => c.command === 'read')).toBe(true)
  await region.getByRole('button', { name: '申请系统通知权限' }).dblclick()
  await expect(region).toContainText('系统已允许通知')
  await expect(region).toContainText('低空间系统提醒已关闭')
  await region.getByRole('button', { name: '开启低空间提醒' }).click()
  await expect(region).toContainText('定期空间观察当前关闭')
  await region.getByText('测试提醒与最近记录', { exact: true }).click()
  const send = region.getByRole('button', { name: '发送一条测试提醒' })
  await expect(send).toBeDisabled()
  await region.getByRole('checkbox').check()
  await send.dblclick()
  await expect(region).toContainText('已提交给 macOS，不代表已显示')
  await expect(send).toBeDisabled()
  const log = await calls(page)
  expect(log.filter((c) => c.command === 'request')).toHaveLength(1)
  expect(log.filter((c) => c.command === 'test')).toHaveLength(1)
  expect(log.filter((c) => c.command === 'configure')).toEqual([
    { command: 'configure', value: true },
  ])
  await region.getByRole('button', { name: '关闭低空间提醒' }).click()
  await expect(region).toContainText('低空间系统提醒已关闭')
})
for (const mode of ['denied', 'future', 'timeout', 'mismatch'])
  test(`${mode} never silently enables alerts or resubmits`, async ({ page }) => {
    const r = await mount(page, mode)
    if (mode === 'future') {
      await expect(r.getByRole('alert')).toContainText('其他版本')
      await expect(r.getByRole('button', { name: '开启低空间提醒' })).toBeDisabled()
    } else if (mode === 'denied') {
      await expect(r.getByRole('button', { name: '申请系统通知权限' })).toBeDisabled()
      await expect(r.getByRole('button', { name: '开启低空间提醒' })).toBeDisabled()
    } else {
      await r.getByRole('button', { name: '申请系统通知权限' }).click()
      if (mode === 'timeout') await expect(r.getByRole('alert')).toContainText('授权尚未取得结果')
      else {
        await r.getByRole('button', { name: '开启低空间提醒' }).click()
        await expect(r.getByRole('alert')).toContainText('未取得可靠结果')
      }
      await expect(r).toContainText('低空间系统提醒已关闭')
    }
    expect((await calls(page)).filter((c) => c.command === 'test')).toHaveLength(0)
  })
test('unknown submission consumes test consent, survives reload and is never automatically sent again', async ({
  page,
}) => {
  let r = await mount(page, 'unknown')
  await r.getByRole('button', { name: '申请系统通知权限' }).click()
  await r.getByText('测试提醒与最近记录', { exact: true }).click()
  await r.getByRole('checkbox').check()
  await r.getByRole('button', { name: '发送一条测试提醒' }).click()
  await expect(r.getByRole('alert')).toContainText('可能已经提交')
  await expect(r.getByRole('checkbox')).not.toBeChecked()
  await r.getByRole('button', { name: '刷新提醒状态' }).click()
  await expect(r).toContainText('提交结果未知，不会自动重试')
  r = await mount(page, 'unknown')
  await r.getByText('测试提醒与最近记录', { exact: true }).click()
  await expect(r).toContainText('提交结果未知，不会自动重试')
  expect((await calls(page)).every((c) => c.command === 'read')).toBe(true)
})
for (const lang of ['en', 'ja'] as const)
  test(`${lang} compact reminder controls remain readable`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: 960, height: 780 })
    await mount(page)
    await page.evaluate(
      async (lang) => (await import('/src/i18n/locale.ts')).setLanguagePreference(lang),
      lang,
    )
    const host = page.locator('#storage-notifications-fixture')
    expect(await host.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
    await expect(host).toContainText(
      lang === 'en' ? 'Low-space system reminders' : '空き容量のシステム通知',
    )
    await host.screenshot({ path: testInfo.outputPath(`storage-notifications-${lang}.png`) })
  })

test('ongoing management exposes the real reminder control without requesting permission on entry', async ({
  page,
}) => {
  await installTaskMock(page, 'history')
  await page.addInitScript(() => {
    const api = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = api.invoke
    api.invoke = async (command, args) => {
      if (command === 'get_storage_notifications')
        return {
          enabled: false,
          monitoringEnabled: false,
          permission: { authorization: 'notDetermined', alerts: false, center: true },
          error: null,
          records: [],
        }
      if (command === 'get_storage_monitor')
        return {
          config: { enabled: false, intervalMinutes: 60, warnBelowBytes: 10 * 1024 ** 3 },
          collecting: false,
          error: null,
          samples: [],
        }
      if (command === 'get_menu_bar')
        return { config: { enabled: false }, visible: false, error: null }
      return original(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '持续管理', exact: true }).click()
  const region = page.getByRole('region', { name: '低空间系统提醒' })
  await expect(region).toContainText('低空间系统提醒已关闭')
  await expect(region.getByRole('button', { name: '申请系统通知权限' })).toBeEnabled()
  const login = page.getByRole('region', { name: 'MooMole 登录启动', exact: true })
  await expect(login).toBeVisible()
  await expect(login.getByRole('button', { name: '开启登录启动', exact: true })).toBeDisabled()
  const commands = await page.evaluate(
    () =>
      JSON.parse(sessionStorage.getItem('mock-native-commands') ?? '[]') as { command: string }[],
  )
  expect(
    commands.filter((c) =>
      /request_storage_notification|configure_storage_notifications|test_storage_notification|set_main_app_login|acknowledge_main_app_login/.test(
        c.command,
      ),
    ),
  ).toHaveLength(0)
})
