import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function setup(page: Page, mode = 'normal') {
  await installTaskMock(page, 'apps')
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/application-restoration.tsx')).mount(mode),
    mode,
  )
  if (mode === 'resume') return page.locator('#application-restoration-fixture')
  await page
    .locator('#application-restoration-fixture')
    .getByRole('button', { name: '应用更新记录', exact: true })
    .click()
  const history = page.getByRole('dialog', { name: '应用更新记录' })
  if (['legacy', 'unknown', 'resume'].includes(mode)) return history
  await history.getByRole('button', { name: '检查并恢复此前版本…' }).click()
  return page.getByRole('dialog', { name: '恢复此前应用版本' })
}
test('restore a retained older App only after independent review, preserving the newer App and data boundary', async ({
  page,
}) => {
  const modal = await setup(page),
    confirm = modal.getByRole('button', { name: '确认恢复旧版并保留新版' })
  await expect(modal.getByRole('checkbox')).toBeEnabled()
  await expect(confirm).toBeDisabled()
  await expect(modal).toContainText('2.0 → 1.0')
  await expect(modal).toContainText('应用数据不回退')
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({
    path: '.cache/application-restoration-review.png',
    animations: 'disabled',
  })
  await modal.getByRole('checkbox').check()
  await confirm.click()
  await expect(modal.getByRole('region', { name: '应用旧版恢复结果' })).toContainText(
    '旧版已恢复，新版保留',
  )
  await expect(confirm).toHaveCount(0)
  const calls = JSON.parse(
      (await page.locator('#application-restoration-fixture').getAttribute('data-calls'))!,
    ),
    prepare = calls.filter((c: any) => c.command === 'prepare_application_restoration')
  expect(prepare).toHaveLength(1)
  expect(prepare[0].args).toHaveProperty('sourceId')
  expect(prepare[0].args).not.toHaveProperty('path')
  expect(calls.filter((c: any) => c.command === 'execute_application_restoration')).toHaveLength(1)
  await modal.getByRole('button', { name: '返回更新记录' }).click()
  await expect(
    page
      .getByRole('dialog', { name: '应用更新记录' })
      .getByRole('button', { name: '检查并恢复此前版本…' }),
  ).toBeDisabled()
})
for (const mode of ['expired', 'mismatch'])
  test(`${mode} recovery cannot execute`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'expired')
      await expect(modal.getByRole('button', { name: '确认恢复旧版并保留新版' })).toBeDisabled()
    else await expect(modal.getByRole('alert')).toContainText('旧版恢复数据无法核对')
    await expect(page.locator('#application-restoration-fixture')).toHaveAttribute(
      'data-executions',
      '0',
    )
  })
for (const mode of ['checkingCancel', 'cancel'])
  test(`${mode} stops only its owned recovery request`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'cancel') {
      await modal.getByRole('checkbox').check()
      await modal.getByRole('button', { name: '确认恢复旧版并保留新版' }).click()
    }
    await modal.getByRole('button', { name: '停止旧版恢复', exact: true }).click()
    if (mode === 'checkingCancel')
      await expect(modal.getByRole('alert')).toContainText('owned preparation cancelled')
    else
      await expect(modal.getByRole('region', { name: '应用旧版恢复结果' })).toContainText(
        '旧版恢复已取消',
      )
    const calls = JSON.parse(
      (await page.locator('#application-restoration-fixture').getAttribute('data-calls'))!,
    )
    expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
      calls.find(
        (c: any) =>
          c.command ===
          (mode === 'cancel'
            ? 'execute_application_restoration'
            : 'prepare_application_restoration'),
      ).args.requestId,
    )
  })
test('unbound completion never enables automatic recovery retry', async ({ page }) => {
  const modal = await setup(page, 'badResult')
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '确认恢复旧版并保留新版' }).click()
  await expect(modal.getByRole('alert')).toContainText('旧版恢复数据无法核对')
  await expect(modal.getByRole('button', { name: '确认恢复旧版并保留新版' })).toHaveCount(0)
})
test('legacy updates with no original receipts retain locations without gaining recovery permission', async ({
  page,
}) => {
  const history = await setup(page, 'legacy')
  await expect(history.getByRole('button', { name: '检查并恢复此前版本…' })).toHaveCount(0)
  await expect(history.getByRole('button', { name: '显示旧版或暂存目录' })).toBeVisible()
})
test('unknown recovery records require explicit actual-file review and keep their result', async ({
  page,
}) => {
  const history = await setup(page, 'unknown')
  await history
    .getByRole('region', { name: '应用旧版恢复记录' })
    .getByRole('button')
    .filter({ hasText: '旧版恢复结果待核实' })
    .click()
  const check = history.getByRole('button', { name: '记录旧版恢复已核对' })
  await expect(check).toBeDisabled()
  await history.getByRole('checkbox').check()
  await check.click()
  await expect(check).toHaveCount(0)
  await expect(history).toContainText('旧版恢复结果待核实')
  await expect(page.locator('#application-restoration-fixture')).toHaveAttribute(
    'data-executions',
    '0',
  )
})
test('a live restoration reconnects without replay and can stop its exact owner', async ({
  page,
}) => {
  await setup(page, 'resume')
  const host = page.locator('#application-restoration-fixture')
  await host.getByRole('button', { name: '停止旧版恢复', exact: true }).click()
  await expect(host.getByText('旧版应用恢复仍在进行，请在更新记录中核对两份应用。')).toHaveCount(0)
  const calls = JSON.parse((await host.getAttribute('data-calls'))!)
  expect(
    calls.filter((c: any) =>
      ['prepare_application_restoration', 'execute_application_restoration'].includes(c.command),
    ),
  ).toHaveLength(0)
  expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
    '7205853c-c7fd-47a9-8e83-fcebe84336dd',
  )
})
