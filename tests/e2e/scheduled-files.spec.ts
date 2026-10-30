import { test, expect, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/scheduled-files.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: '定期文件处理' })
}
async function preview(page: Page, mode = 'normal', action = 'move') {
  const region = await mount(page, mode)
  await region.getByRole('button', { name: '添加处理规则…' }).click()
  const dialog = page.getByRole('dialog', { name: '预览并启用定期处理' })
  await dialog.getByRole('button', { name: '选择源目录…' }).click()
  if (action === 'trash') await dialog.getByLabel('定期操作', { exact: true }).selectOption('trash')
  else await dialog.getByRole('button', { name: '选择目标目录…' }).click()
  await dialog.getByRole('button', { name: '只读预览规则' }).click()
  return dialog
}
for (const action of ['move', 'trash'])
  test(`recurring ${action} remains off until a read-only preview and explicit standing authorization`, async ({
    page,
  }) => {
    const dialog = await preview(page, 'normal', action)
    await expect(dialog.getByRole('region', { name: '定期处理预览' })).toContainText(
      '/Users/demo/RuleSource/old.zip',
    )
    await expect(dialog).toContainText('后续符合条件的文件也会按规则处理')
    await expect(dialog.getByRole('button', { name: '确认启用此处理规则' })).toBeDisabled()
    await expect(page.locator('#scheduled-files-fixture')).not.toHaveAttribute(
      'data-calls',
      /enable_scheduled_files|run_scheduled_files/,
    )
    if (action === 'move') {
      for (const theme of ['light', 'dark']) {
        await page.evaluate((theme) => {
          document.documentElement.dataset.theme = theme
        }, theme)
        expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
        await page.screenshot({
          path: `test-results/scheduled-files-${theme}.png`,
          animations: 'disabled',
        })
      }
    }
    await dialog.getByRole('checkbox').check()
    await dialog.getByRole('button', { name: '确认启用此处理规则' }).click()
    await expect(dialog).toHaveCount(0)
    await expect(page.getByRole('region', { name: '定期文件处理' })).toContainText('本次运行已启用')
    const calls = JSON.parse(
      (await page.locator('#scheduled-files-fixture').getAttribute('data-calls'))!,
    )
    expect(
      calls.filter((c: { command: string }) => c.command === 'enable_scheduled_files'),
    ).toEqual([
      {
        command: 'enable_scheduled_files',
        args: { id: '5a79a140-f83f-49f9-8ef2-f4e04688c4fa', confirmed: true },
      },
    ])
    expect(
      calls.filter((c: { command: string }) => c.command === 'run_scheduled_files'),
    ).toHaveLength(0)
  })
test('changing any previewed rule withdraws acknowledgement and requires a new preview', async ({
  page,
}) => {
  const dialog = await preview(page)
  await dialog.getByRole('checkbox').check()
  await dialog.getByText('编辑处理条件', { exact: true }).click()
  await dialog.getByLabel('至少多少天未修改', { exact: true }).fill('60')
  await expect(dialog.getByRole('region', { name: '定期处理预览' })).toHaveCount(0)
  await expect(dialog.getByRole('button', { name: '确认启用此处理规则' })).toBeDisabled()
  await expect(page.locator('#scheduled-files-fixture')).not.toHaveAttribute(
    'data-calls',
    /enable_scheduled_files/,
  )
})
test('an enabled rule runs only on the explicit one-run action and sends IDs without file paths', async ({
  page,
}) => {
  const dialog = await preview(page)
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认启用此处理规则' }).click()
  const region = page.getByRole('region', { name: '定期文件处理' })
  await region.getByRole('button', { name: '按规则处理一次' }).click()
  await region.getByRole('button', { name: '停止本次处理' }).click()
  await expect(region).toContainText('已暂停，需重新预览启用')
  const calls = JSON.parse(
    (await page.locator('#scheduled-files-fixture').getAttribute('data-calls'))!,
  )
  const run = calls.filter((c: { command: string }) => c.command === 'run_scheduled_files')
  expect(run).toHaveLength(1)
  expect(run[0].args.policyId).toBe('4a79a140-f83f-49f9-8ef2-f4e04688c4fa')
  expect(run[0].args).not.toHaveProperty('root')
  expect(run[0].args).not.toHaveProperty('selectedIds')
  expect(calls.find((c: { command: string }) => c.command === 'cancel_task').args.taskId).toBe(
    run[0].args.requestId,
  )
})
test('zero current matches still explains future-file authorization before enabling', async ({
  page,
}) => {
  const dialog = await preview(page, 'zero')
  await expect(dialog).toContainText('本次没有符合项；启用后仍会处理后续符合规则的文件。')
  await expect(dialog.getByRole('checkbox')).not.toBeChecked()
  await expect(dialog.getByRole('button', { name: '确认启用此处理规则' })).toBeDisabled()
})
for (const mode of ['mismatch', 'saved-mismatch'])
  test(`${mode} data cannot silently enable or repeat a different rule`, async ({ page }) => {
    const dialog = await preview(page, mode)
    if (mode === 'saved-mismatch') {
      await dialog.getByRole('checkbox').check()
      await dialog.getByRole('button', { name: '确认启用此处理规则' }).click()
    }
    await expect(dialog.getByRole('alert')).toBeVisible()
    await expect(dialog.getByRole('button', { name: '确认启用此处理规则' })).toBeDisabled()
    const calls = JSON.parse(
      (await page.locator('#scheduled-files-fixture').getAttribute('data-calls'))!,
    )
    expect(
      calls.filter((c: { command: string }) => c.command === 'enable_scheduled_files'),
    ).toHaveLength(mode === 'mismatch' ? 0 : 1)
  })
test('existing active recurring work is only observed, and cancellation addresses its request', async ({
  page,
}) => {
  const region = await mount(page, 'active')
  await expect(region.getByRole('button', { name: '停止本次处理' })).toBeVisible()
  await expect(page.locator('#scheduled-files-fixture')).not.toHaveAttribute(
    'data-calls',
    /run_scheduled_files|enable_scheduled_files/,
  )
  await region.getByRole('button', { name: '停止本次处理' }).click()
  await expect(region).toContainText('已暂停，需重新预览启用')
  await region.getByText('最近定期处理结果', { exact: true }).click()
  await expect(region).toContainText('未全部完成，规则已暂停')
  const calls = JSON.parse(
    (await page.locator('#scheduled-files-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.find((c: { command: string }) => c.command === 'cancel_task').args.taskId).toBe(
    '6a79a140-f83f-49f9-8ef2-f4e04688c4fa',
  )
})
test('a pause-save failure remains visible after successful metadata reads and never starts processing', async ({
  page,
}) => {
  const region = await mount(page, 'pause-failure')
  await region.getByRole('button', { name: '暂停此处理规则' }).click()
  await expect(region.getByRole('alert')).toContainText('暂停设置尚未保存')
  await region.getByRole('button', { name: '刷新定期处理' }).click()
  await expect(region.getByRole('alert')).toContainText('暂停设置尚未保存')
  await expect(region).toContainText('已暂停，需重新预览启用')
  await expect(page.locator('#scheduled-files-fixture')).not.toHaveAttribute(
    'data-calls',
    /run_scheduled_files|enable_scheduled_files/,
  )
})
