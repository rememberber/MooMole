import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'

async function setup(page: Page, many = false, hold = false) {
  const apps = many
    ? Array.from({ length: 115 }, (_, index) => ({
        ...applications[0],
        name: `App ${String(index).padStart(3, '0')}`,
        path: `/fixture/${index}.app`,
      }))
    : applications
  await installTaskMock(page, 'apps', undefined, apps)
  await page.addInitScript(
    ({ hold }) => {
      const internal = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
          }
        }
      ).__TAURI_INTERNALS__
      const previous = internal.invoke
      internal.invoke = async (command, args) => {
        if (command !== 'application_icon') return previous(command, args)
        const calls = JSON.parse(sessionStorage.getItem('icon-calls') ?? '[]')
        calls.push(args)
        sessionStorage.setItem('icon-calls', JSON.stringify(calls))
        const id = String(args?.taskId)
        if (hold && !sessionStorage.getItem('icon-old-task'))
          sessionStorage.setItem('icon-old-task', id)
        while (
          hold &&
          id === sessionStorage.getItem('icon-old-task') &&
          !sessionStorage.getItem('icon-release')
        )
          await new Promise((resolve) => setTimeout(resolve, 20))
        if (args?.index === 1) throw new Error('fixture optional artwork unavailable')
        if (args?.index === 2) return 'https://untrusted.invalid/icon.png'
        if (args?.index === 3) return 'data:image/png;base64,aGVsbG8='
        const canvas = document.createElement('canvas')
        canvas.width = 64
        canvas.height = 64
        const context = canvas.getContext('2d')!
        const old = id === sessionStorage.getItem('icon-old-task')
        context.fillStyle = old ? '#cc0000' : '#008855'
        context.fillRect(0, 0, 64, 64)
        const image = canvas.toDataURL('image/png')
        sessionStorage.setItem(old ? 'icon-old-value' : 'icon-current-value', image)
        return image
      }
    },
    { hold },
  )
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await expect(page.getByText('应用列表进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(many ? 100 : 5)
  return library
}

test('native artwork is optional, uses task indices and keeps fallbacks and comparison usable in both themes', async ({
  page,
}) => {
  const external: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('untrusted.invalid')) external.push(request.url())
  })
  const library = await setup(page)
  const first = library.getByRole('article', {
    name: 'Example · /Applications/Example.app',
    exact: true,
  })
  await expect(first.locator('.app-icon img')).toBeVisible()
  const canvasSize = await first
    .locator('.app-icon img')
    .evaluate((image: HTMLImageElement) => [image.naturalWidth, image.naturalHeight])
  expect(canvasSize).toEqual([64, 64])
  for (const index of [1, 2, 3]) {
    const row = library.getByRole('article', {
      name: `${applications[index].name} · ${applications[index].path}`,
      exact: true,
    })
    await row.scrollIntoViewIfNeeded()
    await expect
      .poll(() =>
        page.evaluate(
          (index) =>
            JSON.parse(sessionStorage.getItem('icon-calls') ?? '[]').some(
              (call: { index: number }) => call.index === index,
            ),
          index,
        ),
      )
      .toBe(true)
    await expect(row.locator('.app-icon svg')).toBeVisible()
    await expect(row.locator('.app-icon img')).toHaveCount(0)
    await row.getByRole('checkbox').check()
  }
  await expect(library.getByRole('status')).toContainText('已勾选 3 项')
  expect(external).toEqual([])
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('icon-calls') ?? '[]'))
  expect(
    calls.every(
      (call: Record<string, unknown>) => Object.keys(call).sort().join(',') === 'index,taskId',
    ),
  ).toBe(true)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await page.evaluate(() => window.scrollTo(0, 0))
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-icons-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('only nearby rows request artwork and filtering keeps the original Mole index', async ({
  page,
}) => {
  const library = await setup(page, true)
  await expect(library.getByRole('article').first().locator('.app-icon img')).toBeVisible()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('icon-calls') ?? '[]'))
  expect(calls.length).toBeLessThan(25)
  expect(calls.every((call: { index: number }) => call.index < 25)).toBe(true)
  await library.getByRole('textbox', { name: '搜索应用' }).fill('App 114')
  await expect(library.getByRole('article')).toHaveCount(1)
  await expect(library.getByRole('article').locator('.app-icon img')).toBeVisible()
  await expect
    .poll(() =>
      page.evaluate(() =>
        JSON.parse(sessionStorage.getItem('icon-calls') ?? '[]').some(
          (call: { index: number }) => call.index === 114,
        ),
      ),
    )
    .toBe(true)
  await library.getByRole('combobox', { name: '应用排序' }).selectOption('name')
  await expect(library.getByRole('article')).toContainText('App 114')
})

test('a new list never shows a delayed icon belonging to the previous task', async ({ page }) => {
  const library = await setup(page, false, true)
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('icon-old-task')))
    .not.toBeNull()
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toHaveCount(0)
  await expect(library.locator('.app-icon img')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('icon-release', 'yes'))
  const first = library.getByRole('article', {
    name: 'Example · /Applications/Example.app',
    exact: true,
  })
  await expect(first.locator('.app-icon img')).toBeVisible()
  const values = await page.evaluate(() => ({
    old: sessionStorage.getItem('icon-old-value'),
    current: sessionStorage.getItem('icon-current-value'),
  }))
  expect(values.old).toBeTruthy()
  expect(values.current).toBeTruthy()
  expect(values.old).not.toBe(values.current)
  await expect(first.locator('.app-icon img')).toHaveAttribute('src', values.current!)
  expect(await first.locator('.app-icon img').getAttribute('src')).not.toBe(values.old)
})
