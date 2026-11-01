import { expect, test, type Page } from '@playwright/test'
import { target } from '../fixtures/uninstall'
async function mount(page: Page, mode = 'usage') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/app-uninstall.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: 'Mole 应用清单' })
}
test('old opening hints exclude running and unknown copies and lead to one separate confirmed uninstall', async ({
  page,
}, testInfo) => {
  const library = await mount(page)
  await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
  await expect(library.getByRole('combobox', { name: '应用使用线索' })).toBeEnabled()
  await library.getByRole('combobox', { name: '应用使用线索' }).selectOption('90')
  await expect(library.getByRole('article')).toHaveCount(2)
  await expect(library).not.toContainText('受管理 App')
  await expect(library.getByRole('article', { name: /记录未知/ })).toHaveCount(0)
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  await page.setViewportSize({ width: 960, height: 780 })
  await page.screenshot({
    path: testInfo.outputPath('application-usage.png'),
    animations: 'disabled',
  })
  await library
    .getByRole('article', { name: `${target.name} · ${target.path}` })
    .getByRole('button', { name: '卸载…', exact: true })
    .click()
  const dialog = page.getByRole('dialog', { name: '确认卸载' })
  await expect(dialog.getByRole('button', { name: '移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await expect(page.getByRole('dialog', { name: '卸载结果' })).toContainText('已移入废纸篓')
  const calls = await page.evaluate(() => (window as any).uninstallFixture.calls)
  expect(calls.filter((c: any) => c.command === 'prepare')[0].args).toEqual({
    taskId: target.taskId,
    index: 0,
  })
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
})
test('unknown hints stay separate, malformed or expired reports do not enable an age filter', async ({
  page,
}) => {
  const library = await mount(page)
  await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
  await library.getByRole('combobox', { name: '应用使用线索' }).selectOption('unknown')
  await expect(library.getByRole('article')).toHaveCount(1)
  await expect(library).toContainText('系统未提供打开记录')
  for (const mode of ['usage-mismatch', 'usage-expired']) {
    await page.reload()
    const library = await mount(page, mode)
    await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
    await expect(library.getByRole('combobox', { name: '应用使用线索' })).toBeDisabled()
    if (mode === 'usage-mismatch') await expect(library.getByRole('alert')).toContainText('不匹配')
    else await expect(library).toContainText('使用线索已过期')
    await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  }
})
test('stopping returns only observed hints and source changes withdraw late results', async ({
  page,
}) => {
  let library = await mount(page, 'usage-cancel')
  await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
  await library.getByRole('button', { name: '停止检查使用线索', exact: true }).click()
  await expect(library).toContainText('本轮使用线索检查已停止')
  await expect(library).toContainText('本轮已检查 1 / 5 项')
  await expect(library).toContainText('未完整覆盖')
  const calls = await page.evaluate(() => (window as any).uninstallFixture.calls)
  expect(calls.find((c: any) => c.command === 'usageCancel').args.taskId).toBe(
    calls.find((c: any) => c.command === 'usage').args.requestId,
  )
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  await page.reload()
  library = await mount(page, 'usage-late')
  await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
  await page.getByRole('button', { name: '模拟应用清单刷新', exact: true }).click()
  await page.evaluate(() => (window as any).uninstallFixture.releaseUsage())
  await expect(library.getByRole('combobox', { name: '应用使用线索' })).toBeDisabled()
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
})
test('usage filters translate without changing literal app identities or submitting an action', async ({
  page,
}) => {
  await mount(page)
  const library = page.locator('#uninstall-fixture .application-library')
  await library.getByRole('button', { name: '检查使用线索', exact: true }).click()
  await library.getByRole('combobox', { name: '应用使用线索' }).selectOption('90')
  for (const [language, label] of [
    ['en', 'Application usage hints'],
    ['ja', 'アプリの使用の手がかり'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await expect(library.getByRole('combobox', { name: label })).toHaveValue('90')
    await expect(library.getByRole('article')).toHaveCount(2)
    await expect(
      library.getByRole('article', { name: `${target.name} · ${target.path}` }),
    ).toBeVisible()
  }
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
})
