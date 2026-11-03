import { test, expect, type Page } from '@playwright/test'
async function start(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/main-app-login.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: 'MooMole 登录启动', exact: true })
}
async function calls(page: Page) {
  return JSON.parse((await page.locator('#main-login-fixture').getAttribute('data-calls'))!) as {
    command: string
    args: Record<string, unknown>
  }[]
}
test('read-only default and explicit enable/disable preserve independent preferences', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const section = await start(page)
  await expect(section).toContainText('登录时不自动打开')
  const enable = section.getByRole('button', { name: '开启登录启动', exact: true })
  await expect(enable).toBeDisabled()
  expect((await calls(page)).every((c) => c.command === 'get_main_app_login')).toBe(true)
  await section.getByRole('checkbox').check()
  await enable.dblclick()
  await expect(section).toContainText('macOS 已允许登录启动')
  await expect(section.getByRole('checkbox')).not.toBeChecked()
  expect((await calls(page)).filter((c) => c.command === 'set_main_app_login')).toHaveLength(1)
  await section.getByRole('checkbox').check()
  await section.getByRole('button', { name: '关闭登录启动', exact: true }).click()
  await expect(section).toContainText('登录时不自动打开')
  const changed = (await calls(page)).filter((c) => c.command === 'set_main_app_login')
  expect(changed.map((c) => c.args.enabled)).toEqual([true, false])
  expect(changed[0].args).toEqual({ enabled: true, requestId: expect.any(String), confirmed: true })
  expect((await calls(page)).some((c) => /configure_|start_task/.test(c.command))).toBe(false)
  await page.screenshot({ path: info.outputPath('login-zh.png') })
})
test('an initially missing own service needs an explicit registration request', async ({
  page,
}) => {
  const s = await start(page, 'notFound')
  await expect(s).toContainText('系统尚未确认此副本的登录服务')
  await s.getByRole('checkbox').check()
  await s.getByRole('button', { name: '开启登录启动', exact: true }).click()
  await expect(s).toContainText('macOS 已允许登录启动')
})
test('approval stays pending and system settings opens only on a separate click', async ({
  page,
}) => {
  const s = await start(page, 'approval')
  await s.getByRole('checkbox').check()
  await s.getByRole('button', { name: '开启登录启动', exact: true }).click()
  await expect(s).toContainText('登录启动等待系统批准')
  expect((await calls(page)).some((c) => c.command === 'open_login_item_settings')).toBe(false)
  await s.getByRole('button', { name: '在系统登录项中核对' }).click()
  expect((await calls(page)).filter((c) => c.command === 'open_login_item_settings')).toHaveLength(
    1,
  )
})
test('unknown records require explicit review and are never reissued or relabelled as success', async ({
  page,
}) => {
  const s = await start(page, 'unknown')
  await s.getByText('当前副本与最近 50 次登录设置记录', { exact: true }).click()
  await expect(s.getByRole('button', { name: '开启登录启动', exact: true })).toBeDisabled()
  await s.getByRole('button', { name: '已核对系统登录状态，保留原结果', exact: true }).click()
  await expect(s).toContainText('已记录人工核查，原结果未更改。')
  await expect(s).toContainText('登录设置结果待核对')
  expect((await calls(page)).filter((c) => c.command === 'set_main_app_login')).toHaveLength(0)
})
for (const mode of ['failed', 'mismatch'])
  test(`${mode} consumes confirmation and never retries`, async ({ page }) => {
    const s = await start(page, mode)
    await s.getByRole('checkbox').check()
    await s.getByRole('button', { name: '开启登录启动', exact: true }).click()
    if (mode === 'mismatch') await expect(s.getByRole('alert')).toBeVisible()
    else await expect(s.getByRole('button', { name: '开启登录启动', exact: true })).toBeDisabled()
    await expect(s.getByRole('checkbox')).not.toBeChecked()
    expect((await calls(page)).filter((c) => c.command === 'set_main_app_login')).toHaveLength(1)
  })
for (const lang of ['en', 'ja'])
  test(`${lang} explains login scope in a compact layout`, async ({ page }, info) => {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/main-app-login.tsx')).mount(mode),
      lang,
    )
    const s = page.getByRole('region', {
      name: lang === 'en' ? 'MooMole login startup' : 'MooMole のログイン起動',
      exact: true,
    })
    await expect(s).toBeVisible()
    expect(await s.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await expect(s.getByRole('checkbox')).not.toBeChecked()
    await page.screenshot({ path: info.outputPath(`login-${lang}.png`) })
    expect((await calls(page)).every((c) => c.command === 'get_main_app_login')).toBe(true)
  })
