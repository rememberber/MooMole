import { expect, test } from '@playwright/test'
import { installRecoveryMock } from './fixtures/execution-recovery'

test('recovered uncertainty is visible on startup and leads to focused execution records', async ({
  page,
}) => {
  await installRecoveryMock(page, 'uncertain')
  await page.goto('/')
  const notice = page.getByRole('region', { name: '清理中断与待核实结果' })
  await expect(notice.getByRole('heading')).toHaveText('有 1 项清理结果待核实')
  await expect(notice).toContainText('涉及 2 个批次，已记录移入废纸篓 3 项')
  await expect(notice).toContainText('未开始的项目不会自动继续')
  await page.reload()
  await expect(notice).toBeVisible()
  await notice.getByRole('button', { name: '前往执行记录' }).click()
  const records = page.getByRole('region', { name: '执行记录', exact: true })
  await expect(records).toBeFocused()
  await expect(records).toBeInViewport()
  await records.getByRole('button').filter({ hasText: '待核实 1 项' }).click()
  const detail = page.getByRole('dialog', { name: '逐项执行结果' })
  await expect(detail).toContainText('存在只有意图、没有可靠完成记录的项目')
  await expect(detail.getByRole('button', { name: /恢复|重试执行|继续清理/ })).toHaveCount(0)
  for (const name of ['卸载记录', '维护记录']) {
    const section = page.getByRole('region', { name, exact: true })
    await expect(section).toBeVisible()
    await expect(section.getByRole('alert')).toHaveCount(0)
  }
  const calls: string[] = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('recovery-calls') ?? '[]'),
  )
  expect(calls).toContain('get_execution')
  const readOnlyCommands = new Set([
    'bootstrap',
    'list_executions',
    'get_execution',
    'list_application_uninstalls',
    'list_quicklook_refreshes',
  ])
  expect(calls.filter((command) => !readOnlyCommands.has(command))).toEqual([])
  await page.keyboard.press('Escape')
  await page.screenshot({ path: 'test-results/execution-recovery.png', fullPage: true })
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(records).not.toBeFocused()
  await notice.getByRole('button', { name: '前往执行记录' }).click()
  await expect(records).toBeFocused()
})

test('an interrupted batch with committed item results is distinct from uncertainty', async ({
  page,
}) => {
  await installRecoveryMock(page, 'interrupted')
  await page.goto('/')
  const notice = page.getByRole('region', { name: '清理中断与待核实结果' })
  await expect(notice.getByRole('heading')).toHaveText('有 1 个清理批次中断')
  await expect(notice).toContainText('已记录移入废纸篓 2 项')
  await expect(notice).toContainText('批次中断不会撤销已完成的操作')
  await expect(notice).not.toContainText('结果待核实')
})

test('completed and explicitly cancelled batches do not produce an interruption notice', async ({
  page,
}) => {
  await installRecoveryMock(page, 'terminal')
  await page.goto('/')
  await expect(page.getByRole('main', { name: '概览内容' })).toBeVisible()
  await expect(page.getByRole('region', { name: '清理中断与待核实结果' })).toHaveCount(0)
})

test('refresh failures preserve recovery evidence and a successful refresh updates the notice', async ({
  page,
}) => {
  await installRecoveryMock(page, 'uncertain')
  await page.goto('/')
  await page.getByRole('region', { name: '清理中断与待核实结果' }).getByRole('button').click()
  const records = page.getByRole('region', { name: '执行记录', exact: true })
  await page.evaluate(() => sessionStorage.setItem('recovery-list-error', 'yes'))
  await records.getByRole('button', { name: '刷新记录', exact: true }).click()
  await expect(records.getByRole('alert')).toContainText('fixture 执行记录暂不可读')
  await expect(page.getByRole('region', { name: '清理中断与待核实结果' })).toContainText(
    '有 1 项清理结果待核实',
  )
  await page.evaluate(() => {
    sessionStorage.removeItem('recovery-list-error')
    sessionStorage.setItem('recovery-updated', 'yes')
  })
  await records.getByRole('button', { name: '刷新记录', exact: true }).click()
  await expect(page.getByRole('region', { name: '清理中断与待核实结果' })).toHaveCount(0)
  await expect(records.getByText('已中断', { exact: false })).toHaveCount(0)
  await expect(records).toContainText('已取消')
})
