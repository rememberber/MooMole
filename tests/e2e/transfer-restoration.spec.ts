import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function setup(page: Page, mode = 'normal') {
  await installTaskMock(page, 'history')
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/transfer-restoration.tsx')).mount(mode),
    mode,
  )
  const history = page
    .locator('#transfer-restoration-fixture')
    .getByRole('region', { name: '跨磁盘移动记录' })
  await history.getByRole('button', { name: /所选文件已完成跨磁盘移动/ }).click()
  await page
    .getByRole('dialog', { name: '跨磁盘移动记录' })
    .getByRole('button', { name: '从副本恢复原位置…' })
    .click()
  return page.getByRole('dialog', { name: '从跨磁盘副本恢复' })
}
test('a retained transfer restores only after independent explicit confirmation and preserves both-copy boundaries', async ({
  page,
}) => {
  const modal = await setup(page)
  await expect(modal.getByRole('checkbox')).toBeEnabled()
  await expect(modal).toContainText('/Volumes/OwnedRestore/one (1).txt')
  await expect(modal).toContainText('/Users/demo/Downloads/one.txt')
  await expect(modal).toContainText('原废纸篓项目不改动')
  const confirm = modal.getByRole('button', { name: '确认恢复并保留副本' })
  await expect(confirm).toBeDisabled()
  await expect(page.locator('#transfer-restoration-fixture')).toHaveAttribute(
    'data-executions',
    '0',
  )
  await modal.getByRole('checkbox').check()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await modal.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({
    path: 'test-results/transfer-restoration-review.png',
    animations: 'disabled',
  })
  await confirm.click()
  await expect(modal.getByRole('region', { name: '跨磁盘恢复结果' })).toContainText(
    '已恢复到原位置，副本保留',
  )
  await expect(confirm).toHaveCount(0)
  const calls = JSON.parse(
    (await page.locator('#transfer-restoration-fixture').getAttribute('data-calls'))!,
  )
  const prepare = calls.filter((c: any) => c.command === 'prepare_transfer_restoration')
  expect(prepare).toHaveLength(1)
  expect(prepare[0].args).toMatchObject({
    runId: '00000002-1234-4123-8123-123456789abc',
    itemId: '00000003-1234-4123-8123-123456789abc',
  })
  expect(prepare[0].args).not.toHaveProperty('copyPath')
  expect(calls.filter((c: any) => c.command === 'execute_transfer_restoration')).toHaveLength(1)
  await modal.getByRole('button', { name: '查看保留副本' }).click()
  await expect(page.locator('#transfer-restoration-fixture')).toHaveAttribute(
    'data-reveal',
    '/Volumes/OwnedRestore/one (1).txt',
  )
})
for (const mode of ['expired', 'mismatch'])
  test(`${mode} restoration cannot execute`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'expired') {
      await expect(modal).toContainText('恢复确认已过期')
      await expect(modal.getByRole('button', { name: '确认恢复并保留副本' })).toBeDisabled()
    } else {
      await expect(modal.getByRole('alert')).toContainText('跨磁盘恢复数据无法核对')
      await expect(modal.getByRole('checkbox')).toHaveCount(0)
    }
    await expect(page.locator('#transfer-restoration-fixture')).toHaveAttribute(
      'data-executions',
      '0',
    )
  })
for (const mode of ['checkingCancel', 'cancel'])
  test(`${mode} only stops its own restoration request`, async ({ page }) => {
    const modal = await setup(page, mode)
    if (mode === 'cancel') {
      await modal.getByRole('checkbox').check()
      await modal.getByRole('button', { name: '确认恢复并保留副本' }).click()
    }
    await modal.getByRole('button', { name: '停止本次恢复', exact: true }).click()
    if (mode === 'checkingCancel')
      await expect(modal.getByRole('alert')).toContainText('owned prepare cancelled')
    else {
      await expect(modal.getByRole('region', { name: '跨磁盘恢复结果' })).toContainText(
        '恢复已取消',
      )
      await expect(modal.getByRole('button', { name: '确认恢复并保留副本' })).toHaveCount(0)
    }
    const calls = JSON.parse(
      (await page.locator('#transfer-restoration-fixture').getAttribute('data-calls'))!,
    )
    const own = calls.find(
      (c: any) =>
        c.command ===
        (mode === 'cancel' ? 'execute_transfer_restoration' : 'prepare_transfer_restoration'),
    )
    expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(own.args.requestId)
  })
test('a mismatched result never offers automatic retry', async ({ page }) => {
  const modal = await setup(page, 'badResult')
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '确认恢复并保留副本' }).click()
  await expect(modal.getByRole('alert')).toContainText('跨磁盘恢复数据无法核对')
  await expect(modal.getByRole('button', { name: '确认恢复并保留副本' })).toHaveCount(0)
  await expect(page.locator('#transfer-restoration-fixture')).toHaveAttribute(
    'data-executions',
    '1',
  )
})
test('unknown restoration history requires explicit review and never replays a restore', async ({
  page,
}) => {
  await installTaskMock(page, 'history')
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/transfer-restoration.tsx')).mount('unknown'),
  )
  const region = page
    .locator('#transfer-restoration-fixture')
    .getByRole('region', { name: '跨磁盘恢复记录' })
  await region.getByRole('button', { name: /跨磁盘恢复结果待核实/ }).click()
  const modal = page.getByRole('dialog', { name: '跨磁盘恢复记录' })
  await expect(modal.getByRole('button', { name: '记录恢复已核对' })).toBeDisabled()
  await modal.getByRole('checkbox').check()
  await modal.getByRole('button', { name: '记录恢复已核对' }).click()
  await expect(modal.getByRole('checkbox')).toHaveCount(0)
  await expect(modal).toContainText('跨磁盘恢复结果待核实')
  await expect(page.locator('#transfer-restoration-fixture')).toHaveAttribute(
    'data-executions',
    '0',
  )
})
test('a running recovery is reattached after mount without issuing prepare or execute', async ({
  page,
}) => {
  await installTaskMock(page, 'history')
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/transfer-restoration.tsx')).mount('resume'),
  )
  const host = page.locator('#transfer-restoration-fixture')
  await expect(
    host.getByText('跨磁盘恢复仍在进行，原副本保持。请在恢复记录中核对结果。'),
  ).toBeVisible()
  await host.getByRole('button', { name: '停止后续恢复' }).click()
  await expect(
    host.getByText('跨磁盘恢复仍在进行，原副本保持。请在恢复记录中核对结果。'),
  ).toHaveCount(0)
  const calls = JSON.parse((await host.getAttribute('data-calls'))!)
  expect(
    calls.filter(
      (c: any) =>
        c.command === 'prepare_transfer_restoration' ||
        c.command === 'execute_transfer_restoration',
    ),
  ).toHaveLength(0)
})
