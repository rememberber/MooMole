import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function open(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/mounted-volumes.tsx')).mount(mode),
    mode,
  )
  await page.getByRole('button', { name: '选择磁盘', exact: true }).click()
  return page.getByRole('dialog', { name: '选择磁盘分析' })
}
test('mounted-volume selection shows capacity and read-only boundaries and resolves the exact selected ID once', async ({
  page,
}) => {
  const dialog = await open(page)
  await expect(dialog).toContainText('当前账户目录')
  await expect(dialog).toContainText('APFS 卷可能共享空间')
  await expect(dialog.getByRole('button', { name: '分析 暂不可读', exact: true })).toBeDisabled()
  await expect(page.locator('#mounted-volume-fixture')).not.toHaveAttribute(
    'data-calls',
    /resolve_mounted_volume/,
  )
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/mounted-volumes-${theme}.png`,
      animations: 'disabled',
    })
  }
  await dialog.getByRole('button', { name: '分析 中文 外接盘', exact: true }).click()
  await expect(page.locator('#mounted-volume-fixture')).toHaveAttribute(
    'data-selected',
    '/Volumes/中文 外接盘',
  )
  const calls = JSON.parse(
    (await page.locator('#mounted-volume-fixture').getAttribute('data-calls'))!,
  )
  expect(calls.filter((c: { command: string }) => c.command === 'resolve_mounted_volume')).toEqual([
    { command: 'resolve_mounted_volume', args: { id: '41d8c490-1245-414b-8d62-0c6aab303555' } },
  ])
})
test('a read-only volume remains available for analysis without implying cleanup permission', async ({
  page,
}) => {
  const dialog = await open(page)
  await expect(dialog).toContainText('只读卷')
  await dialog.getByRole('button', { name: '分析 只读镜像', exact: true }).click()
  await expect(page.locator('#mounted-volume-fixture')).toHaveAttribute(
    'data-selected',
    '/Volumes/只读镜像',
  )
})
test('startup root and account directory remain distinct analysis targets', async ({ page }) => {
  const dialog = await open(page)
  await dialog.getByRole('button', { name: '分析 当前账户目录', exact: true }).click()
  await expect(page.locator('#mounted-volume-fixture')).toHaveAttribute(
    'data-selected',
    '/Users/demo',
  )
  await page.getByRole('button', { name: '选择磁盘', exact: true }).click()
  await dialog.getByRole('button', { name: '分析 启动卷根目录', exact: true }).click()
  await expect(page.locator('#mounted-volume-fixture')).toHaveAttribute('data-selected', '/')
})
for (const mode of ['changed', 'mismatch'])
  test(`a ${mode} mount never starts analysis or silently retries`, async ({ page }) => {
    const dialog = await open(page, mode)
    await dialog.getByRole('button', { name: '分析 中文 外接盘', exact: true }).click()
    await expect(dialog.getByRole('alert')).toBeVisible()
    await expect(page.locator('#mounted-volume-fixture')).not.toHaveAttribute('data-selected')
    await expect(dialog.getByRole('button', { name: '分析 中文 外接盘', exact: true })).toHaveCount(
      0,
    )
    await dialog.getByRole('button', { name: '刷新磁盘列表' }).click()
    await expect(page.locator('#mounted-volume-fixture')).toHaveAttribute('data-reads', '2')
  })
test('unknown capacity is retained as unknown and partial inventory remains explicit', async ({
  page,
}) => {
  const dialog = await open(page, 'unknown')
  const row = dialog
    .getByRole('listitem')
    .filter({ has: page.getByRole('button', { name: '分析 中文 外接盘', exact: true }) })
  await expect(row).toContainText('可用 — / 总量 —')
  await dialog.getByRole('button', { name: '返回空间分析' }).click()
  await expect(page.locator('#mounted-volume-fixture')).not.toHaveAttribute('data-selected')
  await page.reload()
  const partial = await open(page, 'partial')
  await expect(partial.getByRole('status')).toContainText('磁盘列表不完整')
})
test('invalid capacity cannot become an analyzable disk list', async ({ page }) => {
  const dialog = await open(page, 'bad')
  await expect(dialog.getByRole('alert')).toContainText('磁盘列表无法核对')
  await expect(dialog.getByRole('button', { name: '分析 中文 外接盘', exact: true })).toHaveCount(0)
})
test('the actual app toolbar analyzes the resolved mounted root and hands it to fresh file organization', async ({
  page,
}) => {
  await installTaskMock(page, 'navigation')
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.evaluate(() => {
    const internals = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (c: string, a?: Record<string, unknown>) => Promise<unknown>
          }
        }
      ).__TAURI_INTERNALS__,
      original = internals.invoke
    internals.invoke = async (c, a) => {
      if (c === 'get_mounted_volumes') {
        const createdAt = Date.now() - 1000
        return {
          version: 1,
          createdAt,
          expiresAt: createdAt + 120_000,
          complete: true,
          volumes: [
            {
              id: '41d8c490-1245-414b-8d62-0c6aab303555',
              kind: 'mounted',
              name: 'fixture-disk',
              path: '/Volumes/fixture-disk',
              fileSystem: 'apfs',
              readOnly: false,
              totalBytes: 200_000_000,
              availableBytes: 60_000_000,
              canAnalyze: true,
            },
          ],
        }
      }
      if (c === 'resolve_mounted_volume') return '/Volumes/fixture-disk'
      return original(c, a)
    }
  })
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '空间分析', exact: true })
    .click()
  await page.getByRole('button', { name: '选择磁盘', exact: true }).click()
  await page
    .getByRole('dialog', { name: '选择磁盘分析' })
    .getByRole('button', { name: '分析 fixture-disk', exact: true })
    .click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await expect(explorer).toBeVisible()
  await explorer.getByRole('button', { name: '整理当前目录', exact: true }).click()
  await expect(
    page.getByRole('main', { name: '文件整理内容' }).getByRole('textbox', { name: '个人文件目录' }),
  ).toHaveValue('/Volumes/fixture-disk')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-native-commands')!),
  )
  expect(
    calls.filter((c: { command: string }) => c.command === 'start_personal_scan'),
  ).toHaveLength(0)
  const starts = await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts')!))
  expect(starts.filter((r: { kind: string }) => r.kind === 'analyze')).toMatchObject([
    { path: '/Volumes/fixture-disk' },
  ])
})
