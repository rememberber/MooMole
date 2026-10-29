import { expect, test, type Page } from '@playwright/test'
import { installSoftwareCleanupMock } from './fixtures/software-cleanup-native'

test('app software scan skips home analysis and routes exact IDs through review, final confirmation and result', async ({
  page,
}) => {
  await installSoftwareCleanupMock(page)
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '软件清理', exact: true })
    .click()
  const rescan = page.getByRole('button', { name: '重新扫描', exact: true })
  await expect(rescan).toBeEnabled()
  await rescan.click()
  await expect(page.getByText('软件清理扫描完成。', { exact: false })).toBeVisible()
  const main = page.getByRole('main', { name: '软件清理内容' })
  await page.setViewportSize({ width: 960, height: 680 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await main.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/software-cleanup-app-${theme}.png`,
      animations: 'disabled',
    })
  }
  await main
    .getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/com.google.Chrome' })
    .check()
  await main.getByRole('button', { name: '清理所选', exact: true }).click()
  const confirmation = page.getByRole('dialog', { name: '确认移入废纸篓' })
  await expect(confirmation).toBeVisible()
  let calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('software-ipc')!))
  expect(
    calls.filter((call: { command: string }) => call.command === 'execute_log_cleanup'),
  ).toHaveLength(0)
  expect(
    calls
      .filter((call: { command: string }) => call.command === 'start_task')
      .map((call: { args: { kind: string } }) => call.args.kind),
  ).toEqual(['status', 'clean', 'apps'])
  expect(
    calls.find((call: { command: string }) => call.command === 'create_review_plan').args,
  ).toEqual({ scanId: 'scan', selectedIds: ['chrome-cache'] })
  await confirmation.getByRole('button', { name: '确认清理', exact: true }).click()
  await expect(page.getByRole('dialog', { name: '清理结果' })).toContainText('已移入 1 项')
  calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('software-ipc')!))
  expect(
    calls.filter((call: { command: string }) => call.command === 'execute_log_cleanup'),
  ).toHaveLength(1)
})

async function mount(page: Page) {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/quick-cleanup.tsx')).mount('software'),
  )
  return page.locator('#quick-fixture')
}

test('software candidates start unselected and preserve exact selections across category and search changes', async ({
  page,
}) => {
  const view = await mount(page)
  const action = view.getByRole('button', { name: '清理所选', exact: true })
  await expect(action).toBeDisabled()
  await expect(view.getByText('所有软件候选默认不选。', { exact: false })).toBeVisible()
  await view.getByRole('combobox', { name: '软件类型' }).selectOption('browser')
  await expect(view.getByText('Google Chrome', { exact: true })).toBeVisible()
  await expect(view.getByText('微信', { exact: true })).toHaveCount(0)
  await expect(
    view.getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/com.microsoft.edgemac' }),
  ).toBeDisabled()
  await view
    .getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/com.google.Chrome' })
    .check()
  await view.getByRole('combobox').selectOption('chat')
  await view
    .getByRole('checkbox', { name: '选择 /Users/demo/Library/Caches/com.tencent.xinWeChat' })
    .check()
  await view.getByRole('searchbox').fill('no-match')
  await expect(view.getByText('没有匹配的软件缓存候选', { exact: false })).toBeVisible()
  await expect(view.getByText('已选 2 项', { exact: false })).toBeVisible()
  await action.click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('quick-plan')!))).toEqual({
    scanId: 'scan',
    ids: ['chrome', 'wechat'],
  })
})

test('software name search, literal installed names and protecting selections keep real boundaries', async ({
  page,
}) => {
  const view = await mount(page)
  await view.getByRole('searchbox').fill('Large App')
  await expect(view.getByText('Large App', { exact: true })).toBeVisible()
  await expect(view.getByText('Google Chrome', { exact: true })).toHaveCount(0)
  await view.getByRole('searchbox').fill('Chrome')
  const checkbox = view.getByRole('checkbox', {
    name: '选择 /Users/demo/Library/Caches/com.google.Chrome',
  })
  await checkbox.check()
  await checkbox.locator('..').getByRole('button', { name: '保护', exact: true }).click()
  await expect(checkbox).toBeDisabled()
  await expect(checkbox).not.toBeChecked()
  await expect(view.getByRole('button', { name: '清理所选', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('quick-plan'))).toBeNull()
})

test('expiry withdraws selection and does not submit a stale software plan', async ({ page }) => {
  await page.clock.install()
  const view = await mount(page)
  const checkbox = view.getByRole('checkbox', {
    name: '选择 /Users/demo/Library/Caches/com.google.Chrome',
  })
  await checkbox.check()
  await page.clock.fastForward(11_000)
  await expect(checkbox).toBeDisabled()
  await expect(checkbox).not.toBeChecked()
  await expect(view.getByRole('button', { name: '清理所选', exact: true })).toBeDisabled()
})

test('software cleanup is navigable in the app and fits minimum-size dark and light windows', async ({
  page,
}) => {
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '软件清理', exact: true })
    .click()
  await expect(page.getByRole('main', { name: '软件清理内容' })).toBeVisible()
  await expect(page.getByText('逐项确认', { exact: true })).toBeVisible()
  const view = await mount(page)
  await page.setViewportSize({ width: 960, height: 680 })
  for (const theme of ['dark', 'light']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await view.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/software-cleanup-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('English and Japanese software categories and translated product search retain original paths', async ({
  page,
}) => {
  const view = await mount(page)
  for (const [language, search, title, category] of [
    ['en', 'WeChat', 'WeChat', 'Software category'],
    ['ja', 'WeChat', 'WeChat', 'ソフトウェアの種類'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await view.getByRole('combobox', { name: category }).selectOption('chat')
    await view.getByRole('searchbox').fill(search)
    await expect(view.getByText(title, { exact: true })).toBeVisible()
    await expect(
      view.getByText('~/Library/Caches/com.tencent.xinWeChat', { exact: true }),
    ).toBeVisible()
  }
})
