import { expect, test } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installSoftwareContentMock } from './fixtures/software-content-native'
async function open(page: import('@playwright/test').Page) {
  await installSoftwareContentMock(page)
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '软件清理', exact: true }).click()
  await page.getByRole('tab', { name: '附件、下载与离线', exact: true }).click()
  return page.getByRole('main', { name: '软件清理内容' })
}
test('Music local media requires a selected XML, previews exact files and executes only after independent confirmation', async ({
  page,
}, info) => {
  const data = readFileSync(new URL('../fixtures/media/owned-color.mp4', import.meta.url))
  await page.route('http://moomole-media.localhost/**', async (route) => {
    const range = route.request().headers()['range'],
      match = range?.match(/^bytes=(\d+)-(\d*)$/)
    const start = match ? Number(match[1]) : 0,
      end = match?.[2] ? Math.min(Number(match[2]), data.length - 1) : data.length - 1
    await route.fulfill({
      status: range ? 206 : 200,
      contentType: 'video/mp4',
      headers: {
        'accept-ranges': 'bytes',
        'content-range': `bytes ${start}-${end}/${data.length}`,
      },
      body: data.subarray(start, end + 1),
    })
  })
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('musicLibrary')
  const scan = page.getByRole('button', { name: '扫描 Music 本地媒体', exact: true })
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('music-picker-cancelled', '1'))
  await main.getByRole('button', { name: '选择 Music 导出资料库', exact: true }).click()
  await expect(scan).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('music-picker-cancelled'))
  await main.getByRole('button', { name: '选择 Music 导出资料库', exact: true }).click()
  await expect(main.getByRole('textbox', { name: 'Music 导出资料库来源' })).toHaveValue(
    '/Users/demo/Exports/Library.xml',
  )
  await scan.click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await list.getByRole('button', { name: '预览内容' }).first().click()
  const preview = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(preview.locator('video')).toBeVisible()
  await expect
    .poll(() => preview.locator('video').evaluate((e: HTMLVideoElement) => e.videoWidth))
    .toBe(160)
  await expect(preview.getByRole('button', { name: '播放', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: '返回文件列表' }).click()
  await list.getByRole('checkbox').first().check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const d = page.getByRole('dialog', { name: '确认处理 Music 本地媒体', exact: true })
  await expect(d).toContainText('可能使 Music 中的引用失效')
  await expect(d.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('music-confirm.png'), animations: 'disabled' })
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(calls.find((c: any) => c.command === 'start_software_content').args).toMatchObject({
    source: 'musicLibrary',
    playlistPath: '/Users/demo/Exports/Library.xml',
  })
  expect(calls.filter((c: any) => c.command === 'execute_software_trash')).toHaveLength(1)
  expect(
    calls.find((c: any) => c.command === 'prepare_software_trash').args.selectedIds,
  ).toHaveLength(1)
})
test('changing Music exports or switching back from VLC withdraws old choices under the same account root', async ({
  page,
}) => {
  const main = await open(page)
  const source = main.getByRole('combobox', { name: '软件内容来源' })
  await source.selectOption('musicLibrary')
  await main.getByRole('button', { name: '选择 Music 导出资料库', exact: true }).click()
  await page.getByRole('button', { name: '扫描 Music 本地媒体', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await list.getByRole('checkbox').first().check()
  await page.evaluate(() =>
    sessionStorage.setItem('music-picker-path', '/Users/demo/Exports/Other.xml'),
  )
  await main.getByRole('button', { name: '选择 Music 导出资料库', exact: true }).click()
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await page.getByRole('button', { name: '扫描 Music 本地媒体', exact: true }).click()
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await source.selectOption('vlcPlaylist')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await source.selectOption('musicLibrary')
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
})
for (const [language, sourceLabel, picker, scan] of [
  [
    'en',
    'Local media from a Music library export',
    'Choose Music library export',
    'Scan Music local media',
  ],
  [
    'ja',
    'Music のライブラリ書き出し内のローカルメディア',
    'Music の書き出したライブラリを選択',
    'Music のローカルメディアをスキャン',
  ],
])
  test(`${language} Music export scope keeps exact paths and defaults to zero selection`, async ({
    page,
  }, info) => {
    const main = await open(page)
    await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('musicLibrary')
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await expect(page.getByRole('option', { name: sourceLabel, exact: true })).toHaveCount(1)
    await page.getByRole('button', { name: picker, exact: true }).click()
    await page.getByRole('button', { name: scan, exact: true }).click()
    await expect(page.locator('input[readonly]').first()).toBeVisible()
    await expect(page.locator('input[value="/Users/demo/Exports/Library.xml"]')).toBeVisible()
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`music-${language}.png`),
      animations: 'disabled',
    })
  })
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

test('Firefox requires an explicit profile and confirms only the newly scanned profile files', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const main = await open(page)
  await main.getByRole('combobox', { name: '软件内容来源' }).selectOption('chromeDownloads')
  await main.getByRole('combobox', { name: '浏览器', exact: true }).selectOption('firefox')
  const profile = main.getByRole('combobox', { name: '浏览器配置目录' })
  await expect(profile).toBeEnabled()
  await expect(profile).toHaveValue('')
  const scan = page.getByRole('button', { name: '扫描下载记录', exact: true })
  await expect(scan).toBeDisabled()
  await profile.selectOption('abcd1234.default-release')
  await expect(main).toContainText('/Firefox/Profiles/abcd1234.default-release/places.sqlite')
  await scan.click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await list.getByRole('checkbox').first().check()
  await profile.selectOption('efgh5678.work')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  await expect(list.getByRole('checkbox').first()).toBeDisabled()
  await scan.click()
  await expect(list.getByRole('checkbox').first()).not.toBeChecked()
  await list.getByRole('checkbox').first().check()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理浏览器下载' })
  await expect(dialog).toContainText('Firefox · efgh5678.work')
  await expect(dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.screenshot({
    path: testInfo.outputPath('firefox-confirmation.png'),
    animations: 'disabled',
  })
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
    { browser: 'firefox', profile: 'abcd1234.default-release' },
    { browser: 'firefox', profile: 'efgh5678.work' },
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
  await page.getByRole('combobox', { name: 'ブラウザ', exact: true }).selectOption('firefox')
  const firefoxProfile = page.getByRole('combobox', { name: 'ブラウザのプロファイルフォルダ' })
  await expect(firefoxProfile).toBeEnabled()
  await expect(firefoxProfile).toHaveValue('')
  await firefoxProfile.selectOption('abcd1234.default-release')
  await expect(
    page.getByText(
      '/Users/demo/Library/Application Support/Firefox/Profiles/abcd1234.default-release/places.sqlite',
      { exact: true },
    ),
  ).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('software-content-calls')!),
  )
  expect(
    calls.some(
      (c: any) => c.command === 'start_software_content' || c.command === 'execute_software_trash',
    ),
  ).toBe(false)
})
