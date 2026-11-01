import { expect, test, type Page } from '@playwright/test'
import { installOfficePreviewMock } from './fixtures/office-preview'
async function setup(page: Page, mode = 'normal') {
  await installOfficePreviewMock(page, mode)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  return page.getByRole('region', { name: '个人文件列表' })
}
async function preview(page: Page, format: string) {
  await page
    .getByRole('region', { name: '个人文件列表' })
    .getByRole('article')
    .filter({ hasText: `review.${format}` })
    .getByRole('button', { name: '预览内容', exact: true })
    .click()
  return page.getByRole('dialog', { name: '文件内容预览' })
}
async function calls(page: Page) {
  return page.evaluate(
    () =>
      JSON.parse(sessionStorage.getItem('personal-calls')!) as {
        command: string
        args: Record<string, unknown>
      }[],
  )
}
test('Word excerpt remains literal and separate selection and confirmation are needed before any file action', async ({
  page,
}) => {
  const list = await setup(page)
  const dialog = await preview(page, 'docx')
  await expect(dialog.locator('pre')).toContainText('<script>window.officeInjected=true</script>')
  expect(await page.evaluate(() => (window as any).officeInjected)).toBeUndefined()
  await expect(dialog).toContainText('不还原版式、图片、图表')
  await dialog.getByRole('button', { name: '返回文件列表' }).click()
  const choose = list.getByRole('checkbox', {
    name: '选择 /Users/demo/Downloads/review.docx',
    exact: true,
  })
  await expect(choose).not.toBeChecked()
  expect(
    (await calls(page)).filter((c) => /^(execute|prepare)_personal/.test(c.command)),
  ).toHaveLength(0)
  await choose.check()
  await page.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  const confirm = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(confirm).toContainText('review.docx')
  await expect(confirm.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(page.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  expect((await calls(page)).filter((c) => c.command === 'execute_personal_trash')).toHaveLength(1)
})
test('Excel part selection keeps literal sheet names, cached values, hidden state and the original scan IDs', async ({
  page,
}, info) => {
  await setup(page)
  const dialog = await preview(page, 'xlsx')
  await expect(dialog).toContainText('未重新计算公式')
  await expect(dialog.locator('pre')).toContainText('=SUM(A2:A3) → 4')
  await dialog.getByRole('combobox', { name: '选择办公文件部分' }).selectOption('2')
  await expect(dialog.locator('pre')).toContainText('hidden saved cell')
  await expect(dialog.getByText('隐藏部分', { exact: true })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '下一部分', exact: true })).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('office-excel.png'), animations: 'disabled' })
  await dialog.getByRole('button', { name: '上一部分', exact: true }).click()
  await expect(dialog.locator('pre')).toContainText('A1\t<script>literal</script>')
  const previews = (await calls(page)).filter((c) => c.command === 'preview_personal_file')
  expect(previews.map((c) => c.args.page)).toEqual([1, 2, 1])
  expect(new Set(previews.map((c) => c.args.itemId)).size).toBe(1)
  expect(previews[0].args).not.toHaveProperty('path')
})
test('PowerPoint moves through declared slide parts and never auto-selects a file', async ({
  page,
}) => {
  const list = await setup(page)
  const dialog = await preview(page, 'pptx')
  await expect(dialog.locator('pre')).toHaveText('First slide 中文')
  await dialog.getByRole('button', { name: '下一部分', exact: true }).click()
  await expect(dialog.locator('pre')).toHaveText('Second slide 中文')
  await dialog.getByRole('button', { name: '返回文件列表' }).click()
  await expect(
    list.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/review.pptx', exact: true }),
  ).not.toBeChecked()
})
for (const mode of ['empty', 'truncated', 'unsupported', 'mismatch'])
  test(`${mode} office preview clearly limits what is displayed`, async ({ page }) => {
    await setup(page, mode)
    const dialog = await preview(page, 'docx')
    if (mode === 'empty') await expect(dialog).toContainText('不表示文件或所选部分为空')
    if (mode === 'truncated') await expect(dialog).toContainText('摘录已截断')
    if (mode === 'unsupported' || mode === 'mismatch')
      await expect(dialog.getByRole('alert')).toBeVisible()
    expect(
      (await calls(page)).filter((c) => /^(execute|prepare)_personal/.test(c.command)),
    ).toHaveLength(0)
  })
test('closing a pending Office excerpt discards its late response', async ({ page }) => {
  await setup(page, 'late')
  const dialog = await preview(page, 'docx')
  await expect(dialog).toContainText('正在读取预览')
  await dialog.getByRole('button', { name: '返回文件列表' }).click()
  await page.evaluate(async () => {
    await (window as any).__TAURI_INTERNALS__.invoke('fixture_resolve_office')
  })
  await expect(dialog).toHaveCount(0)
  expect(
    (await calls(page)).filter((c) => /^(execute|prepare)_personal/.test(c.command)),
  ).toHaveLength(0)
})
for (const language of ['en', 'ja'])
  test(`${language} Office explanations translate while worksheet names remain literal`, async ({
    page,
  }, info) => {
    await setup(page)
    await page.evaluate(async (language) => {
      const { setLanguagePreference } = await import('/src/i18n/locale.ts')
      setLanguagePreference(language as 'en' | 'ja')
    }, language)
    const list = page.getByRole('region', {
      name: language === 'en' ? 'Personal file list' : '個人ファイル一覧',
    })
    await list
      .getByRole('article')
      .filter({ hasText: 'review.xlsx' })
      .getByRole('button', {
        name: language === 'en' ? 'Preview content' : '内容をプレビュー',
        exact: true,
      })
      .click()
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText(
      language === 'en' ? 'formulas are not recalculated' : '数式の再計算は行いません',
    )
    await expect(dialog.locator('select option').first()).toHaveText('清理')
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`office-${language}.png`),
      animations: 'disabled',
    })
  })
