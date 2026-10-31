import { expect, test } from '@playwright/test'
import { installSoftwareContentMock } from './fixtures/software-content-native'
async function open(page: import('@playwright/test').Page) {
  await installSoftwareContentMock(page)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '软件清理', exact: true }).click()
  await page.getByRole('tab', { name: '附件、下载与离线', exact: true }).click()
  return page.getByRole('main', { name: '软件清理内容' })
}
test('chat exports require a selected JSON manifest, scoped previews and separately confirmed attachments', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('telegramExport')
  const scan = page.getByRole('button', { name: '扫描聊天导出附件', exact: true })
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('export-picker-cancelled', '1'))
  await main.getByRole('button', { name: '选择聊天导出清单' }).click()
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('export-picker-cancelled'))
  await main.getByRole('button', { name: '选择聊天导出清单' }).click()
  await expect(main.getByRole('textbox', { name: '聊天导出来源' })).toHaveValue(
    '/Users/demo/Downloads/ChatExport/result.json',
  )
  await expect(main.getByRole('textbox', { name: '聊天导出附件目录' })).toHaveValue(
    '/Users/demo/Downloads/ChatExport',
  )
  await scan.click()
  await expect(main).toContainText('不扫描软件内部资料库')
  const list = main.getByRole('region', { name: '个人文件列表' })
  const notes = list.getByRole('article').filter({ hasText: 'notes.txt' })
  await notes.getByRole('button', { name: '预览内容' }).click()
  await expect(page.getByRole('dialog', { name: '文件内容预览' }).locator('pre')).toHaveText(
    'owned attachment preview',
  )
  await page.getByRole('button', { name: '返回文件列表' }).click()
  await notes.getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理聊天导出附件' })
  await expect(dialog).toContainText('导出正文清单保留')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  const started = calls.find((c: any) => c.command === 'start_software_content')
  expect(started.args).toMatchObject({
    source: 'telegramExport',
    exportPath: '/Users/demo/Downloads/ChatExport/result.json',
  })
  expect(started.args).not.toHaveProperty('playlistPath')
  expect(
    calls.find((c: any) => c.command === 'prepare_software_trash').args.selectedIds,
  ).toHaveLength(1)
})
test('changing chat exports withdraws old selections even within the same account root', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('telegramExport')
  await main.getByRole('button', { name: '选择聊天导出清单' }).click()
  await page.getByRole('button', { name: '扫描聊天导出附件', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await list.getByRole('checkbox').first().check()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeEnabled()
  await page.evaluate(() =>
    sessionStorage.setItem('export-picker-path', '/Users/demo/Downloads/OtherExport/result.json'),
  )
  await main.getByRole('button', { name: '选择聊天导出清单' }).click()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await page.getByRole('button', { name: '扫描聊天导出附件', exact: true }).click()
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
})
test('protected modern Mail content is previewable and cannot be selected or submitted', async ({
  page,
}) => {
  const main = await open(page)
  await page.getByRole('button', { name: '扫描附件副本', exact: true }).click()
  await expect(main).toContainText('受到 Mole 核心规则保护，仅供预览')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await list
    .getByRole('article')
    .filter({ hasText: 'notes.txt' })
    .getByRole('button', { name: '预览内容' })
    .click()
  await expect(page.getByRole('dialog', { name: '文件内容预览' }).locator('pre')).toHaveText(
    'owned attachment preview',
  )
  await page.getByRole('button', { name: '返回文件列表' }).click()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some(
      (c: any) => c.command === 'prepare_software_trash' || c.command === 'execute_software_trash',
    ),
  ).toBe(false)
})
test('supported attachment copies use catalog sources, filters and one explicit confirmed request', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('mailDownloads')
  await page.getByRole('button', { name: '扫描附件副本', exact: true }).click()
  await expect(main.getByRole('textbox', { name: '软件附件目录' })).toHaveAttribute('readonly', '')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await main.getByRole('searchbox', { name: '搜索个人文件' }).fill('notes')
  await list.getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理软件附件副本' })
  await expect(dialog).toContainText('/Users/demo/Library/Mail Downloads/notes.txt')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  const start = calls.find((c: any) => c.command === 'start_software_content')
  expect(start.args.source).toBe('mailDownloads')
  expect(start.args).not.toHaveProperty('root')
  expect(
    calls.find((c: any) => c.command === 'prepare_software_trash').args.selectedIds,
  ).toHaveLength(1)
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})

test('browser downloads keep their source explicit and process only separately confirmed local files', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  await page.getByRole('button', { name: '扫描下载记录', exact: true }).click()
  await expect(main.getByRole('textbox', { name: '下载文件范围' })).toHaveValue('/Users/demo')
  await expect(main).toContainText('不遍历整个账户目录')
  await expect(main).toContainText('浏览器历史和下载记录保持原样')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await list.getByRole('article').filter({ hasText: 'notes.txt' }).getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理浏览器下载' })
  await expect(dialog).toContainText('/Users/demo/Downloads/notes.txt')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(calls.find((c: any) => c.command === 'start_software_content').args.source).toBe(
    'chromeDownloads',
  )
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})

