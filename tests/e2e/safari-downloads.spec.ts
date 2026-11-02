import { test, expect, type Page } from '@playwright/test'
import { installSoftwareContentMock } from './fixtures/software-content-native'
async function setup(page: Page, mode = 'normal') {
  await installSoftwareContentMock(page)
  if (mode === 'unknown')
    await page.addInitScript(() => sessionStorage.setItem('browser-profile-error', '1'))
  if (mode === 'mismatch')
    await page.addInitScript(() => {
      const native = (window as any).__TAURI_INTERNALS__,
        invoke = native.invoke
      native.invoke = async (command: string, args: any) => {
        const r = await invoke(command, args)
        return command === 'list_browser_download_profiles' && args.browser === 'safari'
          ? {
              ...r,
              profiles: r.profiles.map((p: any) => ({
                ...p,
                historyPath: p.historyPath.replace('Downloads.plist', 'History.db'),
              })),
            }
          : r
      }
    })
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '软件清理', exact: true }).click()
  await page.getByRole('tab', { name: '附件、下载与离线', exact: true }).click()
  const main = page.getByRole('main', { name: '软件清理内容' })
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  await main.getByRole('combobox', { name: '浏览器', exact: true }).selectOption('safari')
  return main
}
test('Safari requires a list selection, withdraws old files on source change and confirms exact newly selected IDs', async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const main = await setup(page),
    profiles = main.getByRole('combobox', { name: '浏览器配置目录' }),
    scan = page.getByRole('button', { name: '扫描下载记录', exact: true })
  await expect(profiles).toBeEnabled()
  await expect(profiles).toHaveValue('')
  await expect(scan).toBeDisabled()
  await expect(main).toContainText('并非浏览配置')
  await profiles.selectOption('Standard')
  await expect(main).toContainText('/Users/demo/Library/Safari/Downloads.plist')
  await scan.click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await list
    .getByRole('article')
    .filter({ hasText: 'notes.txt' })
    .getByRole('button', { name: '预览内容' })
    .click()
  const preview = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(preview.locator('pre')).toHaveText('owned attachment preview')
  await expect(preview).toContainText('/Users/demo/Downloads/notes.txt')
  await preview.getByRole('button', { name: '返回文件列表' }).click()
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await list.getByRole('checkbox').first().check()
  await profiles.selectOption('Sandbox')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await scan.click()
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await list.getByRole('checkbox').first().check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理浏览器下载' })
  await expect(dialog).toContainText('Safari · Sandbox')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('safari-confirm-zh.png'), animations: 'disabled' })
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls
      .filter((c: any) => c.command === 'start_software_content')
      .map((c: any) => c.args.browserProfile),
  ).toEqual([
    { browser: 'safari', profile: 'Standard' },
    { browser: 'safari', profile: 'Sandbox' },
  ])
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
  expect(
    calls.find((c: any) => c.command === 'prepare_software_trash').args.selectedIds,
  ).toHaveLength(1)
})
for (const mode of ['unknown', 'mismatch'])
  test(`${mode} Safari sources stay unknown without scanning or falling back`, async ({ page }) => {
    const main = await setup(page, mode)
    await expect(main.getByRole('alert')).toBeVisible()
    await expect(page.getByRole('button', { name: '扫描下载记录', exact: true })).toBeDisabled()
    const calls = await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('software-content-calls')!),
    )
    expect(
      calls.filter((c: any) =>
        ['start_software_content', 'execute_software_trash'].includes(c.command),
      ),
    ).toHaveLength(0)
  })
for (const language of ['en', 'ja'])
  test(`${language} Safari list explanation translates and keeps the exact fixed source`, async ({
    page,
  }, info) => {
    const main = await setup(page)
    await main.getByRole('combobox', { name: '浏览器配置目录' }).selectOption('Standard')
    await page.evaluate(
      async (language) =>
        (await import('/src/i18n/locale.ts')).setLanguagePreference(language as 'en' | 'ja'),
      language,
    )
    await expect(page.getByRole('main')).toContainText('/Users/demo/Library/Safari/Downloads.plist')
    await expect(page.getByRole('main')).toContainText(
      language === 'en' ? 'not browsing profiles' : '閲覧プロファイルではありません',
    )
    await page.setViewportSize({ width: 960, height: 780 })
    const bounds = await page.locator('.browser-downloads-picker').evaluate((e) => {
      const rect = e.getBoundingClientRect()
      return {
        right: rect.right,
        viewport: innerWidth,
        width: e.clientWidth,
        scroll: e.scrollWidth,
      }
    })
    expect(bounds.right).toBeLessThanOrEqual(bounds.viewport)
    expect(bounds.scroll).toBeLessThanOrEqual(bounds.width)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`safari-source-${language}.png`),
      animations: 'disabled',
    })
    const calls = await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('software-content-calls')!),
    )
    expect(calls.filter((c: any) => c.command === 'start_software_content')).toHaveLength(0)
  })
