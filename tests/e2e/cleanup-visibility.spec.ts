import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

async function setup(page: Page) {
  await installTaskMock(page, 'new')
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const previous = internal.invoke
    const idle = {
      version: 1,
      readOnly: true,
      requestId: null,
      state: 'idle',
      startedAt: null,
      finishedAt: null,
      observation: null,
    }
    const snapshot = () =>
      JSON.parse(sessionStorage.getItem('visibility-snapshot') ?? JSON.stringify(idle))
    internal.invoke = async (command, args) => {
      if (command === 'get_cleanup_visibility') {
        const result = snapshot()
        if (sessionStorage.getItem('visibility-read-error')) throw new Error('读取状态失败')
        return result
      }
      if (command === 'start_cleanup_visibility') {
        sessionStorage.setItem(
          'visibility-starts',
          String(Number(sessionStorage.getItem('visibility-starts') ?? 0) + 1),
        )
        sessionStorage.setItem('visibility-start-args', JSON.stringify(args ?? null))
        const result = {
          ...idle,
          state: 'running',
          requestId: '8a2d1589-dda4-4fb5-b22f-150724c26fbb',
          startedAt: Date.now(),
        }
        sessionStorage.setItem('visibility-snapshot', JSON.stringify(result))
        if (sessionStorage.getItem('visibility-lost-response'))
          throw new Error('响应未确认，请读取当前状态')
        return result
      }
      if (command === 'cancel_cleanup_visibility') {
        sessionStorage.setItem('visibility-cancel-args', JSON.stringify(args))
        const result = { ...snapshot(), state: 'cancelling' }
        sessionStorage.setItem('visibility-snapshot', JSON.stringify(result))
        return result
      }
      return previous(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  const settings = page.getByRole('dialog', { name: '设置', exact: true })
  await settings.getByRole('button', { name: '关于', exact: true }).click()
  await settings.getByText('清理权限检查', { exact: true }).click()
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeEnabled()
  return settings
}

async function finish(page: Page, observation: string) {
  await page.evaluate((observation) => {
    const snapshot = JSON.parse(sessionStorage.getItem('visibility-snapshot')!)
    sessionStorage.setItem(
      'visibility-snapshot',
      JSON.stringify({
        ...snapshot,
        state: 'finished',
        observation,
        finishedAt: Date.now(),
        diagnostic: '脚本状态：0；固定探针标记：有。',
      }),
    )
  }, observation)
}

test('opening the diagnostic does not authorize; explicit start is fixed and result is not cleanup permission', async ({
  page,
}) => {
  const settings = await setup(page)
  await expect(settings.getByText('尚未检查系统进程可见性。')).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('visibility-starts'))).toBeNull()
  await settings.getByRole('button', { name: '授权检查', exact: true }).click()
  await expect(settings.getByText('等待系统确认或读取检查结果…')).toBeVisible()
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('visibility-start-args'))).toBe('{}')
  await finish(page, 'completeViewObserved')
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('仍须对所选对象逐项复核')
  await expect(settings.getByText('检查时间：', { exact: false })).toBeVisible()
  await expect(
    settings.getByText('脚本状态：0；固定探针标记：有。', { exact: true }),
  ).not.toBeVisible()
  await settings.getByText('检查细节', { exact: true }).click()
  await expect(settings.getByText('脚本状态：0；固定探针标记：有。', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('visibility-starts'))).toBe('1')
})

test('closing and reloading observes the same running request without reauthorizing', async ({
  page,
}) => {
  let settings = await setup(page)
  await settings.getByRole('button', { name: '授权检查', exact: true }).click()
  await page.keyboard.press('Escape')
  await page.reload()
  await page.getByRole('button', { name: '设置', exact: true }).click()
  settings = page.getByRole('dialog', { name: '设置', exact: true })
  await settings.getByRole('button', { name: '关于', exact: true }).click()
  await settings.getByText('清理权限检查', { exact: true }).click()
  await expect(settings.getByText('等待系统确认或读取检查结果…')).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('visibility-starts'))).toBe('1')
  await finish(page, 'incompleteOrFailed')
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('仍应保留')
})

test('stop binds the exact request and keeps admission disabled until terminal cancellation', async ({
  page,
}) => {
  const settings = await setup(page)
  await settings.getByRole('button', { name: '授权检查', exact: true }).click()
  await settings.getByRole('button', { name: '停止等待', exact: true }).click()
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('若系统授权窗口仍在')
  await expect(settings.getByRole('button', { name: '停止等待', exact: true })).toBeDisabled()
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeDisabled()
  expect(
    JSON.parse((await page.evaluate(() => sessionStorage.getItem('visibility-cancel-args')))!)
      .requestId,
  ).toBe('8a2d1589-dda4-4fb5-b22f-150724c26fbb')
  await finish(page, 'cancelled')
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('本次检查已取消')
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeEnabled()
})

test('lost start response is read back without replaying and read errors withdraw prior proof', async ({
  page,
}) => {
  const settings = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('visibility-lost-response', 'yes'))
  await settings.getByRole('button', { name: '授权检查', exact: true }).click()
  await expect(
    settings
      .getByRole('group', { name: '清理权限检查', exact: true })
      .getByRole('alert')
      .filter({ hasText: '响应未确认' }),
  ).toBeVisible()
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('等待系统确认')
  expect(await page.evaluate(() => sessionStorage.getItem('visibility-starts'))).toBe('1')
  await page.evaluate(() => sessionStorage.setItem('visibility-read-error', 'yes'))
  await expect(
    settings
      .getByRole('group', { name: '清理权限检查', exact: true })
      .getByRole('alert')
      .filter({ hasText: '读取状态失败' }),
  ).toBeVisible()
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeDisabled()
  await finish(page, 'completeViewObserved')
  await page.evaluate(() => sessionStorage.removeItem('visibility-read-error'))
  await settings.getByRole('button', { name: '重新读取', exact: true }).click()
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('本次已读到')
})

test('malformed snapshots remain unavailable and narrow light/dark layouts stay contained', async ({
  page,
}) => {
  const settings = await setup(page)
  await settings.getByRole('button', { name: '授权检查', exact: true }).click()
  await page.evaluate(() => {
    const snapshot = JSON.parse(sessionStorage.getItem('visibility-snapshot')!)
    sessionStorage.setItem(
      'visibility-snapshot',
      JSON.stringify({
        ...snapshot,
        state: 'finished',
        finishedAt: Date.now(),
        observation: 'completeViewObserved',
        readOnly: false,
      }),
    )
  })
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('alert'),
  ).toContainText('状态无法确认')
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).toContainText('状态暂时无法确认')
  await expect(settings.getByRole('button', { name: '授权检查', exact: true })).toBeDisabled()
  await expect(
    settings.getByRole('group', { name: '清理权限检查', exact: true }).getByRole('status'),
  ).not.toContainText('已读到')
  await page.setViewportSize({ width: 960, height: 680 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await expect(settings.getByRole('button', { name: '重新读取', exact: true })).toBeVisible()
    expect(
      await settings.evaluate((element) => element.scrollWidth <= element.clientWidth + 1),
    ).toBe(true)
    await settings.screenshot({
      path: `test-results/cleanup-visibility-${theme}.png`,
      animations: 'disabled',
    })
  }
})
