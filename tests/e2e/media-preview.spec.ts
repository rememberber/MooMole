import { test, expect, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
function wav() {
  const size = 32000
  const b = Buffer.alloc(44 + size)
  b.write('RIFF')
  b.writeUInt32LE(36 + size, 4)
  b.write('WAVEfmt ', 8)
  b.writeUInt32LE(16, 16)
  b.writeUInt16LE(1, 20)
  b.writeUInt16LE(1, 22)
  b.writeUInt32LE(8000, 24)
  b.writeUInt32LE(16000, 28)
  b.writeUInt16LE(2, 32)
  b.writeUInt16LE(16, 34)
  b.write('data', 36)
  b.writeUInt32LE(size, 40)
  return b
}
async function mount(page: Page, kind: 'audio' | 'video', mode = 'normal') {
  const data =
    kind === 'video'
      ? readFileSync(new URL('../fixtures/media/owned-color.mp4', import.meta.url))
      : wav()
  await page.route('http://moomole-media.localhost/**', async (route) => {
    const range = route.request().headers()['range']
    let start = 0,
      end = data.length - 1
    if (range) {
      const m = /^bytes=(\d+)-(\d*)$/.exec(range)
      if (m) {
        start = Number(m[1])
        if (m[2]) end = Math.min(end, Number(m[2]))
      }
    }
    if (start >= data.length) {
      await route.fulfill({ status: 416, headers: { 'content-range': `bytes */${data.length}` } })
      return
    }
    await route.fulfill({
      status: range ? 206 : 200,
      contentType: kind === 'video' ? 'video/mp4' : 'audio/wav',
      headers: {
        'accept-ranges': 'bytes',
        'content-range': `bytes ${start}-${end}/${data.length}`,
        'cache-control': 'no-store',
      },
      body: data.subarray(start, end + 1),
    })
  })
  await page.goto('/')
  await page.evaluate(
    async ({ kind, mode }) =>
      (await import('/tests/e2e/fixtures/media-preview.tsx')).mount(kind, mode),
    { kind, mode },
  )
  return page.getByRole('dialog', { name: '文件内容预览' })
}
for (const kind of ['audio', 'video'] as const)
  test(`${kind} plays only on request, pauses, seeks and releases on close`, async ({ page }) => {
    const modal = await mount(page, kind)
    const play = modal.getByRole('button', { name: '播放', exact: true })
    await expect(play).toBeEnabled()
    const player = modal.locator(kind === 'video' ? 'video' : 'audio')
    expect(await player.evaluate((el: HTMLMediaElement) => el.paused)).toBe(true)
    if (kind === 'video')
      expect(await player.evaluate((el: HTMLVideoElement) => el.videoWidth)).toBe(160)
    await play.click()
    await expect
      .poll(() => player.evaluate((el: HTMLMediaElement) => el.currentTime))
      .toBeGreaterThan(0.05)
    await modal.getByRole('button', { name: '暂停', exact: true }).click()
    expect(await player.evaluate((el: HTMLMediaElement) => el.paused)).toBe(true)
    await modal.getByRole('slider', { name: '播放位置' }).fill('1')
    await expect
      .poll(() => player.evaluate((el: HTMLMediaElement) => el.currentTime))
      .toBeGreaterThanOrEqual(0.9)
    await modal.getByRole('button', { name: '返回文件列表' }).click()
    await expect(modal).toHaveCount(0)
    await expect(page.locator('#media-preview-fixture')).toHaveAttribute(
      'data-releases',
      /12345678-1234-4123-8123-123456789abc/,
    )
  })
test('expiry stops the source and closing before preparation finishes still revokes the late capability', async ({
  page,
}) => {
  await page.clock.install()
  let modal = await mount(page, 'audio', 'expiry')
  await expect(modal.getByRole('button', { name: '播放', exact: true })).toBeEnabled()
  await page.clock.fastForward(4000)
  await expect(modal.getByRole('alert')).toContainText('影音预览已过期')
  await expect(modal.getByRole('button', { name: '播放', exact: true })).toBeDisabled()
  await expect(page.locator('#media-preview-fixture')).toHaveAttribute('data-releases', /12345678/)
  await page.clock.resume()
  modal = await mount(page, 'audio', 'late')
  await expect(modal.getByText('正在读取预览…')).toBeVisible()
  await modal.getByRole('button', { name: '返回文件列表' }).click()
  await expect(page.locator('#media-preview-fixture')).toHaveAttribute('data-releases', /12345678/)
})
