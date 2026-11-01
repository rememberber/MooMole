import { expect, test, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installSimilarMock } from './fixtures/similar-photos-native'
async function open(page: Page) {
  await installSimilarMock(page)
  const data = readFileSync(new URL('../fixtures/media/owned-color.mp4', import.meta.url))
  await page.route('http://moomole-media.localhost/**', async (route) => {
    const range = /^bytes=(\d+)-(\d*)$/.exec(route.request().headers().range ?? '')
    const start = range ? Number(range[1]) : 0,
      end = range?.[2] ? Math.min(Number(range[2]), data.length - 1) : data.length - 1
    await route.fulfill({
      status: range ? 206 : 200,
      contentType: 'video/mp4',
      headers: {
        'accept-ranges': 'bytes',
        'content-range': `bytes ${start}-${end}/${data.length}`,
        'cache-control': 'no-store',
      },
      body: data.subarray(start, end + 1),
    })
  })
  await page.goto('/')
  await page
    .getByRole('navigation')
    .getByRole('button', { name: '相似照片与视频', exact: true })
    .click()
  const main = page.getByRole('main', { name: '相似照片与视频内容' })
  await main.getByRole('combobox', { name: '比较方式' }).selectOption('video')
  await main.getByRole('textbox', { name: '相似视频目录' }).fill('/Users/demo/Downloads')
  return main
}
test('video candidates show sample positions, play for comparison and retain a keeper through independent confirmation', async ({
  page,
}, info) => {
  const main = await open(page)
  await page.getByRole('button', { name: '查找相似视频', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list).toContainText('采样画面相似 · 3 段')
  await expect(list.getByRole('img', { name: /采样画面/ })).toHaveCount(9)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await list.screenshot({ path: info.outputPath('video-candidates.png'), animations: 'disabled' })
  await list.getByRole('button', { name: '对比本组视频' }).click()
  const compare = page.getByRole('dialog', { name: '视频并排对比' })
  await expect(compare.getByRole('button', { name: '一起播放', exact: true })).toBeEnabled()
  await expect(compare.locator('video')).toHaveCount(3)
  expect(
    await compare
      .locator('video')
      .evaluateAll((nodes) => nodes.every((n) => (n as HTMLVideoElement).paused)),
  ).toBe(true)
  await compare.getByRole('button', { name: '一起播放', exact: true }).click()
  await expect
    .poll(() =>
      compare
        .locator('video')
        .first()
        .evaluate((n) => (n as HTMLVideoElement).currentTime),
    )
    .toBeGreaterThan(0.05)
  await compare.getByRole('button', { name: '全部暂停', exact: true }).click()
  await compare.getByRole('checkbox').first().check()
  await compare.getByRole('button', { name: '将选择带回文件列表' }).click()
  const rows = list.getByRole('article')
  await expect(rows.first().getByRole('checkbox')).toBeChecked()
  await rows.nth(1).getByRole('checkbox').check()
  await expect(rows.nth(2).getByRole('checkbox')).toBeDisabled()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const confirm = page.getByRole('dialog', { name: '确认处理相似视频候选' })
  await expect(confirm).toContainText('本批保留的视频')
  await expect(confirm).toContainText('声音、字幕或未采样片段')
  await expect(confirm.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.find((c: any) => c.command === 'start_similar_scan').args).toMatchObject({
    videoCandidates: true,
    photoSequences: false,
  })
  expect(calls.find((c: any) => c.command === 'prepare_media_comparison').args.domain).toBe(
    'similarPhotos',
  )
  expect(
    calls.find((c: any) => c.command === 'prepare_similar_trash').args.selectedIds,
  ).toHaveLength(2)
  expect(calls.filter((c: any) => c.command === 'execute_similar_trash')).toHaveLength(1)
})
test('changing media modes withdraws video selections and does not process or scan automatically', async ({
  page,
}) => {
  const main = await open(page)
  await page.getByRole('button', { name: '查找相似视频', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await list.getByRole('checkbox').first().check()
  await main.getByRole('combobox', { name: '比较方式' }).selectOption('visual')
  await expect(main).toContainText('比较方式已变化，请重新扫描后选择。')
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toBeDisabled()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.filter((c: any) => c.command === 'start_similar_scan')).toHaveLength(1)
  expect(
    calls.some(
      (c: any) => c.command === 'prepare_similar_trash' || c.command === 'execute_similar_trash',
    ),
  ).toBe(false)
})

test('invalid sample evidence stays unselectable and a later read reconnects without rescanning', async ({
  page,
}) => {
  const main = await open(page)
  await page.evaluate(() => sessionStorage.setItem('video-bad-output', '1'))
  await page.getByRole('button', { name: '查找相似视频', exact: true }).click()
  await expect(main).toContainText('个人文件数据无法核对')
  await expect(main.getByRole('button', { name: '移入废纸篓…' })).toHaveCount(0)
  await page.evaluate(() => sessionStorage.removeItem('video-bad-output'))
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list).toContainText('采样画面相似 · 3 段')
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.filter((c: any) => c.command === 'start_similar_scan')).toHaveLength(1)
  expect(
    calls.some(
      (c: any) => c.command === 'prepare_similar_trash' || c.command === 'execute_similar_trash',
    ),
  ).toBe(false)
})
test('video mode and raw paths survive English and Japanese translation', async ({ page }) => {
  const main = await open(page)
  await page.getByRole('button', { name: '查找相似视频', exact: true }).click()
  for (const [language, label] of [
    ['en', 'Comparison mode'],
    ['ja', '比較方法'],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await expect(page.getByRole('combobox', { name: label, exact: true })).toHaveValue('video')
    await expect(page.getByText('~/Downloads/one.mp4', { exact: true })).toBeVisible()
  }
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.filter((c: any) => c.command === 'start_similar_scan')).toHaveLength(1)
  expect(calls.some((c: any) => c.command === 'execute_similar_trash')).toBe(false)
})
