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

test('v6 Mac restoration keeps its scope in read-only archived details', async ({ page }, info) => {
  await mount(page, 'v6')
  const d = page.getByRole('dialog')
  await d.getByRole('button', { name: '检查当前执行记录' }).click()
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '保存归档并释放容量' }).click()
  await d.getByRole('button', { name: /Mac 属性恢复 · ZIP/ }).click()
  await expect(d).toContainText('含 Mac 数据属性、普通权限和修改时间恢复；历史安全批准未恢复。')
  await expect(d).toContainText('/Users/demo/Archives/owned.zip')
  await expect(d.getByRole('button', { name: /确认解压|恢复 Mac/ })).toHaveCount(0)
  await page.screenshot({
    path: info.outputPath('mac-restore-archive.png'),
    animations: 'disabled',
  })
  expect(await calls(page, 'detail')).toHaveLength(1)
})
test('archive requires exact scope and explicit confirmation, retires live capacity once and reads all three original modules', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 960, height: 680 })
  await mount(page)
  await page.getByRole('button', { name: '检查当前执行记录' }).click()
  const confirm = page.getByRole('dialog', { name: '确认归档' })
  await expect(confirm).toContainText('清理与移动 1 批 · 卸载 1 批 · 维护 1 批')
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
  await expect(d).toContainText('清理与移动 1 批 · 卸载 1 批 · 维护 1 批')
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

test('v4 includes all shared-capacity journals and reads full Mole pages without offering mutation', async ({
  page,
}) => {
  await mount(page, 'v4')
  const d = page.getByRole('dialog')
  await d.getByRole('button', { name: '检查当前执行记录' }).click()
  await expect(d).toContainText('完整清理 1 批 · 压缩包解压 1 批 · Homebrew 操作 1 批')
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '保存归档并释放容量' }).dblclick()
  await expect(d.getByRole('button', { name: /查看记录/ })).toHaveCount(6)
  await d.getByRole('button', { name: /完整清理 · Mole/ }).click()
  await expect(d).toContainText('执行失败，需核对已发生的结果')
  await expect(d).not.toContainText('失败，未作修改')
  await expect(d).toContainText('完整事件 2101 条 · 第 1 / 22 页')
  await expect(d.locator('article')).toHaveCount(100)
  await expect(d.getByRole('button', { name: '上一页事件' })).toBeDisabled()
  for (let page = 2; page <= 22; page++) await d.getByRole('button', { name: '下一页事件' }).click()
  await expect(d).toContainText('owned event 2101')
  await expect(d.locator('article')).toHaveCount(1)
  await expect(d.getByRole('button', { name: '下一页事件' })).toBeDisabled()
  await expect(d.locator('details').filter({ hasText: '原始执行记录' }).locator('pre')).toHaveCount(
    0,
  )
  await d.getByRole('button', { name: '返回归档', exact: true }).click()
  for (const kind of ['压缩包解压', 'Homebrew 卸载']) {
    await d.getByRole('button', { name: new RegExp(kind + ' ·') }).click()
    await expect(d).toContainText(
      kind === '压缩包解压' ? '/absent/source.tar.gz' : '/opt/homebrew/bin/brew',
    )
    await expect(d).not.toContainText('当时移入位置')
    await expect(d.getByRole('button', { name: /执行|重新解压|恢复|核查|卸载/ })).toHaveCount(0)
    await d.getByText('原始执行记录', { exact: true }).click()
    await expect(d.locator('pre')).toContainText('"phase": "finished"')
    await d.getByRole('button', { name: '返回归档', exact: true }).click()
  }
  await d.getByRole('button', { name: '导出归档…' }).click()
  expect(await calls(page, 'archive')).toHaveLength(1)
  expect(await calls(page, 'export')).toHaveLength(1)
  expect(await page.evaluate(() => window.archiveFixture.changed())).toBe(1)
  expect(
    (await page.evaluate(() => window.archiveFixture.calls)).every((c) =>
      ['list', 'prepare', 'archive', 'view', 'detail', 'export'].includes(c.command),
    ),
  ).toBe(true)
})
for (const language of ['zh', 'en', 'ja'] as const)
  test(`v4 ${language} review fits a compact desktop and preserves raw paths`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width: 960, height: 780 })
    await mount(page, 'v4')
    // Switch only after locating the Chinese fixture launcher.
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    const d = page.getByRole('dialog')
    const prepare = {
      zh: '检查当前执行记录',
      en: 'Check current execution records',
      ja: '現在の実行記録を確認',
    }[language]
    await d.getByRole('button', { name: prepare, exact: true }).click()
    expect(await d.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
    if (language === 'en') await expect(d).not.toContainText(/[\p{Script=Han}]/u)
    await d.screenshot({ path: testInfo.outputPath(`archive-v4-${language}-confirm.png`) })
    await d.getByRole('checkbox').check()
    await d.locator('button.primary').click()
    await d.getByRole('button').filter({ hasText: /Mole/ }).click()
    await expect(d).toContainText('/absent/historical-account')
    expect(await d.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
    await d.screenshot({ path: testInfo.outputPath(`archive-v4-${language}-detail.png`) })
  })

test('v5 upgrade archive stays read-only, names the operation and preserves the version transition', async ({
  page,
}) => {
  await mount(page, 'v5')
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: '检查当前执行记录' }).click()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '保存归档并释放容量' }).click()
  await dialog.getByRole('button', { name: /Homebrew 升级 ·/ }).click()
  await expect(dialog).toContainText('已核对新版包登记、App 版本与签名')
  await expect(dialog).toContainText('1.0 → 2.0')
  await expect(dialog).toContainText('不提供版本回滚')
  await expect(dialog.getByRole('button', { name: /确认.*升级|执行|恢复/ })).toHaveCount(0)
  expect(await calls(page, 'execute_managed_app')).toHaveLength(0)
})
