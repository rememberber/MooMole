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
    internal.invoke = async (command, args) => {
      if (command === 'check_updates') {
        sessionStorage.setItem(
          'update-checks',
          String(Number(sessionStorage.getItem('update-checks') ?? 0) + 1),
        )
        sessionStorage.setItem('update-args', JSON.stringify(args ?? null))
        while (sessionStorage.getItem('update-mode') === 'pending')
          await new Promise((resolve) => setTimeout(resolve, 20))
        if (sessionStorage.getItem('update-mode') === 'error')
          throw new Error('检查更新失败，请重试。')
        return JSON.parse(sessionStorage.getItem('update-result') ?? '{}')
      }
      if (command === 'open_update_releases') {
        sessionStorage.setItem('update-release-args', JSON.stringify(args ?? null))
        if (sessionStorage.getItem('update-open-error'))
          throw new Error('无法打开发布页，请稍后重试。')
        return
      }
      return previous(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  return page.getByRole('region', { name: 'MooMole 更新' })
}

const base = {
  status: 'unpublished',
  currentVersion: '0.1.0',
  latestVersion: null,
  channel: 'stable',
  architecture: 'arm64',
  notes: '',
  notesTruncated: false,
  asset: null,
}

test('checks only on request, prevents duplicate clicks and distinguishes unpublished from current', async ({
  page,
}) => {
  const updates = await setup(page)
  expect(await page.evaluate(() => sessionStorage.getItem('update-checks'))).toBeNull()
  await page.evaluate((result) => {
    sessionStorage.setItem('update-result', JSON.stringify(result))
    sessionStorage.setItem('update-mode', 'pending')
  }, base)
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在检查更新…')
  await expect(updates.getByRole('button', { name: '检查更新', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('update-checks'))).toBe('1')
  await page.evaluate(() => sessionStorage.removeItem('update-mode'))
  await expect(updates.getByRole('status')).toContainText('尚未发布可检查的版本')
  await expect(updates).not.toContainText('当前版本无需更新')
  await page.evaluate((result) => sessionStorage.setItem('update-result', JSON.stringify(result)), {
    ...base,
    status: 'current',
    latestVersion: '0.1.0',
  })
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('当前版本无需更新')
  expect(await page.evaluate(() => sessionStorage.getItem('update-args'))).toBe('{}')
})

test('offers matching package release notes as plain text, clears stale results and retries failures', async ({
  page,
}) => {
  const updates = await setup(page)
  await page.evaluate((result) => sessionStorage.setItem('update-result', JSON.stringify(result)), {
    ...base,
    status: 'available',
    latestVersion: '0.2.0',
    channel: 'preview',
    notes:
      '<script>window.untrustedUpdate = true</script>\n新功能与已知问题\n' + '说明'.repeat(3000),
    notesTruncated: true,
    asset: { fileName: 'MooMole-0.2.0-mac-arm64.dmg' },
  })
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('发现新版本 0.2.0')
  await expect(updates.getByRole('status')).toContainText('测试版通道')
  await expect(updates).toContainText('应用内下载尚未开放')
  await expect(updates.getByRole('button', { name: /下载|安装/ })).toHaveCount(0)
  await updates.getByText('更新内容', { exact: true }).click()
  await expect(updates.locator('pre')).toContainText(
    '<script>window.untrustedUpdate = true</script>',
  )
  expect(
    await page.evaluate(() => (window as unknown as Record<string, unknown>).untrustedUpdate),
  ).toBeUndefined()
  await expect(updates).toContainText('这里只展示部分更新内容')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await updates.scrollIntoViewIfNeeded()
    expect(await updates.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/update-check-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('update-mode', 'error'))
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('alert')).toHaveText('检查更新失败，请重试。')
  await expect(updates).not.toContainText('发现新版本')
  await page.evaluate(
    (result) => {
      sessionStorage.removeItem('update-mode')
      sessionStorage.setItem('update-result', JSON.stringify(result))
    },
    { ...base, status: 'available', latestVersion: '0.3.0' },
  )
  await updates.getByRole('button', { name: '重试检查', exact: true }).click()
  await expect(updates).toContainText('新版尚未提供当前架构的安装包')
  await expect(updates.getByRole('alert')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('update-open-error', 'yes'))
  await updates.getByRole('button', { name: '查看发布页', exact: true }).click()
  await expect(updates.getByRole('alert')).toContainText('无法打开发布页')
  await page.evaluate(() => sessionStorage.removeItem('update-open-error'))
  await updates.getByRole('button', { name: '查看发布页', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('update-release-args'))).toBe('{}')
  await expect(updates.getByRole('alert')).toHaveCount(0)
})

test('closing a pending check does not inject its result into a reopened panel', async ({
  page,
}) => {
  const updates = await setup(page)
  await page.evaluate(
    (result) => {
      sessionStorage.setItem('update-result', JSON.stringify(result))
      sessionStorage.setItem('update-mode', 'pending')
    },
    { ...base, status: 'available', latestVersion: '9.0.0' },
  )
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await page.keyboard.press('Escape')
  await expect(updates).toHaveCount(0)
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  await page.evaluate(() => sessionStorage.removeItem('update-mode'))
  await expect(updates.getByRole('status')).toContainText('手动检查 MooMole 客户端更新')
  await expect(updates).not.toContainText('9.0.0')
})

test('browser preview never checks the network', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  const updates = page.getByRole('region', { name: 'MooMole 更新' })
  await expect(updates.getByRole('button', { name: '检查更新', exact: true })).toBeDisabled()
  await expect(updates.getByRole('status')).toContainText('请在桌面 App 中检查更新')
})
