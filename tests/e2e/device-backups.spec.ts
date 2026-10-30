import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function open(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/device-backups.tsx')).mount(mode),
    mode,
  )
  await page.getByRole('button', { name: '设备备份…', exact: true }).last().click()
  return page.getByRole('dialog', { name: '设备备份', exact: true })
}
async function calls(page: Page) {
  return JSON.parse((await page.locator('#device-backups-fixture').getAttribute('data-calls'))!)
}
test('whole-backup flow requires explicit scan, exact item and separate acknowledgement', async ({
  page,
}) => {
  const d = await open(page)
  expect((await calls(page)).every((c: any) => c.command === 'get_device_backups')).toBe(true)
  await d.getByRole('button', { name: '扫描本机备份', exact: true }).click()
  const rows = d.getByRole('region', { name: '设备备份列表' }).locator('article')
  await expect(rows).toHaveCount(2)
  await expect(rows.nth(1).getByRole('button')).toBeDisabled()
  await expect(d.getByRole('status')).toContainText('未完整核对')
  await rows.nth(0).getByRole('button').click()
  await expect(d).toContainText('/Users/demo/BackupCopies/owned-backup')
  await expect(d.getByRole('button', { name: '确认整份移入废纸篓', exact: true })).toBeDisabled()
  expect((await calls(page)).some((c: any) => c.command === 'execute_device_backup')).toBe(false)
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await d.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/device-backups-${theme}.png`,
      animations: 'disabled',
    })
  }
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认整份移入废纸篓', exact: true }).click()
  await expect(d.getByRole('region', { name: '设备备份处理历史' })).toContainText(
    '整份备份已移入废纸篓',
  )
  await d.getByText('整份备份已移入废纸篓', { exact: false }).first().click()
  await expect(d).toContainText('/Users/demo/.Trash/owned-backup')
  const submitted = (await calls(page)).filter((c: any) => c.command === 'execute_device_backup')
  expect(submitted).toHaveLength(1)
  expect(submitted[0].args).not.toHaveProperty('path')
})
test('unknown backup history is observed and acknowledgement never resends a request', async ({
  page,
}) => {
  const d = await open(page, 'existing')
  await d.locator('summary').click()
  await d.getByRole('button', { name: '核对中断结果…' }).click()
  await expect(d.getByRole('button', { name: '记录已核对', exact: true })).toBeDisabled()
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '记录已核对', exact: true }).click()
  await expect(d).toContainText('原未知结果保持')
  expect((await calls(page)).some((c: any) => /scan_device|execute_device/.test(c.command))).toBe(
    false,
  )
})
test('cancel targets only the live backup request and does not claim it moved', async ({
  page,
}) => {
  const d = await open(page, 'pending')
  await d.getByRole('button', { name: '扫描本机备份', exact: true }).click()
  await d.getByRole('button', { name: '审阅整份处理…' }).first().click()
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认整份移入废纸篓', exact: true }).click()
  await d.getByRole('button', { name: '停止当前备份任务', exact: true }).click()
  await expect(d).toContainText('处理已取消')
  const all = await calls(page)
  expect(all.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
    all.find((c: any) => c.command === 'execute_device_backup').args.requestId,
  )
})
test('a substituted confirmation path is rejected without execution', async ({ page }) => {
  const d = await open(page, 'mismatch')
  await d.getByRole('button', { name: '扫描本机备份', exact: true }).click()
  await d.getByRole('button', { name: '审阅整份处理…' }).first().click()
  await expect(d.getByRole('alert')).toContainText('设备备份数据无法核对')
  expect((await calls(page)).some((c: any) => c.command === 'execute_device_backup')).toBe(false)
})
test('an existing live task can be observed and stopped without another scan or execution', async ({
  page,
}) => {
  const d = await open(page, 'resumed')
  await expect(d.getByRole('button', { name: '扫描本机备份', exact: true })).toBeDisabled()
  await d.getByRole('button', { name: '停止当前备份任务', exact: true }).click()
  await expect(d).toContainText('处理已取消')
  const all = await calls(page)
  expect(all.some((c: any) => /scan_device|execute_device/.test(c.command))).toBe(false)
  expect(all.filter((c: any) => c.command === 'cancel_task')).toHaveLength(1)
})
test('the main analysis toolbar exposes backups without starting a scan', async ({ page }) => {
  await installTaskMock(page)
  await page.goto('/')
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByRole('button', { name: '设备备份…', exact: true })).toBeVisible()
})
