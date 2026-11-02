import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function setup(page: Page, mode = 'normal') {
  await installTaskMock(page, 'apps')
  await page.goto('/')
  await page.evaluate(
    async (m) => (await import('/tests/e2e/fixtures/retained-application.tsx')).mount(m),
    mode,
  )
  const fixture = page.locator('#retained-application-fixture')
  if (mode === 'resume') return fixture
  await fixture.getByRole('button', { name: '应用更新记录', exact: true }).click()
  const history = page.getByRole('dialog', { name: '应用更新记录' })
  if (['unknown', 'legacy'].includes(mode)) return history
  await history.getByRole('button', { name: '检查保留版本与占用…' }).click()
  return page.getByRole('dialog', { name: '整理保留的应用版本' })
}
for (const mode of ['normal', 'restored'])
  test(`${mode}: independent retained App review and confirmation, no automatic cleanup`, async ({
    page,
  }, info) => {
    const modal = await setup(page, mode),
      confirm = modal.getByRole('button', { name: '确认将保留版本移入废纸篓' })
    await expect(modal.getByRole('checkbox')).toBeEnabled()
    await expect(confirm).toBeDisabled()
    await expect(modal).toContainText(
      mode === 'restored' ? '这里保留的是当时的新版包' : '更新后保留的旧版包',
    )
    await expect(modal).toContainText('不会立即释放磁盘空间')
    await expect(page.locator('#retained-application-fixture')).toHaveAttribute(
      'data-executions',
      '0',
    )
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
    await page.screenshot({ path: info.outputPath(`retained-${mode}.png`), animations: 'disabled' })
    await modal.getByRole('checkbox').check()
    await confirm.click()
    await expect(modal.getByRole('region', { name: '保留版本整理结果' })).toContainText(
      '保留版本已移入废纸篓',
    )
    await expect(confirm).toHaveCount(0)
    const calls = JSON.parse(
      (await page.locator('#retained-application-fixture').getAttribute('data-calls'))!,
    )
    expect(calls.filter((c: any) => c.command === 'prepare_retained_application')).toHaveLength(1)
    expect(
      calls.find((c: any) => c.command === 'prepare_retained_application').args,
    ).not.toHaveProperty('path')
    const executions = calls.filter((c: any) => c.command === 'execute_retained_application')
    expect(executions).toHaveLength(1)
    expect(Object.keys(executions[0].args).sort()).toEqual(['confirmed', 'id', 'requestId'])
    await modal.getByRole('button', { name: '返回更新记录' }).click()
    const history = page.getByRole('dialog', { name: '应用更新记录' })
    await expect(history.getByRole('button', { name: '检查保留版本与占用…' })).toHaveCount(0)
    await expect(history.getByRole('button', { name: '检查并恢复此前版本…' })).toHaveCount(0)
    await history
      .getByRole('region', { name: '保留版本整理记录' })
      .getByRole('button', { name: /Owned App/ })
      .click()
    await expect(history.getByRole('region', { name: '保留版本整理结果' })).toContainText(
      '空的专用保留目录已移除',
    )
  })
for (const mode of ['expired', 'mismatch'])
  test(`${mode} cannot execute retained App cleanup`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'expired')
      await expect(modal.getByRole('button', { name: '确认将保留版本移入废纸篓' })).toBeDisabled()
    else await expect(modal.getByRole('alert')).toContainText('保留版本整理数据无法核对')
    await expect(page.locator('#retained-application-fixture')).toHaveAttribute(
      'data-executions',
      '0',
    )
  })
for (const mode of ['checkingCancel', 'cancel'])
  test(`${mode} stops only the owned preparation or execution`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'cancel') {
      await modal.getByRole('checkbox').check()
      await modal.getByRole('button', { name: '确认将保留版本移入废纸篓' }).click()
    }
    await modal.getByRole('button', { name: '停止保留版本整理', exact: true }).click()
    if (mode === 'checkingCancel')
      await expect(modal.getByRole('alert')).toContainText('owned preparation cancelled')
    else
      await expect(modal.getByRole('region', { name: '保留版本整理结果' })).toContainText(
        '保留版本整理已取消',
      )
    const calls = JSON.parse(
      (await page.locator('#retained-application-fixture').getAttribute('data-calls'))!,
    )
    expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
      calls.find(
        (c: any) =>
          c.command ===
          (mode === 'cancel' ? 'execute_retained_application' : 'prepare_retained_application'),
      ).args.requestId,
    )
  })
test('mismatched completion leaves a durable-review path without retrying', async ({ page }) => {
  const modal = await setup(page, 'badResult')
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '确认将保留版本移入废纸篓' }).click()
  await expect(modal.getByRole('alert')).toContainText('保留版本整理数据无法核对')
  await expect(modal.getByRole('button', { name: '确认将保留版本移入废纸篓' })).toHaveCount(0)
})
test('legacy receipts cannot gain retained-copy authority', async ({ page }) => {
  const history = await setup(page, 'legacy')
  await expect(history.getByRole('button', { name: '检查保留版本与占用…' })).toHaveCount(0)
  await expect(history.getByRole('button', { name: '显示旧版或暂存目录' })).toBeVisible()
})
test('unknown result needs independent actual-location review and remains unavailable', async ({
  page,
}) => {
  const history = await setup(page, 'unknown')
  const region = history.getByRole('region', { name: '保留版本整理记录' })
  await region.getByRole('button', { name: /Owned App/ }).click()
  await expect(region).toContainText('专用保留目录的移除未获确认，请检查实际状态。')
  const ack = region.getByRole('button', { name: '记录已人工核对' })
  await expect(ack).toBeDisabled()
  await region.getByRole('checkbox').check()
  await ack.click()
  await expect(ack).toHaveCount(0)
  await expect(region).toContainText('保留版本整理结果待核实')
  await expect(history.getByRole('button', { name: '检查保留版本与占用…' })).toHaveCount(0)
})
test('reconnects an exact existing request without another cleanup', async ({ page }) => {
  const host = await setup(page, 'resume')
  await expect(host).toContainText('fixture busy')
  await host.getByRole('button', { name: '停止保留版本整理', exact: true }).click()
  await expect(host).toContainText('fixture idle')
  await expect(host).toHaveAttribute('data-executions', '0')
})
for (const language of ['en', 'ja'])
  test(`${language} retained version review preserves exact paths and consent`, async ({
    page,
  }, info) => {
    await setup(page, 'restored')
    await page.evaluate(
      async (language) =>
        (await import('/src/i18n/locale.ts')).setLanguagePreference(language as 'en' | 'ja'),
      language,
    )
    const modal = page.getByRole('dialog', {
      name:
        language === 'en'
          ? 'Clean up a retained application version'
          : '保持中のアプリバージョンを整理',
      exact: true,
    })
    await expect(modal).toContainText('/Applications/Owned.app')
    await expect(modal.getByRole('checkbox')).not.toBeChecked()
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`retained-${language}.png`),
      animations: 'disabled',
    })
    await expect(page.locator('#retained-application-fixture')).toHaveAttribute(
      'data-executions',
      '0',
    )
  })
