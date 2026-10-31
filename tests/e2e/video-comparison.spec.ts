import { test, expect, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installPersonalMock } from './fixtures/personal-files-native'
async function media(page: Page) {
  const data = readFileSync(new URL('../fixtures/media/owned-color.mp4', import.meta.url))
  await page.route('http://moomole-media.localhost/**', async (route) => {
    const match = /^bytes=(\d+)-(\d*)$/.exec(route.request().headers().range ?? '')
    const start = match ? Number(match[1]) : 0,
      end = match?.[2] ? Math.min(Number(match[2]), data.length - 1) : data.length - 1
    await route.fulfill({
      status: match ? 206 : 200,
      contentType: 'video/mp4',
      headers: {
        'accept-ranges': 'bytes',
        'content-range': `bytes ${start}-${end}/${data.length}`,
        'cache-control': 'no-store',
      },
      body: data.subarray(start, end + 1),
    })
  })
}
async function mount(page: Page, mode = 'normal', count = 2) {
  await media(page)
  await page.goto('/')
  await page.evaluate(
    async ({ mode, count }) =>
      (await import('/tests/e2e/fixtures/video-comparison.tsx')).mount(mode, count),
    { mode, count },
  )
  return page.getByRole('dialog', { name: '视频并排对比' })
}
test('two real browser videos play only on request, pause, seek, use one audio source and return exact choices', async ({
  page,
}) => {
  const modal = await mount(page)
  await expect(modal.getByRole('button', { name: '一起播放', exact: true })).toBeEnabled()
  const players = modal.locator('video')
  await expect(players).toHaveCount(2)
  expect(
    await players.evaluateAll((nodes) =>
      nodes.every(
        (n) =>
          (n as HTMLVideoElement).paused &&
          (n as HTMLVideoElement).muted &&
          (n as HTMLVideoElement).videoWidth === 160,
      ),
    ),
  ).toBe(true)
  await expect(modal.getByRole('checkbox').first()).not.toBeChecked()
  await modal.getByRole('button', { name: '一起播放', exact: true }).click()
  await expect
    .poll(() =>
      players.evaluateAll((nodes) =>
        Math.min(...nodes.map((n) => (n as HTMLVideoElement).currentTime)),
      ),
    )
    .toBeGreaterThan(0.05)
  await modal.getByRole('button', { name: '全部暂停', exact: true }).click()
  expect(
    await players.evaluateAll((nodes) => nodes.every((n) => (n as HTMLVideoElement).paused)),
  ).toBe(true)
  await modal.getByRole('slider', { name: '视频共同定位' }).fill('1')
  expect(
    await players.evaluateAll((nodes) =>
      nodes.every((n) => Math.abs((n as HTMLVideoElement).currentTime - 1) < 0.1),
    ),
  ).toBe(true)
  await modal.getByRole('radio', { name: '听取 clip-1.mp4' }).check()
  expect(
    await players.evaluateAll((nodes) => nodes.map((n) => (n as HTMLVideoElement).muted)),
  ).toEqual([false, true])
  await modal.getByRole('radio', { name: '听取 clip-2.mp4' }).check()
  expect(
    await players.evaluateAll((nodes) => nodes.map((n) => (n as HTMLVideoElement).muted)),
  ).toEqual([true, false])
  await modal.getByRole('checkbox', { name: '待处理 /Users/owned/clip-1.mp4' }).check()
  await modal.getByRole('button', { name: '将选择带回文件列表' }).click()
  await expect(modal).toHaveCount(0)
  await expect(page.locator('#video-comparison-fixture')).toHaveAttribute(
    'data-selected',
    '["00000001-1234-4123-8123-123456789abc"]',
  )
  const request = await page.locator('#video-comparison-fixture').getAttribute('data-request')
  await expect(page.locator('#video-comparison-fixture')).toHaveAttribute('data-released', request!)
})
test('four video cards stay within a compact viewport and cancel never applies a choice', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const modal = await mount(page, 'normal', 4)
  await expect(modal.getByRole('button', { name: '一起播放', exact: true })).toBeEnabled()
  expect(await modal.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await expect
    .poll(() =>
      modal
        .locator('video')
        .evaluateAll((nodes) => nodes.every((n) => (n as HTMLVideoElement).readyState >= 2)),
    )
    .toBe(true)
  await page.screenshot({ path: 'test-results/video-comparison-four.png', animations: 'disabled' })
  await modal.getByRole('checkbox').first().check()
  await modal.getByRole('button', { name: '取消对比', exact: true }).click()
  await expect(page.locator('#video-comparison-fixture')).toHaveAttribute('data-applied', 'false')
})
test('wrong members and late completion never enable selection or resume a closed comparison', async ({
  page,
}) => {
  let modal = await mount(page, 'mismatch')
  await expect(modal.getByRole('alert')).toContainText('预览结果与所选文件不一致')
  await expect(modal.getByRole('button', { name: '将选择带回文件列表' })).toBeDisabled()
  await expect(page.locator('#video-comparison-fixture')).toHaveAttribute(
    'data-released',
    (await page.locator('#video-comparison-fixture').getAttribute('data-request')) as string,
  )
  modal = await mount(page, 'late')
  await expect(modal.getByText('正在准备所选视频…')).toBeVisible()
  await modal.getByRole('button', { name: '取消对比', exact: true }).click()
  await expect(page.locator('#video-comparison-fixture')).toHaveAttribute('data-releases', '2')
  await expect(modal).toHaveCount(0)
})
test('expiration stops both players and revokes the comparison', async ({ page }) => {
  await page.clock.install()
  const modal = await mount(page, 'expiry')
  await expect(modal.getByRole('button', { name: '一起播放', exact: true })).toBeEnabled()
  await page.clock.fastForward(4000)
  await expect(modal.getByRole('alert')).toContainText('视频对比已过期')
  await expect(modal.getByRole('button', { name: '将选择带回文件列表' })).toBeDisabled()
  expect(
    await modal
      .locator('video')
      .evaluateAll((nodes) =>
        nodes.every((n) => (n as HTMLVideoElement).paused && !n.getAttribute('src')),
      ),
  ).toBe(true)
})
test('personal-file comparison choices return to the retained list and require a separate real-operation confirmation', async ({
  page,
}) => {
  await installPersonalMock(page, false, false, true)
  await media(page)
  await page.goto('/')
  await page.getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  const first = page
      .getByRole('article')
      .filter({ has: page.getByText('compare-1.mp4', { exact: true }) }),
    second = page
      .getByRole('article')
      .filter({ has: page.getByText('compare-2.mp4', { exact: true }) })
  await first.getByRole('button', { name: '加入视频对比' }).click()
  await second.getByRole('button', { name: '加入视频对比' }).click()
  await expect(first.getByRole('checkbox')).not.toBeChecked()
  await page.getByRole('button', { name: '并排对比 2 段视频' }).click()
  const modal = page.getByRole('dialog', { name: '视频并排对比' })
  await expect(modal.getByRole('button', { name: '一起播放', exact: true })).toBeEnabled()
  await modal.getByRole('checkbox', { name: '待处理 /Users/demo/Downloads/compare-1.mp4' }).check()
  await modal.getByRole('button', { name: '将选择带回文件列表' }).click()
  await expect(first.getByRole('checkbox')).toBeChecked()
  await expect(second.getByRole('checkbox')).not.toBeChecked()
  expect(await page.evaluate(() => sessionStorage.getItem('personal-executed'))).toBeNull()
  expect(await page.evaluate(() => sessionStorage.getItem('personal-selected'))).toBeNull()
  await page.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  const confirm = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(confirm).toContainText('compare-1.mp4')
  await expect(confirm).not.toContainText('compare-2.mp4')
  expect(await page.evaluate(() => sessionStorage.getItem('personal-executed'))).toBeNull()
  const request = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('video-comparison-request')!),
  )
  expect(request).toMatchObject({
    domain: 'personalFiles',
    scanId: '00000001-1234-4123-8123-123456789abc',
    itemIds: ['00000002-1234-4123-8123-123456789abc', '00000003-1234-4123-8123-123456789abc'],
  })
  expect(request).not.toHaveProperty('paths')
})
