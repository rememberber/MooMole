import { expect, test, type Page } from '@playwright/test'

const settings = (page: Page) => page.getByRole('button', { name: '设置', exact: true })
const settingsDialog = (page: Page) => page.getByRole('dialog', { name: '设置', exact: true })
const source = (url: URL) => url.pathname === '/src/components/SettingsModal.tsx'

async function fixture(page: Page) {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/modal-focus.tsx')).mount())
  return page.getByRole('region', { name: '弹窗焦点测试' })
}

test('settings returns to its actual pointer trigger even when clicking does not focus the button', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  const path = page.getByRole('textbox', { name: '扫描目录', exact: true })
  await path.focus()
  // Safari/macOS pointer clicks need not focus a button. Preserve the input
  // focus on mousedown so a previous-active-element-only solution cannot pass.
  await settings(page).evaluate((button) =>
    button.addEventListener('mousedown', (event) => event.preventDefault()),
  )
  await settings(page).click()
  await expect(settingsDialog(page).getByRole('button', { name: '关闭弹窗' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(settings(page)).toBeFocused()
  await expect(path).not.toBeFocused()
})

test('settings shortcut returns to the prior field and keeps the original trigger across tab changes', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  const path = page.getByRole('textbox', { name: '扫描目录', exact: true })
  await path.fill('/fixture/不运行检查')
  await page.keyboard.press('Meta+Comma')
  await settingsDialog(page).getByRole('button', { name: '关于', exact: true }).click()
  await page.keyboard.press('Meta+Comma')
  await expect(
    settingsDialog(page).getByRole('heading', { name: '关于 MooMole', exact: true }),
  ).toBeVisible()
  await settingsDialog(page).getByRole('button', { name: '关闭弹窗' }).click()
  await expect(path).toBeFocused()
  await expect(path).toHaveValue('/fixture/不运行检查')
})

test('the protection-list entry receives focus again when settings closes', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: '清理', exact: true }).click()
  const opener = page.getByRole('button', { name: '管理保护列表', exact: true })
  await opener.click()
  await expect(
    settingsDialog(page).getByRole('heading', { name: '保护列表', exact: true }),
  ).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(opener).toBeFocused()
})

test('canceling pending settings restores focus and a late module cannot reopen it', async ({
  page,
}) => {
  let release!: () => void
  const waiting = new Promise<void>((resolve) => {
    release = resolve
  })
  await page.route(source, async (route) => {
    await waiting
    await route.continue()
  })
  await page.goto('/')
  await settings(page).click()
  await page.getByRole('button', { name: '取消打开', exact: true }).click()
  await expect(settings(page)).toBeFocused()
  const response = page.waitForResponse((response) => source(new URL(response.url())))
  release()
  await response
  await expect(settingsDialog(page)).toHaveCount(0)
  await expect(settings(page)).toBeFocused()
})

test('closing a failed settings module restores focus without retrying the loader', async ({
  page,
}) => {
  let requests = 0
  await page.route(source, async (route) => {
    requests++
    await route.abort()
  })
  await page.goto('/')
  await settings(page).click()
  await expect(page.getByRole('alert')).toContainText('这部分界面暂时无法打开')
  await page.getByRole('button', { name: '关闭', exact: true }).click()
  await expect(settings(page)).toBeFocused()
  expect(requests).toBe(1)
})

test('nested dialogs restore the inner invoker first and switching dialogs does not steal focus', async ({
  page,
}) => {
  const scope = await fixture(page)
  const opener = scope.getByRole('button', { name: '打开测试弹窗', exact: true })
  await opener.focus()
  await page.keyboard.press('Enter')
  const parent = page.getByRole('dialog', { name: '测试弹窗', exact: true })
  const innerOpener = parent.getByRole('button', { name: '打开内层弹窗', exact: true })
  await innerOpener.focus()
  await page.keyboard.press('Enter')
  const inner = page.getByRole('dialog', { name: '内层弹窗', exact: true })
  await expect(inner.getByRole('button', { name: '关闭弹窗' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(innerOpener).toBeFocused()
  await parent.getByRole('button', { name: '切换到另一弹窗' }).click()
  const next = page.getByRole('dialog', { name: '另一弹窗', exact: true })
  await expect(next.getByRole('button', { name: '关闭弹窗' })).toBeFocused()
  await expect(opener).not.toBeFocused()
})

for (const state of ['hidden', 'disabled', 'missing']) {
  test(`an ${state} trigger falls back to a live entry instead of restoring unusable focus`, async ({
    page,
  }) => {
    const scope = await fixture(page)
    await scope.getByRole('button', { name: '打开测试弹窗', exact: true }).click()
    await page
      .getByRole('dialog', { name: '测试弹窗', exact: true })
      .getByRole('button', { name: `入口设为 ${state}`, exact: true })
      .click()
    await page.keyboard.press('Escape')
    await expect(scope.getByRole('button', { name: '备用入口', exact: true })).toBeFocused()
  })
}

test('a required notice still blocks Escape and backdrop dismissal', async ({ page }) => {
  const scope = await fixture(page)
  const opener = scope.getByRole('button', { name: '打开必读说明', exact: true })
  await opener.focus()
  await page.keyboard.press('Enter')
  const notice = page.getByRole('dialog', { name: '必读说明', exact: true })
  await page.keyboard.press('Escape')
  await expect(notice).toBeVisible()
  await page.mouse.click(5, 5)
  await expect(notice).toBeVisible()
  await expect(notice.getByRole('button', { name: '关闭弹窗' })).toHaveCount(0)
  await notice.getByRole('button', { name: '已阅读说明' }).click()
  await expect(opener).toBeFocused()
})

test('execution details return to the row after success or read failure, then fall back when the row disappears', async ({
  page,
}) => {
  const scope = await fixture(page)
  const records = scope.getByRole('region', { name: '执行记录', exact: true })
  const opener = records.getByRole('button').filter({ hasText: '查看逐项结果' })
  await opener.evaluate((button) =>
    button.addEventListener('mousedown', (event) => event.preventDefault()),
  )
  for (const fail of [false, true]) {
    await page.evaluate(
      (fail) =>
        fail
          ? sessionStorage.setItem('focus-detail-fail', 'yes')
          : sessionStorage.removeItem('focus-detail-fail'),
      fail,
    )
    await opener.click()
    const detail = page.getByRole('dialog', { name: '逐项执行结果', exact: true })
    if (fail) await expect(detail.getByRole('alert')).toContainText('fixture read failed')
    else await expect(detail).toContainText('已取消')
    await page.keyboard.press('Escape')
    await expect(opener).toBeFocused()
  }
  await opener.click()
  // Background record updates can remove the invoker while a detail stays open.
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/modal-focus.tsx')).removeExecutionRecords(),
  )
  await page.keyboard.press('Escape')
  await expect(records.getByRole('button').filter({ hasText: '查看逐项结果' })).toHaveCount(0)
  await expect(records.getByRole('button', { name: '刷新记录', exact: true })).toBeFocused()
})