test('offline media requires a selected playlist and one separately confirmed file selection', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('vlcPlaylist')
  const scan = page.getByRole('button', { name: '扫描离线影音', exact: true })
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('playlist-picker-cancelled', '1'))
  await main.getByRole('button', { name: '选择播放列表', exact: true }).click()
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('playlist-picker-cancelled'))
  await main.getByRole('button', { name: '选择播放列表', exact: true }).click()
  await expect(main.getByRole('textbox', { name: '播放列表来源' })).toHaveValue(
    '/Users/demo/Movies/offline.m3u8',
  )
  await scan.click()
  await expect(main).toContainText('播放列表及播放器数据库保持原样')
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await list.getByRole('article').filter({ hasText: 'recording.wav' }).getByRole('checkbox').check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理离线影音文件' })
  await expect(dialog).toContainText('/Users/demo/Movies/recording.wav')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(calls.find((c: any) => c.command === 'start_software_content').args).toMatchObject({
    source: 'vlcPlaylist',
    playlistPath: '/Users/demo/Movies/offline.m3u8',
  })
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})

test('changing a playlist or source withdraws previous file selection until rescanned', async ({
  page,
}) => {
  const main = await open(page)
  const source = main.getByRole('combobox', { name: '软件内容来源' })
  await source.selectOption('vlcPlaylist')
  await main.getByRole('button', { name: '选择播放列表', exact: true }).click()
  await page.getByRole('button', { name: '扫描离线影音', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await list.getByRole('checkbox').first().check()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeEnabled()
  await page.evaluate(() =>
    sessionStorage.setItem('playlist-picker-path', '/Users/demo/Movies/other.m3u8'),
  )
  await main.getByRole('button', { name: '选择播放列表', exact: true }).click()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await source.selectOption('chromeDownloads')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some(
      (c: any) => c.command === 'execute_software_trash' || c.command === 'prepare_software_trash',
    ),
  ).toBe(false)
})

test('multi-browser profiles bind scans and confirmations and changing a profile withdraws old choices', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  const browser = main.getByRole('combobox', { name: '浏览器', exact: true })
  await expect(browser).toBeEnabled()
  await browser.selectOption('edge')
  const profile = main.getByRole('combobox', { name: '浏览器配置目录' })
  await expect(profile).toBeEnabled()
  await profile.selectOption('Profile 2')
  await expect(main).toContainText('/Microsoft Edge/Profile 2/History')
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.screenshot({ path: '.cache/browser-downloads-picker.png' })
  await page.getByRole('button', { name: '扫描下载记录', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await list.getByRole('checkbox').first().check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理浏览器下载' })
  await expect(dialog).toContainText('Microsoft Edge · Profile 2')
  await page.screenshot({
    path: '.cache/browser-downloads-confirmation.png',
    animations: 'disabled',
  })
  await dialog.getByRole('button', { name: '返回选择' }).click()
  await profile.selectOption('Default')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await page.getByRole('button', { name: '扫描下载记录', exact: true }).click()
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await list.getByRole('checkbox').first().check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  await expect(dialog).toContainText('Microsoft Edge · Default')
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
    { browser: 'edge', profile: 'Profile 2' },
    { browser: 'edge', profile: 'Default' },
  ])
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
})

test('unknown and failed profile reads block scanning and refresh does not submit a file operation', async ({
  page,
}) => {
  const main = await open(page)
  await page.evaluate(() => sessionStorage.setItem('browser-profile-error', '1'))
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  await expect(main.getByRole('alert')).toContainText('配置读取失败')
  await expect(page.getByRole('button', { name: '扫描下载记录', exact: true })).toBeDisabled()
  await page.evaluate(() => {
    sessionStorage.removeItem('browser-profile-error')
    sessionStorage.setItem('browser-profile-empty', '1')
    sessionStorage.setItem('browser-profile-incomplete', '1')
  })
  await main.getByRole('button', { name: '重新检测配置' }).click()
  await expect(main).toContainText('这不代表浏览器没有下载文件')
  await expect(main).toContainText('配置检查未完整覆盖')
  await expect(page.getByRole('button', { name: '扫描下载记录', exact: true })).toBeDisabled()
  await page.evaluate(() => {
    sessionStorage.removeItem('browser-profile-empty')
    sessionStorage.removeItem('browser-profile-incomplete')
  })
  await main.getByRole('button', { name: '重新检测配置' }).click()
  await expect(page.getByRole('button', { name: '扫描下载记录', exact: true })).toBeEnabled()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some((c: any) =>
      ['start_software_content', 'execute_software_trash', 'prepare_software_trash'].includes(
        c.command,
      ),
    ),
  ).toBe(false)
})

test('profile controls translate while keeping browser IDs, profile names and raw paths', async ({
  page,
}) => {
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  const browser = main.getByRole('combobox', { name: '浏览器', exact: true })
  await expect(browser).toBeEnabled()
  await browser.selectOption('edge')
  const profile = main.getByRole('combobox', { name: '浏览器配置目录' })
  await expect(profile).toBeEnabled()
  await profile.selectOption('Profile 2')
  for (const [language, label] of [
    ['en', 'Browser profile folder'],
    ['ja', 'ブラウザのプロファイルフォルダ'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await expect(page.getByRole('combobox', { name: label })).toHaveValue('Profile 2')
    await expect(
      page.getByText('/Users/demo/Library/Application Support/Microsoft Edge/Profile 2/History', {
        exact: true,
      }),
    ).toBeVisible()
  }
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some(
      (c: any) => c.command === 'start_software_content' || c.command === 'execute_software_trash',
    ),
  ).toBe(false)
})
