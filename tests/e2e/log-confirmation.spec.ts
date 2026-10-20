import { expect, test } from '@playwright/test'

async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/log-confirmation.tsx')).mount(mode),
    mode,
  )
}

test('fixed scope requires explicit acknowledgement; records a single result', async ({ page }) => {
  await page.setViewportSize({ width: 960, height: 800 })
  await mount(page)
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
  const dialog = page.getByRole('dialog', { name: '确认移入废纸篓' })
  await expect(page.getByLabel('调用次数')).toHaveText('0')
  await expect(dialog.locator('article')).toHaveCount(1)
  const action = dialog.getByRole('button', { name: '移入废纸篓', exact: true })
  await expect(action).toBeDisabled()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/log-confirmation-${theme}.png`,
      animations: 'disabled',
    })
  }
  await dialog.getByRole('checkbox').check()
  await action.click()
  await expect(page.getByRole('dialog', { name: '日志处理结果' })).toBeVisible()
  await expect(page.getByLabel('调用次数')).toHaveText('1')
  await expect(page.getByText('开始前取消', { exact: true })).toBeVisible()
})

test('expired and malformed confirmations never execute', async ({ page }) => {
  await page.clock.install()
  await mount(page)
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
  await page.getByRole('checkbox').check()
  await page.clock.fastForward(31000)
  await expect(page.getByRole('button', { name: '移入废纸篓', exact: true })).toBeDisabled()
  await expect(page.getByLabel('调用次数')).toHaveText('0')
  await mount(page, 'invalid')
  await expect(page.getByRole('button', { name: '重新复查后生成确认范围' })).toBeDisabled()
})

test('cancel preserves completed evidence after confirmation expires', async ({ page }) => {
  await page.clock.install()
  await mount(page, 'cancel')
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await page.clock.fastForward(31000)
  await page.getByRole('button', { name: '停止后续项目' }).click()
  await expect(page.getByRole('dialog', { name: '日志处理结果' })).toBeVisible()
  await expect(page.getByLabel('调用次数')).toHaveText('1')
  await expect(page.getByText('已移入 1 项 · 其余 1 项未确认移入')).toBeVisible()
})

test('failure consumes confirmation without retry and return revokes it', async ({ page }) => {
  await mount(page, 'error')
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('核对实际结果')
  await expect(page.getByRole('button', { name: '移入废纸篓', exact: true })).toBeDisabled()
  await expect(page.getByLabel('调用次数')).toHaveText('1')
  await page.getByRole('button', { name: '返回审阅' }).click()
  await expect(page.getByLabel('撤销次数')).toHaveText('1')
  await expect(page.getByRole('button', { name: '重新复查后生成确认范围' })).toBeDisabled()
})

test('completed result can locate its actual receipt without repeating execution', async ({
  page,
}) => {
  await mount(page)
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const result = page.getByRole('dialog', { name: '日志处理结果' })
  await result.getByText('实际移入位置', { exact: true }).click()
  await result.getByRole('button', { name: '在 Finder 中查看' }).click()
  await expect(page.getByLabel('定位收据')).toHaveText(
    '6d048c7d-d222-410c-875d-a6e3cef18003/item-0',
  )
  await expect(page.getByLabel('调用次数')).toHaveText('1')
  await expect(result).toContainText('当前没有应用内恢复')
})
