import { test, expect, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'retirement', history = false) {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/execution-archive.tsx')).mount(mode),
    mode,
  )
  await page.getByRole('button', { name: '打开执行归档', exact: true }).click()
  if (history) await page.getByRole('button', { name: '查看副本整理记录', exact: true }).click()
  else {
    await page.getByRole('button', { name: '只读查看' }).click()
    await page.getByRole('button', { name: '导出后整理本机副本…', exact: true }).click()
  }
  return page.getByRole('dialog', { name: '整理本机归档副本' })
}
async function calls(page: Page, command: string) {
  return page.evaluate(
    (command) => window.archiveFixture.calls.filter((c) => c.command === command),
    command,
  )
}
test('archive export is explicit and verified before one separately confirmed local retirement', async ({
  page,
}, info) => {
  const d = await mount(page)
  expect(await calls(page, 'prepare_archive_retirement')).toHaveLength(0)
  await d.getByRole('button', { name: '保存完整副本并检查…', exact: true }).click()
  await expect(d).toContainText('/Volumes/Owned/执行记录.moomole-history')
  const execute = d.getByRole('button', { name: '确认整理本机副本', exact: true })
  await expect(execute).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await d.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('retirement-review.png'), animations: 'disabled' })
  await d.getByRole('checkbox').check()
  await execute.click()
  await expect(d).toContainText('本机副本已移入废纸篓')
  expect(await calls(page, 'execute_archive_retirement')).toHaveLength(1)
  const sent = (await calls(page, 'execute_archive_retirement'))[0].args as Record<string, unknown>
  expect(sent).toMatchObject({ id: 'b1000000-0000-4000-8000-000000000001', confirmed: true })
  expect(sent).not.toHaveProperty('path')
  expect(sent).not.toHaveProperty('exported')
  await d.getByRole('button', { name: '返回归档列表', exact: true }).click()
  await expect(page.getByRole('button', { name: '只读查看' })).toHaveCount(0)
})
for (const mode of ['expired', 'mismatch', 'dialog-cancel'])
  test(`${mode} never grants local archive retirement`, async ({ page }) => {
    const d = await mount(page, 'retirement-' + mode)
    await d.getByRole('button', { name: '保存完整副本并检查…', exact: true }).click()
    if (mode === 'expired') {
      await expect(d).toContainText('确认已过期')
      await expect(d.getByRole('button', { name: '确认整理本机副本', exact: true })).toBeDisabled()
    } else if (mode === 'mismatch') await expect(d.getByRole('alert')).toBeVisible()
    else
      await expect(d.getByRole('button', { name: '确认整理本机副本', exact: true })).toHaveCount(0)
    expect(await calls(page, 'execute_archive_retirement')).toHaveLength(0)
  })
for (const phase of ['prepare', 'run'])
  test(`${phase} stop targets the owning request without replay`, async ({ page }) => {
    const d = await mount(page, `retirement-${phase}-stop`)
    await d.getByRole('button', { name: '保存完整副本并检查…', exact: true }).click()
    if (phase === 'run') {
      await d.getByRole('checkbox').check()
      await d.getByRole('button', { name: '确认整理本机副本', exact: true }).click()
    }
    await d.getByRole('button', { name: '停止副本整理', exact: true }).click()
    await expect(d).toContainText('已停止')
    const owned = (
      await calls(
        page,
        phase === 'prepare' ? 'prepare_archive_retirement' : 'execute_archive_retirement',
      )
    )[0].args as Record<string, unknown>
    expect((await calls(page, 'cancel_task'))[0].args).toMatchObject({ taskId: owned.requestId })
  })
test('unknown history stays unknown after explicit review, without executing a file action', async ({
  page,
}) => {
  const d = await mount(page, 'retirement-history-unknown', true)
  await expect(d).toContainText('副本整理结果待核实')
  await expect(d.getByRole('button', { name: '记录副本核查', exact: true })).toBeDisabled()
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '记录副本核查', exact: true }).click()
  await expect(d).toContainText('已人工核查；原结果保留')
  await expect(d).toContainText('副本整理结果待核实')
  expect(await calls(page, 'execute_archive_retirement')).toHaveLength(0)
})
test('a running archive retirement reconnects to its existing owner and can stop that exact request', async ({
  page,
}) => {
  const d = await mount(page, 'retirement-history-running', true)
  await d.getByRole('button', { name: '停止此副本整理', exact: true }).click()
  await expect(d).toContainText('副本整理已停止')
  expect((await calls(page, 'cancel_task'))[0].args).toMatchObject({
    taskId: 'c1000000-0000-4000-8000-000000000001',
  })
  expect(await calls(page, 'execute_archive_retirement')).toHaveLength(0)
})
for (const language of ['en', 'ja'])
  test(`${language} archive retirement copy and paths stay readable`, async ({ page }, info) => {
    const d = await mount(page)
    await d.getByRole('button', { name: '保存完整副本并检查…', exact: true }).click()
    await page.evaluate(async (language) => {
      ;(await import('/src/i18n/locale.ts')).setLanguagePreference(language as 'en' | 'ja')
    }, language)
    const translated = page.getByRole('dialog', {
      name: language === 'en' ? 'Manage local archive copies' : 'ローカルアーカイブコピーの整理',
      exact: true,
    })
    await expect(translated).toContainText('/Volumes/Owned/执行记录.moomole-history')
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await translated.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`retirement-${language}.png`),
      animations: 'disabled',
    })
    expect(await calls(page, 'execute_archive_retirement')).toHaveLength(0)
  })
