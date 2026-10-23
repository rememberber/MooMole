import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import type { TaskRecord } from '../../src/types'

const task = (
  path: string | null,
  index: number,
  state = 'succeeded',
  kind = 'analyze',
): TaskRecord => ({
  id: `recent-${index}`,
  path,
  kind,
  state,
  createdAt: 1000 - index,
  finishedAt: 2000 - index,
  message: null,
})
const recent = (page: Page) => page.getByRole('combobox', { name: '最近分析目录', exact: true })
const pathInput = (page: Page) => page.getByRole('textbox', { name: '扫描目录', exact: true })
const startButton = (page: Page) => page.getByRole('button', { name: '开始分析', exact: true })
const result = (page: Page) => page.getByRole('region', { name: 'Mole 空间分析结果' })

async function setup(page: Page, records: TaskRecord[] = []) {
  await installTaskMock(page, 'navigation')
  await page.addInitScript((records) => {
    if (!sessionStorage.getItem('recent-archive'))
      sessionStorage.setItem('recent-archive', JSON.stringify(records))
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const previous = internal.invoke
    const merge = (current: TaskRecord[]) => {
      const archive = JSON.parse(sessionStorage.getItem('recent-archive')!) as TaskRecord[]
      const ids = new Set(current.map((record) => record.id))
      const merged = [...current, ...archive.filter((record) => !ids.has(record.id))]
      sessionStorage.setItem('recent-archive', JSON.stringify(merged))
      return merged
    }
    internal.invoke = async (command, args) => {
      const value = await previous(command, args)
      if (command === 'bootstrap') {
        const data = value as { history: TaskRecord[] }
        return { ...data, history: merge(data.history) }
      }
      if (command === 'get_history') return merge(value as TaskRecord[])
      return value
    }
  }, records)
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
}
async function start(page: Page, path: string) {
  await pathInput(page).fill(path)
  await startButton(page).click()
  await expect(page.getByRole('region', { name: '空间分析任务' })).toContainText('空间分析进行中')
}
async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(startButton(page)).toBeEnabled()
}
async function starts(page: Page) {
  return page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').filter(
      (task: { kind: string }) => task.kind === 'analyze',
    ),
  )
}

test('recent directories use bounded successful history and selecting only fills the literal path', async ({
  page,
}) => {
  const literal = '/Users/demo/中文 [*] $(literal)'
  await setup(page, [
    task('/failed', 0, 'failed'),
    task('/cancelled', 1, 'cancelled'),
    task('/other-kind', 2, 'succeeded', 'status'),
    task('/invalid/../scope', 3),
    task('/Users/demo/valid-but-multiline\nname', 30),
    task(literal, 4),
    task('/old', 5),
    task(literal, 6),
    ...Array.from({ length: 12 }, (_, i) => task(`/dir${i}`, 10 + i)),
  ])
  await expect(recent(page).locator('option')).toHaveCount(9)
  expect(
    await recent(page)
      .locator('option')
      .evaluateAll((options) =>
        options.slice(1).map((option) => (option as HTMLOptionElement).value),
      ),
  ).toEqual([literal, '/old', '/dir0', '/dir1', '/dir2', '/dir3', '/dir4', '/dir5'])
  await expect(recent(page).locator('option').nth(1)).toHaveText('~/中文 [*] $(literal)')
  await recent(page).selectOption(literal)
  await expect(pathInput(page)).toHaveValue(literal)
  await expect(pathInput(page)).toBeFocused()
  await expect(recent(page)).toHaveValue('')
  expect(await starts(page)).toEqual([])
  await recent(page).selectOption('/old')
  await recent(page).selectOption(literal)
  await expect(pathInput(page)).toHaveValue(literal)
  expect(await starts(page)).toEqual([])
})

test('successful analysis adds a recent directory, survives reload, and requires an explicit new analysis', async ({
  page,
}) => {
  await setup(page)
  await expect(recent(page)).toHaveCount(0)
  await start(page, '/fixture/first')
  await finish(page)
  await expect(recent(page).locator('option[value="/fixture/first"]')).toHaveCount(1)
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  // Complete only the ordinary startup status snapshot after the reload.
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(startButton(page)).toBeEnabled()
  await expect(result(page)).toHaveCount(0)
  await recent(page).selectOption('/fixture/first')
  await expect(pathInput(page)).toBeFocused()
  expect(await starts(page)).toHaveLength(1)
  await page.keyboard.press('Enter')
  await finish(page)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/first')
  expect(await starts(page)).toHaveLength(2)
})

test('choosing an unavailable historical directory preserves the live result and a failed reread does not imply current access', async ({
  page,
}) => {
  await setup(page, [task('/fixture/no-longer-exists', 0)])
  await start(page, '/fixture/current')
  await finish(page)
  await recent(page).selectOption('/fixture/no-longer-exists')
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/current')
  expect(await starts(page)).toHaveLength(1)
  await startButton(page).click()
  await page.evaluate(() =>
    sessionStorage.setItem('mock-history-error', '目录无法访问，请重新选择。'),
  )
  await finish(page)
  await expect(page.getByRole('region', { name: '空间分析任务' })).toContainText('目录无法访问')
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/current')
  await expect(recent(page).locator('option[value="/fixture/no-longer-exists"]')).toHaveCount(1)
  await expect(recent(page)).toHaveAttribute('title', /历史记录不代表目录仍可访问/)
})

test('active analysis disables recent selection and cancellation adds no successful directory', async ({
  page,
}) => {
  await setup(page, [task('/fixture/previous', 0)])
  await start(page, '/fixture/cancelled')
  await expect(recent(page)).toBeDisabled()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(recent(page)).toBeEnabled()
  await expect(page.getByRole('region', { name: '空间分析任务' })).toContainText('空间分析已取消')
  await expect(recent(page).locator('option[value="/fixture/cancelled"]')).toHaveCount(0)
  await expect(recent(page).locator('option[value="/fixture/previous"]')).toHaveCount(1)
})

test('recent paths fit the compact minimum-width toolbar in both themes with navigation and cancellation', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 780 })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await setup(
    page,
    Array.from({ length: 8 }, (_, i) => task('/fixture/' + '很长的目录 '.repeat(16) + i, i)),
  )
  await start(page, '/fixture/current')
  await finish(page)
  await start(page, '/fixture/next')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(recent(page)).toBeVisible()
    await expect(page.getByRole('button', { name: '取消', exact: true })).toBeVisible()
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/recent-analysis-${theme}.png`,
      animations: 'disabled',
    })
  }
})
