import { expect, test, type Page } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { installPersonalMock } from './fixtures/personal-files-native'
const png =
  'data:image/png;base64,' +
  readFileSync(new URL('../fixtures/preview.png', import.meta.url)).toString('base64')
const movie = readFileSync(new URL('../fixtures/media/owned-color.mp4', import.meta.url))
async function calls(page: Page) {
  return page.evaluate(() => JSON.parse(sessionStorage.getItem('live-calls') ?? '[]')) as Promise<
    { command: string; args: Record<string, unknown> }[]
  >
}
async function setup(page: Page, mode = 'normal') {
  await installPersonalMock(page)
  await page.route('http://moomole-media.localhost/**', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'video/quicktime',
      headers: { 'Access-Control-Allow-Origin': '*' },
      body: movie,
    }),
  )
  await page.addInitScript(
    ({ mode, png }) => {
      const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
      const native = (window as any).__TAURI_INTERNALS__,
        original = native.invoke
      let release: (v: unknown) => void, late: unknown
      native.invoke = async (command: string, args: Record<string, unknown> = {}) => {
        const log = JSON.parse(sessionStorage.getItem('live-calls') ?? '[]')
        sessionStorage.setItem('live-calls', JSON.stringify([...log, { command, args }]))
        if (command === 'get_managed_apps') return []
        if (command === 'start_live_photo_scan') {
          if (args.root !== '/Users/demo/Downloads') throw Error('wrong source')
          const now = Date.now(),
            root = String(args.root)
          const items = ['First.jpg', 'First.mov', 'Second.heic', 'Second.mov'].map((name, n) => ({
            id: uuid(n + 2),
            name,
            path: `${root}/${name}`,
            size: 65536 * (n + 1),
            modifiedAt: now - (n < 2 ? 100 : 400) * 86400000,
          }))
          const report = {
            id: uuid(1),
            root,
            createdAt: now,
            expiresAt: now + 900000,
            complete: true,
            cancelled: false,
            skipped: 0,
            visited: 8,
            items,
            livePhotos: {
              pairs: [
                { id: uuid(10), photoId: uuid(2), movieId: uuid(3) },
                { id: uuid(11), photoId: uuid(4), movieId: uuid(5) },
              ],
              checkedCandidates: 3,
              rejectedCandidates: 1,
              unpairedFiles: 2,
              bytesRead: 655360,
            },
          }
          sessionStorage.setItem('personal-report', JSON.stringify(report))
          sessionStorage.setItem(
            'personal-state',
            JSON.stringify({
              phase: mode === 'scanCancel' ? 'scanning' : 'ready',
              requestId: args.requestId,
              runId: null,
              report: mode === 'scanCancel' ? null : report,
              error: null,
            }),
          )
          return
        }
        if (command === 'get_personal_files') {
          const state = JSON.parse(sessionStorage.getItem('personal-state') ?? 'null')
          if (state?.phase === 'processing' && mode !== 'cancel') {
            state.phase = 'completed'
            sessionStorage.setItem('personal-state', JSON.stringify(state))
          }
        }
        if (command === 'prepare_personal_trash') {
          const d = await original(command, args),
            r = JSON.parse(sessionStorage.getItem('personal-report')!)
          d.livePhotoPairs = r.livePhotos.pairs.filter(
            (p: any) =>
              (args.selectedIds as string[]).includes(p.photoId) &&
              (args.selectedIds as string[]).includes(p.movieId),
          )
          if (mode === 'mismatch') delete d.livePhotoPairs
          if (mode === 'late') {
            late = d
            return new Promise((resolve) => {
              release = resolve
            })
          }
          return d
        }
        if (command === 'fixture_release') {
          release(late)
          return
        }
        if (command === 'preview_personal_file' || command === 'prepare_media_preview') {
          const r = JSON.parse(sessionStorage.getItem('personal-report')!),
            i = r.items.find((i: any) => i.id === args.itemId)
          if (!i || args.scanId !== r.id || args.domain !== 'personalFiles')
            throw Error('wrong preview')
          return {
            scanId: r.id,
            itemId: i.id,
            path: i.path,
            content: i.name.endsWith('.mov')
              ? {
                  kind: 'media',
                  url: `http://moomole-media.localhost/${uuid(15)}/media`,
                  leaseId: uuid(15),
                  mime: 'video/quicktime',
                  mediaKind: 'video',
                  expiresAt: Date.now() + 60000,
                }
              : { kind: 'image', dataUrl: png, width: 16, height: 16, frames: 1 },
          }
        }
        if (command === 'release_media_preview') return
        const value = await original(command, args)
        if (command === 'get_execution')
          return { ...value, revision: 'MooMole.personal-live-photos.v1' }
        return value
      }
    },
    { mode, png },
  )
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('tab', { name: 'Live Photos', exact: true }).click()
  await page.getByRole('button', { name: '扫描 Live Photo 对', exact: true }).click()
  return page.getByRole('region', { name: 'Live Photo 配对资源' })
}
test('actual file organization tab selects both resources only after separate review and acknowledgement', async ({
  page,
}, info) => {
  const list = await setup(page)
  await expect(list.getByRole('article')).toHaveCount(2)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('button', { name: '审阅所选整对文件' }).click()
  const d = page.getByRole('dialog', { name: '确认处理 Live Photo 整对文件' })
  await expect(d).toContainText('First.jpg')
  await expect(d).toContainText('First.mov')
  await expect(d).not.toContainText('Second.heic')
  const sent = (await calls(page)).find((c) => c.command === 'prepare_personal_trash')!.args
  expect(sent.selectedIds).toEqual([
    '00000002-1234-4123-8123-123456789abc',
    '00000003-1234-4123-8123-123456789abc',
  ])
  expect(sent).not.toHaveProperty('path')
  const confirm = d.getByRole('button', { name: '确认移入所选整对文件' })
  await expect(confirm).toBeDisabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('live-confirm-zh.png'), animations: 'disabled' })
  await d.getByRole('checkbox').check()
  await confirm.click()
  const result = page.getByRole('region', { name: 'Live Photo 逐文件结果' })
  await expect(result).toContainText('已移入 2 / 2 份文件')
  await expect(result).toContainText('一份完成不代表整对完成')
  expect((await calls(page)).filter((c) => c.command === 'execute_personal_trash')).toHaveLength(1)
})
test('both resources preview without selection or automatic playback', async ({ page }) => {
  const list = await setup(page),
    row = list.getByRole('article').first()
  await row.getByRole('button', { name: '预览照片', exact: true }).click()
  let d = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(d.getByRole('img')).toBeVisible()
  await d.getByRole('button', { name: '返回文件列表' }).click()
  await row.getByRole('button', { name: '预览短视频', exact: true }).click()
  d = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(d.locator('video')).toBeVisible()
  expect(await d.locator('video').evaluate((v: HTMLVideoElement) => v.paused)).toBe(true)
  await d.getByRole('button', { name: '返回文件列表' }).click()
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
})
test('pair-level time filters preserve explicit choices while a changed root withdraws them', async ({
  page,
}) => {
  const list = await setup(page)
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('combobox', { name: 'Live Photo 修改时间' }).selectOption('365')
  await expect(list.getByRole('article')).toHaveCount(1)
  await expect(page.getByText('已选 1 对 · 2 份文件', { exact: true })).toBeVisible()
  await page.getByRole('textbox', { name: 'Live Photo 导出目录' }).fill('/Users/demo/Other')
  await expect(page.getByRole('button', { name: '审阅所选整对文件' })).toBeDisabled()
  await expect(page.getByText('已选 0 对 · 0 份文件', { exact: true })).toBeVisible()
})
test('stopping the scan cancels its exact request and retains only explicit partial observations', async ({
  page,
}) => {
  await setup(page, 'scanCancel')
  await page.getByRole('button', { name: '停止本次扫描或处理' }).click()
  await expect(
    page.getByText('本次未完整覆盖目录；未显示的资源保持原样，不能据此认定没有其他 Live Photos。', {
      exact: true,
    }),
  ).toBeVisible()
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'start_live_photo_scan')?.args.requestId,
  )
  expect(log.filter((c) => c.command === 'execute_personal_trash')).toHaveLength(0)
})
test('missing pair scope cannot enable processing', async ({ page }) => {
  const list = await setup(page, 'mismatch')
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('button', { name: '审阅所选整对文件' }).click()
  await expect(page.getByRole('alert')).toContainText('无法核对')
  await expect(page.getByRole('dialog', { name: '确认处理 Live Photo 整对文件' })).toHaveCount(0)
  expect((await calls(page)).filter((c) => c.command === 'execute_personal_trash')).toHaveLength(0)
})
test('confirmation expiry and closing withdraw exact preparation, never retry it', async ({
  page,
}) => {
  await page.clock.install()
  const list = await setup(page)
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('button', { name: '审阅所选整对文件' }).click()
  const d = page.getByRole('dialog', { name: '确认处理 Live Photo 整对文件' })
  await d.getByRole('checkbox').check()
  await page.clock.fastForward(31000)
  await expect(d.getByRole('button', { name: '确认移入所选整对文件' })).toBeDisabled()
  await d.getByRole('button', { name: '关闭弹窗' }).click()
  await expect
    .poll(async () => (await calls(page)).some((c) => c.command === 'discard_personal_trash'))
    .toBe(true)
})
test('late preparation after leaving the tab is discarded', async ({ page }) => {
  const list = await setup(page, 'late')
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('button', { name: '审阅所选整对文件' }).click()
  await page.getByRole('tab', { name: '普通文件', exact: true }).click()
  await page.evaluate(async () => {
    await (window as any).__TAURI_INTERNALS__.invoke('fixture_release')
  })
  await expect
    .poll(async () => (await calls(page)).some((c) => c.command === 'discard_personal_trash'))
    .toBe(true)
  expect((await calls(page)).filter((c) => c.command === 'execute_personal_trash')).toHaveLength(0)
})
test('stop records a partial pair and reopening reads the same result without replay', async ({
  page,
}) => {
  const list = await setup(page, 'cancel')
  await list.getByRole('article').first().getByRole('checkbox').check()
  await page.getByRole('button', { name: '审阅所选整对文件' }).click()
  const d = page.getByRole('dialog', { name: '确认处理 Live Photo 整对文件' })
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认移入所选整对文件' }).click()
  await page.getByRole('button', { name: '停止本次扫描或处理' }).click()
  await expect(page.getByRole('region', { name: 'Live Photo 逐文件结果' })).toContainText(
    '已移入 1 / 2 份文件',
  )
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'execute_personal_trash')?.args.requestId,
  )
  await page.reload()
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('tab', { name: 'Live Photos', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Live Photo 逐文件结果' })).toContainText(
    '已移入 1 / 2 份文件',
  )
  expect((await calls(page)).filter((c) => c.command === 'execute_personal_trash')).toHaveLength(1)
})
for (const lang of ['en', 'ja'] as const)
  test(`Live Photo review fits in ${lang}`, async ({ page }, info) => {
    const list = await setup(page)
    await list.getByRole('article').first().getByRole('checkbox').check()
    await page.getByRole('button', { name: '审阅所选整对文件' }).click()
    await page.evaluate(
      async (lang) => (await import('/src/i18n/locale.ts')).setLanguagePreference(lang),
      lang,
    )
    const d = page.getByRole('dialog')
    await expect(d).toContainText(
      lang === 'en'
        ? 'Confirm processing whole Live Photo pairs'
        : 'Live Photo ペア全体の処理を確認',
    )
    await expect(d.getByText('/Users/demo/Downloads/First.jpg', { exact: true })).toBeVisible()
    await expect(d.getByText('/Users/demo/Downloads/First.mov', { exact: true })).toBeVisible()
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`live-confirm-${lang}.png`),
      animations: 'disabled',
    })
  })
