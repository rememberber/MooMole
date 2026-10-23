import { expect, test, type Page } from '@playwright/test'
import corpus from '../fixtures/analysis-protocol.json' with { type: 'json' }
import { installTaskMock } from './fixtures/native-task'

const result = (page: Page) => page.getByRole('region', { name: 'Mole 空间分析结果' })
const notice = (page: Page) => page.getByRole('region', { name: '空间分析任务' })
const startButton = (page: Page) => page.getByRole('button', { name: '开始分析', exact: true })

async function open(page: Page) {
  await installTaskMock(page, 'analyze', undefined, undefined, corpus.base)
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
}
async function start(page: Page, path = '/fixture') {
  await page.getByRole('textbox', { name: '扫描目录', exact: true }).fill(path)
  await startButton(page).click()
  await expect(notice(page)).toContainText('空间分析进行中')
}
async function finish(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(startButton(page)).toBeEnabled()
}
async function nextResult(page: Page, value: unknown) {
  await page.evaluate(
    (value) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(value)),
    value,
  )
}

test('malformed fresh results preserve the prior result, selection, filter and navigation until a valid retry', async ({
  page,
}) => {
  await open(page)
  await start(page)
  await finish(page)
  await result(page).getByRole('textbox', { name: '筛选目录内容' }).fill('child')
  await result(page).getByRole('button', { name: '查看 child · 1 KB', exact: true }).click()
  const beforeErrors: string[] = []
  page.on('pageerror', (error) => beforeErrors.push(error.message))
  for (const invalid of [
    { ...corpus.base, entries: [null] },
    { ...corpus.base, entries: [...corpus.base.entries, ...corpus.base.entries] },
    { ...corpus.base, entries: [{ ...corpus.base.entries[0], path: '/outside/child' }] },
    { ...corpus.base, total_size: Number.MAX_SAFE_INTEGER + 1 },
    { ...corpus.base, scan_status: 'future' },
  ]) {
    await nextResult(page, invalid)
    await start(page, '/fixture/next')
    await finish(page)
    await expect(notice(page)).toContainText('分析结果未能显示')
    await expect(notice(page)).toContainText('格式异常，未更新')
    await expect(notice(page)).toContainText('上次成功结果仍保留，本次未替换')
    await expect(notice(page)).not.toContainText('任务状态待确认')
    await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture')
    await expect(result(page).getByRole('textbox', { name: '筛选目录内容' })).toHaveValue('child')
    await expect(result(page).getByRole('region', { name: '选中项目详情' })).toContainText('child')
    await expect(page.getByRole('button', { name: '返回上次分析的目录' })).toBeDisabled()
  }
  expect(beforeErrors).toEqual([])
  const valid = {
    ...corpus.base,
    path: '/fixture/next',
    entries: [{ ...corpus.base.entries[0], path: '/fixture/next/child' }],
  }
  await nextResult(page, valid)
  await notice(page).getByRole('button', { name: '重新分析此目录' }).click()
  await finish(page)
  await expect(notice(page)).toHaveCount(0)
  await expect(result(page).locator('.analysis-path')).toHaveAttribute('title', '/fixture/next')
  await expect(page.getByRole('button', { name: '返回上次分析的目录' })).toBeEnabled()
})

test('a malformed retained result is rejected after reload without restarting the original task', async ({
  page,
}) => {
  await open(page)
  await start(page)
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(notice(page)).toContainText('请求目录：/fixture')
  await nextResult(page, { ...corpus.base, entries: {} })
  await finish(page)
  await expect(notice(page)).toContainText('分析结果未能显示')
  await expect(notice(page)).not.toContainText('本机任务可能仍在运行')
  await expect(result(page)).toHaveCount(0)
  expect(
    await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').filter(
        (task: { kind: string }) => task.kind === 'analyze',
      ),
    ),
  ).toHaveLength(1)
})

test('pinned symlink display markers and filenames ending in arrows keep their literal paths', async ({
  page,
}) => {
  await open(page)
  await nextResult(page, {
    ...corpus.base,
    entries: [
      { ...corpus.base.entries[0], name: 'link →', path: '/fixture/link', is_dir: false },
      { ...corpus.base.entries[0], name: 'literal →', path: '/fixture/literal →', is_dir: false },
      { ...corpus.base.entries[0], name: 'link → →', path: '/fixture/link →', is_dir: false },
    ],
  })
  await start(page)
  await finish(page)
  await expect(notice(page)).toHaveCount(0)
  await expect(result(page).getByRole('article')).toHaveCount(3)
  await result(page).getByRole('button', { name: '查看 /fixture/link → 详情', exact: true }).click()
  await expect(
    result(page).getByRole('region', { name: '选中项目详情' }).locator('.mono'),
  ).toHaveText('/fixture/link →')
})

test('over-budget and malformed optional containers show a recoverable result error with no partial new list', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 780 })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await open(page)
  await nextResult(page, { ...corpus.base, entries: Array(20_001).fill(corpus.base.entries[0]) })
  await start(page)
  await finish(page)
  await expect(notice(page)).toContainText('超过 20000 项')
  await expect(notice(page)).toContainText('选择更小的目录')
  await expect(result(page)).toHaveCount(0)
  for (const large_files of ['invalid', Array(1001).fill(null)]) {
    await nextResult(page, { ...corpus.base, large_files })
    await start(page)
    await finish(page)
    const diagnostic = notice(page).getByText('不匹配字段：large_files', { exact: true })
    await expect(diagnostic).toBeHidden()
    await notice(page).getByText('查看格式诊断', { exact: true }).focus()
    await page.keyboard.press('Enter')
    await expect(diagnostic).toBeVisible()
    await page.keyboard.press('Enter')
    await expect(diagnostic).toBeHidden()
    await expect(result(page)).toHaveCount(0)
  }
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/analysis-protocol-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
})
