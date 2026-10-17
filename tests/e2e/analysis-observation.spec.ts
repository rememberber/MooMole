import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'
import type { AnalysisObservation } from '../../src/types'

const observation: AnalysisObservation = {
  version: 1,
  source: 'mole-directory-json',
  startedAt: 1792180800000,
  finishedAt: 1792180801250,
  elapsedMs: 1250,
  engineVersion: '1.59.1',
  revision: 'fixture-revision',
  cacheUse: 'unreported',
}

test('read provenance stays distinct from measurement freshness; coverage ignores filters', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(
    async (value) =>
      (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(false, value),
    observation,
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  const summary = explorer.getByLabel('结果时间与完整性')
  await expect(summary).toContainText('本次读取完成')
  await expect(summary).toContainText('用时 1.3 秒')
  await expect(summary).toContainText('缓存使用情况未知')
  await expect(summary).toContainText('3 项已完整测量 · 2 项部分测量 · 1 项无法测量或状态未知')
  await expect(summary).toContainText('未知大小不代表零')
  await expect(explorer.getByText(/来源：Mole CLI/)).not.toBeVisible()
  await explorer.getByRole('combobox', { name: '目录项目类型' }).selectOption('file')
  await explorer.getByRole('textbox', { name: '筛选目录内容' }).fill('empty')
  await expect(
    explorer.getByRole('region', { name: '目录分页列表' }).getByRole('article'),
  ).toHaveCount(1)
  await expect(summary).toContainText('3 项已完整测量 · 2 项部分测量 · 1 项无法测量或状态未知')
  const details = explorer.getByText('测量范围与图表说明', { exact: true })
  await details.focus()
  await page.keyboard.press('Enter')
  await expect(explorer.getByText(/来源：Mole CLI v1.59.1/)).toBeVisible()
  await expect(explorer.getByText(/引擎修订：fixture-revision/)).toBeVisible()
  await expect(explorer.getByText(/读取完成时间不等于每个项目的测量时间/)).toBeVisible()
})

test('missing, future, and malformed provenance remain unknown and cannot claim fresh measurement', async ({
  page,
}) => {
  for (const value of [
    undefined,
    { ...observation, version: 2 },
    { ...observation, finishedAt: -1 },
    { ...observation, cacheUse: 'fresh' },
  ]) {
    await page.goto('/')
    await page.evaluate(
      async (value) =>
        (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(
          false,
          value as AnalysisObservation,
        ),
      value,
    )
    const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
    await expect(explorer.getByLabel('结果时间与完整性')).toContainText(
      '读取时间未记录 · 缓存使用情况未知',
    )
    await expect(explorer.locator('time')).toHaveCount(0)
    await explorer.getByText('测量范围与图表说明', { exact: true }).click()
    await expect(explorer.getByText(/没有可识别的读取来源记录/)).toBeVisible()
    await expect(explorer.getByText(/来源：Mole CLI/)).toHaveCount(0)
  }
})

test('task snapshots preserve original timestamps through pending, failed, cancelled and resumed observations', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze', undefined, undefined, {
    ...analysis,
    mooObservation: observation,
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录' }).fill(analysis.path)
  const start = page.getByRole('button', { name: '开始分析', exact: true })
  await start.click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const time = page.getByRole('region', { name: 'Mole 空间分析结果' }).locator('time')
  const original = new Date(observation.finishedAt).toISOString()
  await expect(time).toHaveAttribute('datetime', original)
  await start.click()
  await expect(time).toHaveAttribute('datetime', original)
  await page.evaluate(() => {
    sessionStorage.setItem('mock-history-error', 'fixture 扫描失败')
    sessionStorage.setItem('mock-done', 'yes')
  })
  await expect(start).toBeEnabled()
  await expect(page.getByRole('alert')).toContainText('fixture 扫描失败')
  await expect(time).toHaveAttribute('datetime', original)
  await page.evaluate(() => sessionStorage.removeItem('mock-history-error'))
  await start.click()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(start).toBeEnabled()
  await expect(time).toHaveAttribute('datetime', original)
  await start.click()
  await page.reload()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(time).toHaveAttribute('datetime', original)
  const updated = {
    ...observation,
    startedAt: observation.startedAt + 60000,
    finishedAt: observation.finishedAt + 60000,
  }
  await page.evaluate(
    (value) => sessionStorage.setItem('mock-analysis-result', JSON.stringify(value)),
    { ...analysis, mooObservation: updated },
  )
  await start.click()
  await expect(time).toHaveAttribute('datetime', original)
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(time).toHaveAttribute('datetime', new Date(updated.finishedAt).toISOString())
  await page.getByText('测量范围与图表说明', { exact: true }).click()
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/analysis-observation-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
})
