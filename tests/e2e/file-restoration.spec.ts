import { test, expect } from '@playwright/test'
async function open(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/file-restoration.tsx')).mount(mode),
    mode,
  )
  await page
    .getByRole('region', { name: '执行记录' })
    .getByRole('button', { name: /查看逐项结果/ })
    .click()
  await page.getByRole('dialog').locator('.execution-item > summary').click()
  await page.getByRole('button', { name: '恢复原位置…' }).click()
  return page.getByRole('dialog', { name: '恢复原位置', exact: true })
}
test('software content receipts enter separate same-volume restoration without modifying application indexes', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const d = await open(page, 'software')
  await expect(d).toContainText('/Users/demo/.Trash/chosen.txt')
  await expect(d).toContainText('不修改浏览器下载记录、聊天导出清单或播放器资料库')
  await expect(d.getByRole('button', { name: '确认恢复这一项' })).toBeDisabled()
  expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({
    path: info.outputPath('software-restoration.png'),
    animations: 'disabled',
  })
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认恢复这一项' }).click()
  await expect(page.getByRole('dialog', { name: '文件恢复结果', exact: true })).toContainText(
    '已恢复到原位置',
  )
  const calls = JSON.parse((await page.locator('#restoration-fixture').getAttribute('data-calls'))!)
  expect(calls.filter((c: any) => c.command === 'execute_file_restoration')).toHaveLength(1)
  expect(calls.find((c: any) => c.command === 'prepare_file_restoration').args).not.toHaveProperty(
    'path',
  )
})
for (const [language, title, notice, confirm] of [
  [
    'en',
    'Restore original location',
    'does not change browser download records',
    'Confirm this restoration',
  ],
  ['ja', '元の場所に復元', 'ブラウザのダウンロード記録', 'この項目の復元を確認'],
])
  test(`${language} software restoration translates its scope and preserves exact receipt paths`, async ({
    page,
  }) => {
    await open(page, 'software')
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    const d = page.getByRole('dialog', { name: title, exact: true })
    await expect(d).toContainText(notice)
    await expect(d).toContainText('/Users/demo/.Trash/chosen.txt')
    await expect(d.getByRole('button', { name: confirm, exact: true })).toBeDisabled()
  })
for (const mode of ['softwareLibrary', 'softwareUnknown'])
  test(`${mode} keeps Finder guidance without offering automatic restoration`, async ({ page }) => {
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/file-restoration.tsx')).mount(mode),
      mode,
    )
    await page
      .getByRole('region', { name: '执行记录' })
      .getByRole('button', { name: /查看逐项结果/ })
      .click()
    await page.getByRole('dialog').locator('.execution-item > summary').click()
    await expect(page.getByRole('button', { name: '恢复原位置…' })).toHaveCount(0)
    expect(
      JSON.parse((await page.locator('#restoration-fixture').getAttribute('data-calls'))!),
    ).toEqual([])
  })
test('history offers a fresh exact restoration confirmation and submits once after acknowledgement', async ({
  page,
}) => {
  const dialog = await open(page)
  await expect(dialog).toContainText('/Users/demo/Organized/chosen.txt')
  await expect(dialog).toContainText('/Users/demo/Downloads/chosen.txt')
  await expect(dialog.getByRole('button', { name: '确认恢复这一项' })).toBeDisabled()
  await expect(page.locator('#restoration-fixture')).not.toHaveAttribute(
    'data-calls',
    /execute_file_restoration/,
  )
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认恢复这一项' }).click()
  await expect(page.getByRole('dialog', { name: '文件恢复结果', exact: true })).toContainText(
    '已恢复到原位置',
  )
  const calls = JSON.parse((await page.locator('#restoration-fixture').getAttribute('data-calls'))!)
  const submit = calls.filter((c: { command: string }) => c.command === 'execute_file_restoration')
  expect(submit).toHaveLength(1)
  expect(submit[0].args).toMatchObject({
    id: '9f3123c1-95a5-4af0-aec8-0b5c82c6a2dd',
    confirmed: true,
  })
  expect(submit[0].args).not.toHaveProperty('path')
})
test('a different original location cannot be confirmed or executed', async ({ page }) => {
  const dialog = await open(page, 'mismatch')
  await expect(dialog.getByRole('alert')).toContainText('恢复确认与原记录不一致')
  await expect(dialog.getByRole('button', { name: '确认恢复这一项' })).toBeDisabled()
  await expect(page.locator('#restoration-fixture')).not.toHaveAttribute(
    'data-calls',
    /execute_file_restoration/,
  )
})
test('cancel targets the exact restoration request and keeps the not-restored result', async ({
  page,
}) => {
  const dialog = await open(page, 'pending')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认恢复这一项' }).click()
  await dialog.getByRole('button', { name: '取消后续恢复' }).click()
  await expect(page.getByRole('dialog', { name: '文件恢复结果', exact: true })).toContainText(
    '本项未恢复',
  )
  const calls = JSON.parse((await page.locator('#restoration-fixture').getAttribute('data-calls'))!)
  expect(calls.find((c: { command: string }) => c.command === 'cancel_task').args.taskId).toBe(
    calls.find((c: { command: string }) => c.command === 'execute_file_restoration').args.requestId,
  )
})
