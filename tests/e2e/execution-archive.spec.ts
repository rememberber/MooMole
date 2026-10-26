import { test, expect } from '@playwright/test'
async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/execution-archive.tsx')).mount(mode),
    mode,
  )
  await page.getByRole('button', { name: '打开执行归档', exact: true }).click()
  const d = page.getByRole('dialog', { name: '执行归档', exact: true })
  await expect(d.getByRole('button', { name: '检查当前执行记录' })).toBeEnabled()
  return d
}
const calls = (page: import('@playwright/test').Page, command: string) =>
  page.evaluate(
    (command) => window.archiveFixture.calls.filter((c) => c.command === command),
    command,
  )
test('archive requires exact scope and explicit confirmation, retires live capacity once and reads all three original modules', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 960, height: 680 })
  await mount(page)
  await page.getByRole('button', { name: '检查当前执行记录' }).click()
  const confirm = page.getByRole('dialog', { name: '确认归档' })
  await expect(confirm).toContainText('清理 1 批 · 卸载 1 批 · 维护 1 批')
  await expect(confirm.getByRole('button', { name: '保存归档并释放容量' })).toBeDisabled()
  await expect.poll(async () => (await calls(page, 'prepare')).length).toBe(1)
  expect(await calls(page, 'archive')).toHaveLength(0)
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await confirm.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
    await confirm.screenshot({ path: testInfo.outputPath('archive-confirm-' + theme + '.png') })
  }
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: '保存归档并释放容量' }).dblclick()
  const view = page.getByRole('dialog', { name: '执行归档', exact: true })
  await expect(view).toContainText('活动历史容量已释放')
  expect(await calls(page, 'archive')).toEqual([
    { command: 'archive', args: { fingerprint: 'a'.repeat(64) } },
  ])
  expect(await page.evaluate(() => window.archiveFixture.changed())).toBe(1)
  for (const kind of ['清理', '卸载', '维护']) {
    await view.getByRole('button', { name: new RegExp(kind + ' ·') }).click()
    const detail = page.getByRole('dialog', { name: '归档记录' })
    await expect(detail).toContainText('已人工核查')
    await expect(detail.getByRole('button', { name: /执行|记录已核查|重新检查/ })).toHaveCount(0)
    await detail.getByText('原始执行记录', { exact: true }).click()
    await expect(detail).toContainText('"snapshotSha256"')
    await detail.getByRole('button', { name: '返回归档', exact: true }).click()
  }
  await view.getByRole('button', { name: '导出归档…' }).click()
  await expect(view).toContainText('归档已导出')
  expect(await calls(page, 'export')).toHaveLength(1)
})
test('opening an existing archive is read-only and rejects mismatched archive identity', async ({
  page,
}) => {
  const d = await mount(page, 'existing')
  await d.getByRole('button', { name: '只读查看' }).click()
  await expect(d).toContainText('清理 1 批 · 卸载 1 批 · 维护 1 批')
  await expect(d.getByRole('button', { name: /查看记录/ })).toHaveCount(3)
  await expect(d.getByRole('alert')).toHaveCount(0)
  expect(await calls(page, 'prepare')).toHaveLength(0)
  expect(await calls(page, 'archive')).toHaveLength(0)
  await mount(page, 'existing-invalid')
  const bad = page.getByRole('dialog', { name: '执行归档', exact: true }).last()
  await bad.getByRole('button', { name: '只读查看' }).click()
  await expect(bad.getByRole('alert')).toContainText('无法核对')
  expect(await calls(page, 'archive')).toHaveLength(0)
})
test('unknown or failed storage does not release capacity or silently retry', async ({ page }) => {
  let d = await mount(page, 'unknown')
  await d.getByRole('button', { name: '检查当前执行记录' }).click()
  await expect(d.getByRole('alert')).toContainText('核查未知')
  expect(await calls(page, 'archive')).toHaveLength(0)
  await mount(page, 'save-error')
  d = page.getByRole('dialog', { name: '执行归档', exact: true }).last()
  await d.getByRole('button', { name: '检查当前执行记录' }).click()
  const c = page.getByRole('dialog', { name: '确认归档' }).last()
  await c.getByRole('checkbox').check()
  await c.getByRole('button', { name: '保存归档并释放容量' }).click()
  await expect(d.getByRole('alert')).toContainText('原记录保持')
  expect(await page.evaluate(() => window.archiveFixture.changed())).toBe(0)
  expect(await calls(page, 'archive')).toHaveLength(1)
  await page.evaluate(() => window.archiveFixture.allowSave())
  await expect(d.getByRole('button', { name: '检查当前执行记录' })).toBeDisabled()
  await d.getByRole('button', { name: '刷新活动记录和归档' }).click()
  await expect(d.getByRole('button', { name: '检查当前执行记录' })).toBeEnabled()
  await d.getByRole('button', { name: '检查当前执行记录' }).click()
  await c.getByRole('checkbox').check()
  await c.getByRole('button', { name: '保存归档并释放容量' }).click()
  await expect(d).toContainText('归档已保存')
  expect(await page.evaluate(() => window.archiveFixture.changed())).toBe(2)
})

test('closing a pending read does not reopen the archive when its response arrives', async ({
  page,
}) => {
  const d = await mount(page, 'existing-slow')
  await d.getByRole('button', { name: '只读查看' }).click()
  await expect(d.getByRole('button', { name: /清理 ·/ })).toBeVisible()
  await d.getByRole('button', { name: /清理 ·/ }).click()
  await expect.poll(async () => (await calls(page, 'detail')).length).toBe(1)
  await page
    .getByRole('dialog', { name: '归档记录' })
    .getByRole('button', { name: '关闭弹窗' })
    .click()
  await page.evaluate(() => window.archiveFixture.release())
  await expect(page.getByRole('dialog')).toHaveCount(0)
  expect(await calls(page, 'archive')).toHaveLength(0)
})
