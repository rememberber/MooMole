import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { maintenanceReport } from './fixtures/maintenance'

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
  await expect(region).toContainText('可检查 2 项本机设置，其余仅提供说明。执行暂未开放')
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

test('explicit setting inspection exposes protection, preserves prior results on errors and makes no mutation calls', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  await page.addInitScript((fixture) => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = async (command, args) => {
      if (command === 'inspect_maintenance') {
        sessionStorage.setItem(
          'maintenance-calls',
          String(Number(sessionStorage.getItem('maintenance-calls') ?? 0) + 1),
        )
        if (sessionStorage.getItem('maintenance-fail')) throw new Error('fixture 读取未确认')
        return { ...fixture, checkedAt: Date.now() }
      }
      if (
        ![
          'bootstrap',
          'get_task_snapshot',
          'get_history',
          'plugin:event|listen',
          'plugin:event|unlisten',
        ].includes(command) &&
        !(command === 'start_task' && (args as { kind?: string })?.kind === 'status')
      ) {
        sessionStorage.setItem('unexpected-maintenance-command', command)
      }
      return original(command, args)
    }
  }, maintenanceReport())
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('maintenance-calls'))).toBeNull()
  const check = page.getByRole('button', { name: '检查本机设置', exact: true })
  await check.click()
  const region = page.getByRole('region', { name: '维护项目说明' })
  await expect(region).toContainText('2 项已有设置观察，其余尚未检查')
  await expect(region).toContainText('上次结果：两项设置已开启，无须重复设置')
  await expect(region).toContainText('上次结果：已受保护，保持现状')
  const search = page.getByRole('textbox', { name: '搜索维护项目' })
  await search.fill('legacy_overrides_audit')
  await region.locator('.maintenance-item > summary').click()
  await expect(region.getByText('Mole 项目保护：命中维护跳过规则，应保留。')).toBeVisible()
  await expect(region.getByText('未知（未返回值或无法确认）', { exact: true })).toHaveCount(4)
  await region.getByText('文件保护与访问观察', { exact: true }).click()
  await expect(region.getByText('原始键值尚未保存，当前没有精确撤销能力。')).toBeVisible()
  await expect(
    region.getByText('文件访问：有限检查未发现限制，不代表系统偏好服务可以写入。', { exact: true }),
  ).toHaveCount(2)
  await page.setViewportSize({ width: 960, height: 680 })
  for (const label of await region.locator('.maintenance-observed dt').all()) {
    expect(
      await label.evaluate(
        (element) =>
          element.getBoundingClientRect().height /
          Number.parseFloat(getComputedStyle(element).lineHeight),
      ),
    ).toBeLessThanOrEqual(2.1)
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.evaluate(() => sessionStorage.setItem('maintenance-fail', 'yes'))
  await check.click()
  await expect(region.getByRole('alert')).toContainText('本次未更新')
  await expect(region).toContainText('上次结果：已受保护，保持现状')
  expect(await page.evaluate(() => sessionStorage.getItem('maintenance-calls'))).toBe('2')
  expect(
    await page.evaluate(() => sessionStorage.getItem('unexpected-maintenance-command')),
  ).toBeNull()
})

test('path protection and file restrictions remain visible without enabling execution', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  const fixture = maintenanceReport()
  fixture.items[0].paths[0].clientProtection = 'matched'
  fixture.items[0].code = 'protected'
  fixture.items[1].taskProtection = 'notMatched'
  fixture.items[1].paths[0].access = 'restricted'
  fixture.items[1].code = 'restricted'
  await page.addInitScript((fixture) => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = (command, args) =>
      command === 'inspect_maintenance'
        ? Promise.resolve({ ...fixture, checkedAt: Date.now() })
        : original(command, args)
  }, fixture)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  await page.getByRole('button', { name: '检查本机设置', exact: true }).click()
  const region = page.getByRole('region', { name: '维护项目说明' })
  await expect(region).toContainText('上次结果：已受保护，保持现状')
  await expect(region).toContainText('上次结果：发现文件访问限制，保持现状')
  await page.getByRole('textbox', { name: '搜索维护项目' }).fill('legacy_overrides_audit')
  await region.locator('.maintenance-item > summary').click()
  await region.getByText('文件保护与访问观察', { exact: true }).click()
  await expect(
    region.getByText('文件访问：发现限制，不会自动提权或修改权限。', { exact: true }),
  ).toBeVisible()
  await expect(region.getByRole('button')).toHaveCount(0)
})

test('leaving maintenance ignores late observations and does not restart inspection', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  await page.addInitScript((fixture) => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    native.invoke = (command, args) =>
      command === 'inspect_maintenance'
        ? new Promise((resolve) => {
            Object.assign(window, {
              finishMaintenance: () => resolve({ ...fixture, checkedAt: Date.now() }),
            })
          })
        : original(command, args)
  }, maintenanceReport())
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  await page.getByRole('button', { name: '检查本机设置', exact: true }).click()
  await expect(page.getByRole('button', { name: '正在检查…' })).toBeDisabled()
  await page.getByRole('button', { name: '系统状态', exact: true }).click()
  await page.evaluate(() =>
    (window as unknown as { finishMaintenance: () => void }).finishMaintenance(),
  )
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  await expect(page.getByRole('region', { name: '维护项目说明' })).not.toContainText('上次结果')
  await expect(page.getByRole('button', { name: '检查本机设置', exact: true })).toBeEnabled()
})
