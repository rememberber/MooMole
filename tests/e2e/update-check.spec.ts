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
    const snapshot = () => ({
      recovering: sessionStorage.getItem('update-recovering') === 'yes',
      recoveryError: sessionStorage.getItem('update-recovery-error'),
      installing: sessionStorage.getItem('update-installing') === 'yes',
      installError: sessionStorage.getItem('update-install-error'),
      check: JSON.parse(sessionStorage.getItem('update-cached-check') ?? 'null'),
      download: JSON.parse(
        sessionStorage.getItem('update-download') ??
          '{"state":"idle","restored":false,"fileName":null,"transferred":0,"total":0,"error":null}',
      ),
    })
    internal.invoke = async (command, args) => {
      if (command === 'save_settings') {
        sessionStorage.setItem('update-save-started', 'yes')
        while (sessionStorage.getItem('update-save-pending'))
          await new Promise((resolve) => setTimeout(resolve, 20))
        return args?.settings
      }
      if (command === 'check_updates') {
        sessionStorage.setItem(
          'update-checks',
          String(Number(sessionStorage.getItem('update-checks') ?? 0) + 1),
        )
        sessionStorage.setItem('update-args', JSON.stringify(args ?? null))
        sessionStorage.removeItem('update-cached-check')
        sessionStorage.removeItem('update-download')
        sessionStorage.removeItem('update-recovery-error')
        while (sessionStorage.getItem('update-mode') === 'pending')
          await new Promise((resolve) => setTimeout(resolve, 20))
        if (sessionStorage.getItem('update-mode') === 'error')
          throw new Error('检查更新失败，请重试。')
        const result = JSON.parse(sessionStorage.getItem('update-result') ?? '{}')
        sessionStorage.setItem('update-cached-check', JSON.stringify(result))
        if (sessionStorage.getItem('update-mode') === 'restore-pending') {
          sessionStorage.setItem('update-recovering', 'yes')
          while (sessionStorage.getItem('update-mode') === 'restore-pending')
            await new Promise((resolve) => setTimeout(resolve, 20))
        }
        if (sessionStorage.getItem('update-mode') === 'restore-ready') {
          sessionStorage.setItem(
            'update-download',
            JSON.stringify({
              state: 'ready',
              restored: true,
              fileName: result.asset.fileName,
              transferred: 1000,
              total: 1000,
              error: null,
            }),
          )
        }
        if (sessionStorage.getItem('update-mode') === 'restore-error')
          sessionStorage.setItem('update-recovery-error', '本地更新包未能复用，请重新下载。')
        sessionStorage.removeItem('update-recovering')
        return snapshot()
      }
      if (command === 'get_update_snapshot') {
        if (sessionStorage.getItem('update-snapshot-error')) throw new Error('state unavailable')
        return snapshot()
      }
      if (command === 'download_update') {
        sessionStorage.setItem('download-args', JSON.stringify(args))
        const check = JSON.parse(sessionStorage.getItem('update-cached-check') ?? 'null')
        const download = {
          state: 'downloading',
          restored: false,
          fileName: check.asset.fileName,
          transferred: 0,
          total: 1000,
          error: null,
        }
        sessionStorage.setItem('update-download', JSON.stringify(download))
        sessionStorage.removeItem('update-recovery-error')
        return snapshot()
      }
      if (command === 'cancel_update_download') {
        sessionStorage.setItem('cancel-download-args', JSON.stringify(args))
        const download = JSON.parse(sessionStorage.getItem('update-download')!)
        download.state = 'cancelling'
        sessionStorage.setItem('update-download', JSON.stringify(download))
        return
      }
      if (command === 'reveal_update_download') {
        sessionStorage.setItem('reveal-download-args', JSON.stringify(args))
        if (sessionStorage.getItem('reveal-download-error')) {
          const download = JSON.parse(sessionStorage.getItem('update-download')!)
          download.state = 'failed'
          download.error = '更新文件已变化，请重新下载。'
          sessionStorage.setItem('update-download', JSON.stringify(download))
          throw new Error('更新文件已变化，请重新下载。')
        }
        return
      }
      if (command === 'install_update') {
        sessionStorage.setItem('install-update-args', JSON.stringify(args))
        sessionStorage.setItem(
          'install-calls',
          String(Number(sessionStorage.getItem('install-calls') ?? 0) + 1),
        )
        sessionStorage.setItem('update-installing', 'yes')
        sessionStorage.removeItem('update-install-error')
        while (sessionStorage.getItem('install-mode') === 'pending')
          await new Promise((resolve) => setTimeout(resolve, 20))
        const mode = sessionStorage.getItem('install-mode')
        if (mode === 'busy' || mode === 'error' || mode === 'changed') {
          const message =
            mode === 'busy'
              ? '还有检查或数据保存正在进行，请等待完成或先取消任务，再安装。'
              : mode === 'changed'
                ? '更新文件已变化，请重新下载。'
                : '无法打开安装包，应用保持运行；请重试。'
          sessionStorage.removeItem('update-installing')
          sessionStorage.setItem('update-install-error', message)
          if (mode === 'changed') {
            const download = JSON.parse(sessionStorage.getItem('update-download')!)
            download.state = 'failed'
            download.error = message
            sessionStorage.setItem('update-download', JSON.stringify(download))
          }
          throw new Error(message)
        }
        sessionStorage.setItem('update-exit-requested', 'yes')
        return
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
  await expect(updates.getByRole('button', { name: '下载更新包', exact: true })).toBeVisible()
  await expect(updates.getByRole('button', { name: '在 Finder 中查看', exact: true })).toHaveCount(
    0,
  )
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

test('reopening observes the backend check without starting another network request', async ({
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
  await expect(updates.getByRole('status')).toContainText('发现新版本 9.0.0')
  expect(await page.evaluate(() => sessionStorage.getItem('update-checks'))).toBe('1')
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

test('download progress survives closing settings, cancellation waits for confirmation, failures retry and only verified downloads reveal', async ({
  page,
}) => {
  const updates = await setup(page)
  await page.evaluate((result) => sessionStorage.setItem('update-result', JSON.stringify(result)), {
    ...base,
    status: 'available',
    latestVersion: '0.2.0',
    asset: { fileName: 'MooMole-0.2.0-mac-arm64.dmg' },
  })
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await updates.getByRole('button', { name: '下载更新包', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在下载更新包')
  await expect(updates.getByRole('button', { name: '检查更新', exact: true })).toBeDisabled()
  await page.evaluate(() => {
    const d = JSON.parse(sessionStorage.getItem('update-download')!)
    d.transferred = 500
    sessionStorage.setItem('update-download', JSON.stringify(d))
  })
  await expect(updates.getByRole('progressbar')).toHaveAttribute('value', '500')
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await updates.scrollIntoViewIfNeeded()
    expect(await updates.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/update-download-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  await expect(updates.getByRole('progressbar')).toHaveAttribute('value', '500')
  expect(await page.evaluate(() => sessionStorage.getItem('download-args'))).toBe('{}')
  await updates.getByRole('button', { name: '取消下载', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在取消并清理临时文件')
  await expect(updates.getByRole('button', { name: '取消下载', exact: true })).toBeDisabled()
  await page.evaluate(() => {
    const d = JSON.parse(sessionStorage.getItem('update-download')!)
    d.state = 'cancelled'
    sessionStorage.setItem('update-download', JSON.stringify(d))
  })
  await expect(updates.getByRole('status')).toContainText('下载已取消')
  await updates.getByRole('button', { name: '下载更新包', exact: true }).click()
  await page.evaluate(() => {
    const d = JSON.parse(sessionStorage.getItem('update-download')!)
    d.transferred = 1000
    d.state = 'verifying'
    sessionStorage.setItem('update-download', JSON.stringify(d))
  })
  await expect(updates.getByRole('status')).toContainText('正在校验已保存')
  await expect(updates.getByRole('button', { name: '在 Finder 中查看', exact: true })).toHaveCount(
    0,
  )
  await page.evaluate(() => {
    const d = JSON.parse(sessionStorage.getItem('update-download')!)
    d.state = 'failed'
    d.error = '更新包大小或 SHA-512 校验失败。'
    sessionStorage.setItem('update-download', JSON.stringify(d))
  })
  await expect(updates.getByRole('alert')).toContainText('SHA-512')
  await updates.getByRole('button', { name: '重新下载', exact: true }).click()
  await page.evaluate(() => {
    const d = JSON.parse(sessionStorage.getItem('update-download')!)
    d.transferred = 1000
    d.state = 'ready'
    sessionStorage.setItem('update-download', JSON.stringify(d))
  })
  await expect(updates.getByRole('status')).toContainText('更新包已下载并校验')
  await page.evaluate(() => sessionStorage.setItem('update-snapshot-error', 'yes'))
  await expect(updates.getByRole('status')).toContainText('更新状态尚未确认')
  await expect(
    updates.getByRole('button', { name: '在 Finder 中查看', exact: true }),
  ).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('update-snapshot-error'))
  await expect(updates.getByRole('status')).toContainText('更新包已下载并校验')
  await updates.getByRole('button', { name: '在 Finder 中查看', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('reveal-download-args'))).toBe('{}')
  await page.evaluate(() => sessionStorage.setItem('reveal-download-error', 'yes'))
  await updates.getByRole('button', { name: '在 Finder 中查看', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('下载未完成')
  await expect(updates.getByRole('button', { name: '在 Finder 中查看', exact: true })).toHaveCount(
    0,
  )
  expect(await page.evaluate(() => sessionStorage.getItem('cancel-download-args'))).toBe('{}')
})

async function readyUpdate(page: Page) {
  const updates = await setup(page)
  await page.evaluate(
    (result) => {
      sessionStorage.setItem('update-cached-check', JSON.stringify(result))
      sessionStorage.setItem(
        'update-download',
        JSON.stringify({
          state: 'ready',
          fileName: 'MooMole-0.2.0-mac-arm64.dmg',
          transferred: 1000,
          total: 1000,
          error: null,
        }),
      )
    },
    {
      ...base,
      status: 'available',
      latestVersion: '0.2.0',
      asset: { fileName: 'MooMole-0.2.0-mac-arm64.dmg' },
    },
  )
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toBeVisible()
  return updates
}

test('install requires explicit open-and-quit confirmation, retains ready package on active work or open failure and retries', async ({
  page,
}) => {
  const updates = await readyUpdate(page)
  await updates.getByRole('button', { name: '打开 DMG 安装', exact: true }).click()
  await expect(updates.getByRole('group', { name: '确认安装更新' })).toContainText('退出 MooMole')
  expect(await page.evaluate(() => sessionStorage.getItem('install-calls'))).toBeNull()
  await updates.getByRole('button', { name: '暂不安装', exact: true }).click()
  await expect(updates.getByRole('group', { name: '确认安装更新' })).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('install-mode', 'busy'))
  await updates.getByRole('button', { name: '打开 DMG 安装', exact: true }).click()
  await updates.getByRole('button', { name: '打开并退出', exact: true }).click()
  await expect(updates.getByRole('alert')).toContainText('还有检查或数据保存')
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toBeEnabled()
  expect(await page.evaluate(() => sessionStorage.getItem('update-exit-requested'))).toBeNull()
  await page.evaluate(() => sessionStorage.setItem('install-mode', 'error'))
  await updates.getByRole('button', { name: '打开 DMG 安装', exact: true }).click()
  await updates.getByRole('button', { name: '打开并退出', exact: true }).click()
  await expect(updates.getByRole('alert')).toContainText('应用保持运行')
  await expect(updates.getByRole('status')).toContainText('更新包已下载并校验')
  await expect(updates.getByRole('button', { name: '在 Finder 中查看', exact: true })).toBeEnabled()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  await expect(updates.getByRole('alert')).toContainText('应用保持运行')
  await page.evaluate(() => sessionStorage.removeItem('install-mode'))
  await updates.getByRole('button', { name: '打开 DMG 安装', exact: true }).click()
  await updates.getByRole('button', { name: '打开并退出', exact: true }).click()
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('update-exit-requested')))
    .toBe('yes')
  expect(await page.evaluate(() => sessionStorage.getItem('install-update-args'))).toBe('{}')
})

test('installation preparation survives closing settings, blocks duplicate actions and revokes a changed package', async ({
  page,
}) => {
  const updates = await readyUpdate(page)
  await page.setViewportSize({ width: 960, height: 780 })
  await updates.getByRole('button', { name: '打开 DMG 安装', exact: true }).click()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await updates.scrollIntoViewIfNeeded()
    expect(await updates.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/update-install-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('install-mode', 'pending'))
  await updates.getByRole('button', { name: '打开并退出', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在复核并打开安装包')
  await expect(updates.getByRole('button', { name: '正在准备安装…', exact: true })).toBeDisabled()
  await expect(updates.getByRole('button', { name: '检查更新', exact: true })).toBeDisabled()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  await expect(updates.getByRole('status')).toContainText('正在复核并打开安装包')
  await expect(
    updates.getByRole('button', { name: '在 Finder 中查看', exact: true }),
  ).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('install-calls'))).toBe('1')
  await page.evaluate(() => sessionStorage.setItem('install-mode', 'changed'))
  await expect(updates.getByRole('status')).toContainText('下载未完成')
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toHaveCount(0)
  await expect(updates.getByRole('button', { name: '重新下载', exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('update-exit-requested'))).toBeNull()
})

test('pending settings save prevents confirming installation until the save completes', async ({
  page,
}) => {
  const updates = await readyUpdate(page)
  const settings = page.getByRole('dialog', { name: '设置', exact: true })
  await settings.getByRole('button', { name: '通用', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('update-save-pending', 'yes'))
  await settings.getByLabel('主题', { exact: true }).selectOption('dark')
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('update-save-started')))
    .toBe('yes')
  await settings.getByRole('button', { name: '关于', exact: true }).click()
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toBeDisabled()
  await expect(settings.getByText('正在保存…', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('install-calls'))).toBeNull()
  await page.evaluate(() => sessionStorage.removeItem('update-save-pending'))
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toBeEnabled()
})

test('fresh manual check revalidates a saved package without downloading, survives closing and reports restored readiness', async ({
  page,
}) => {
  const updates = await setup(page)
  await page.evaluate(
    (result) => {
      sessionStorage.setItem('update-result', JSON.stringify(result))
      sessionStorage.setItem('update-mode', 'restore-pending')
    },
    {
      ...base,
      status: 'available',
      latestVersion: '0.2.0',
      asset: { fileName: 'MooMole-0.2.0-mac-arm64.dmg' },
    },
  )
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在复核本地更新包')
  await expect(updates.getByRole('button', { name: '检查更新', exact: true })).toBeDisabled()
  await expect(updates.getByRole('progressbar')).toHaveCount(0)
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page
    .getByRole('dialog', { name: '设置', exact: true })
    .getByRole('button', { name: '关于', exact: true })
    .click()
  await expect(updates.getByRole('status')).toContainText('正在复核本地更新包')
  await expect(updates.getByRole('button', { name: '下载更新包', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('update-checks'))).toBe('1')
  await page.evaluate(() => sessionStorage.setItem('update-snapshot-error', 'yes'))
  await expect(updates.getByRole('status')).toContainText('更新状态尚未确认')
  await expect(updates.getByRole('button', { name: '下载更新包', exact: true })).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('update-snapshot-error'))
  await expect(updates.getByRole('status')).toContainText('正在复核本地更新包')
  await page.evaluate(() => sessionStorage.setItem('update-mode', 'restore-ready'))
  await expect(updates.getByRole('status')).toContainText('已找到并校验本地更新包')
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toBeEnabled()
  await expect(updates.getByRole('button', { name: '在 Finder 中查看', exact: true })).toBeEnabled()
  await expect(updates.getByRole('button', { name: '下载更新包', exact: true })).toHaveCount(0)
  expect(await page.evaluate(() => sessionStorage.getItem('download-args'))).toBeNull()
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await updates.scrollIntoViewIfNeeded()
    expect(await updates.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/update-recovery-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('local recovery failure keeps the fresh release result and starting a new download clears the warning', async ({
  page,
}) => {
  const updates = await setup(page)
  await page.evaluate(
    (result) => {
      sessionStorage.setItem('update-result', JSON.stringify(result))
      sessionStorage.setItem('update-mode', 'restore-error')
    },
    {
      ...base,
      status: 'available',
      latestVersion: '0.2.0',
      asset: { fileName: 'MooMole-0.2.0-mac-arm64.dmg' },
    },
  )
  await updates.getByRole('button', { name: '检查更新', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('发现新版本 0.2.0')
  await expect(updates.getByRole('alert')).toHaveText('本地更新包未能复用，请重新下载。')
  await expect(updates.getByRole('button', { name: '下载更新包', exact: true })).toBeEnabled()
  await expect(updates.getByRole('button', { name: '打开 DMG 安装', exact: true })).toHaveCount(0)
  await updates.getByRole('button', { name: '下载更新包', exact: true }).click()
  await expect(updates.getByRole('status')).toContainText('正在下载更新包')
  await expect(updates.getByRole('alert')).toHaveCount(0)
})
