import { test, expect, type Page } from '@playwright/test'
async function select(page: Page, mode = 'normal') {
  await page.setViewportSize({ width: 960, height: 780 })
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/file-restoration-batch.tsx')).mount(mode),
    mode,
  )
  await page
    .getByRole('region', { name: '执行记录' })
    .getByRole('button', { name: /查看逐项结果/ })
    .click()
  const history = page.getByRole('dialog')
  await expect(
    history.getByRole('checkbox', { name: '选择恢复 /Users/demo/Downloads/2.txt', exact: true }),
  ).toHaveCount(0)
  await expect(history.getByRole('button', { name: '恢复所选原位置…' })).toBeDisabled()
  for (const n of [0, 1])
    await history
      .getByRole('checkbox', { name: `选择恢复 /Users/demo/Downloads/${n}.txt`, exact: true })
      .check()
  await history.getByRole('textbox', { name: '搜索执行路径' }).fill('0.txt')
  await expect(history).toContainText('已选恢复 2 项（含筛选外项目）')
  await history.getByRole('button', { name: '恢复所选原位置…' }).click()
  return page.getByRole('dialog', { name: '批量恢复原位置' })
}
for (const mode of ['normal', 'conflict'])
  test(`batch restoration ${mode} confirms the full selection and records each result`, async ({
    page,
  }) => {
    const dialog = await select(page, mode)
    await expect(dialog.getByRole('button', { name: '确认恢复所选文件' })).toBeDisabled()
    await expect(dialog).toContainText('/Users/demo/Downloads/1.txt')
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    if (mode === 'normal') {
      for (const theme of ['light', 'dark']) {
        await page.evaluate((theme) => {
          document.documentElement.dataset.theme = theme
        }, theme)
        await page.screenshot({
          path: `test-results/batch-restoration-${theme}.png`,
          animations: 'disabled',
        })
      }
    }
    await expect(page.locator('#batch-restoration-fixture')).not.toHaveAttribute(
      'data-calls',
      /execute_file/,
    )
    await dialog.getByRole('checkbox').check()
    await dialog.getByRole('button', { name: '确认恢复所选文件' }).click()
    const result = page.getByRole('dialog', { name: '批量恢复结果' })
    await expect(result.getByRole('status')).toContainText(
      mode === 'conflict' ? '已恢复 1 / 2 项' : '已恢复 2 / 2 项',
    )
    if (mode === 'conflict') await expect(result).toContainText('原位置已有同名项目')
    const calls = JSON.parse(
      (await page.locator('#batch-restoration-fixture').getAttribute('data-calls'))!,
    )
    expect(
      calls.filter((c: { command: string }) => c.command === 'execute_file_restoration_batch'),
    ).toHaveLength(1)
    expect(calls[0].args).toMatchObject({ itemIds: ['source-0', 'source-1'] })
    expect(calls[0].args).not.toHaveProperty('path')
    await expect(result.getByRole('button', { name: '确认恢复所选文件' })).toHaveCount(0)
  })
test('batch cancellation targets the request and preserves completed files', async ({ page }) => {
  const dialog = await select(page, 'pending')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认恢复所选文件' }).click()
  await dialog.getByRole('button', { name: '取消后续恢复' }).click()
  await expect(page.getByRole('dialog').getByRole('status')).toContainText('已恢复 1 / 2 项')
  const calls = JSON.parse(
    (await page.locator('#batch-restoration-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.find((c: { command: string }) => c.command === 'cancel_task').args.taskId).toBe(
    calls.find((c: { command: string }) => c.command === 'execute_file_restoration_batch').args
      .requestId,
  )
})
for (const mode of ['mismatch', 'result-mismatch'])
  test(`batch ${mode} evidence is rejected without retry`, async ({ page }) => {
    const dialog = await select(page, mode)
    if (mode === 'result-mismatch') {
      await dialog.getByRole('checkbox').check()
      await dialog.getByRole('button', { name: '确认恢复所选文件' }).click()
    }
    await expect(dialog.getByRole('alert')).toContainText(
      mode === 'mismatch' ? '批量恢复确认无法核对' : '批量恢复结果无法核对',
    )
    if (mode === 'mismatch')
      await expect(page.locator('#batch-restoration-fixture')).not.toHaveAttribute(
        'data-calls',
        /execute_file/,
      )
    else await expect(dialog.getByRole('button', { name: '确认恢复所选文件' })).toHaveCount(0)
  })
