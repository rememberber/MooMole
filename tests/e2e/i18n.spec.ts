import { expect, test } from '@playwright/test'

test.describe('first launch system language', () => {
  for (const [locale, language, overview] of [
    ['zh-CN', 'zh-CN', '概览'],
    ['zh-TW', 'zh-CN', '概览'],
    ['en-GB', 'en', 'Overview'],
    ['ja-JP', 'ja', '概要'],
    ['fr-FR', 'en', 'Overview'],
  ]) {
    test.describe(locale, () => {
      test.use({ locale })
      test('uses supported primary language or English fallback', async ({ page }) => {
        await page.goto('/')
        await expect(page.locator('html')).toHaveAttribute('lang', language)
        await expect(page.getByRole('button', { name: overview, exact: true })).toBeVisible()
      })
    })
  }
})

test('switches immediately, persists reloads, preserves page state and returns to system language', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('Projects')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page.getByRole('combobox', { name: '界面语言', exact: true }).selectOption('en')
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await expect(page.getByRole('dialog', { name: 'Settings', exact: true })).toBeVisible()
  await expect(page.getByRole('combobox', { name: 'Interface language' })).toHaveValue('en')
  await page.getByRole('combobox', { name: 'Interface language' }).selectOption('ja')
  await expect(page.getByRole('dialog', { name: '設定', exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('textbox', { name: 'フォルダの内容を絞り込む' })).toHaveValue(
    'Projects',
  )
  await page.reload()
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja')
  await page.getByRole('button', { name: '設定', exact: true }).click()
  await expect(page.getByRole('combobox', { name: '表示言語' })).toHaveValue('ja')
  await page.getByRole('combobox', { name: '表示言語' }).selectOption('system')
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN')
})

test('all main pages translate headings and keep preview boundaries', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page.getByRole('combobox', { name: '界面语言', exact: true }).selectOption('en')
  await page.keyboard.press('Escape')
  for (const label of [
    'Quick cleanup',
    'AI deep scan',
    'Space analyzer',
    'Cleanup',
    'Applications',
    'System status',
    'Maintenance',
    'Project artifacts',
    'Installers',
    'Activity history',
  ]) {
    await page.getByRole('button', { name: label, exact: true }).click()
    await expect(page.getByRole('main')).toBeVisible()
    await expect(page.getByRole('main')).not.toContainText('正在打开…')
  }
})

test('native system language overrides WebView locale; failed saves keep the previous language', async ({
  page,
}) => {
  const { demoBootstrap } = await import('../../src/lib/demo')
  await page.addInitScript(
    (initial) => {
      const stored = () =>
        JSON.parse(sessionStorage.getItem('native-settings') ?? 'null') ?? initial.settings
      Object.assign(window, {
        isTauri: true,
        __TAURI_INTERNALS__: {
          transformCallback: () => 1,
          invoke: async (command: string, args: { settings?: unknown }) => {
            if (command === 'bootstrap')
              return { ...initial, systemLanguage: 'ja-JP', settings: stored() }
            if (command === 'save_settings') {
              if (sessionStorage.getItem('fail-save')) throw new Error('Synthetic save failure')
              sessionStorage.setItem('native-settings', JSON.stringify(args.settings))
              return args.settings
            }
            if (
              [
                'get_history',
                'list_review_plans',
                'list_application_uninstalls',
                'list_quicklook_refreshes',
              ].includes(command)
            )
              return []
            throw new Error(`Unexpected command: ${command}`)
          },
        },
      })
    },
    { ...demoBootstrap, settings: { ...demoBootstrap.settings, language: undefined } },
  )
  await page.goto('/')
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja')
  await page.getByRole('button', { name: '設定', exact: true }).click()
  const language = page.getByRole('combobox', { name: '表示言語', exact: true })
  await expect(language).toHaveValue('system')
  await page.evaluate(() => sessionStorage.setItem('fail-save', 'yes'))
  await language.selectOption('en')
  await expect(page.getByRole('alert')).toContainText('Synthetic save failure')
  await expect(page.locator('html')).toHaveAttribute('lang', 'ja')
  await expect(language).toHaveValue('system')
  await page.evaluate(() => sessionStorage.removeItem('fail-save'))
  await language.selectOption('en')
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await page.reload()
  await expect(page.locator('html')).toHaveAttribute('lang', 'en')
  await expect(page.getByRole('button', { name: 'Overview', exact: true })).toBeVisible()
})

test('maintenance search matches current-language descriptions after switching', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await page.getByRole('combobox', { name: '界面语言', exact: true }).selectOption('en')
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: 'Maintenance', exact: true }).click()
  await page.getByRole('textbox', { name: 'Search maintenance tasks' }).fill('thumbnail')
  await expect(page.getByText('Preview and icon caches', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Settings', exact: true }).click()
  await page.getByRole('combobox', { name: 'Interface language', exact: true }).selectOption('ja')
  await page.keyboard.press('Escape')
  await page.getByRole('textbox', { name: 'メンテナンス項目を検索' }).fill('サムネイル')
  await expect(page.getByText('プレビューとアイコンのキャッシュ', { exact: true })).toBeVisible()
})
