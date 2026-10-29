import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

const report =
  JSON.stringify(
    {
      schema: 'moomole-client-task-summary-v1',
      appVersion: '0.1.0',
      platform: 'macos',
      architecture: 'aarch64',
      currentEngineVersion: '1.65.0',
      taskKind: 'status',
      taskState: 'running',
      directoryRecorded: false,
      diagnosticRecorded: false,
      recordElapsedMs: null,
      note: 'fixture metadata summary; no paths, IDs, exact timestamps or original diagnostics',
    },
    null,
    2,
  ) + '\n'

async function setup(page: Page) {
  await installTaskMock(page, 'new')
  await page.addInitScript(
    ({ report }) => {
      const internals = (
        window as unknown as {
          __TAURI_INTERNALS__: {
            invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
          }
        }
      ).__TAURI_INTERNALS__
      const invoke = internals.invoke
      internals.invoke = async (command, args = {}) => {
        if (command === 'prepare_task_diagnostic') {
          sessionStorage.setItem(
            'diagnostic-prepares',
            String(Number(sessionStorage.getItem('diagnostic-prepares') ?? 0) + 1),
          )
          if (sessionStorage.getItem('diagnostic-prepare-error'))
            throw new Error('fixture record unavailable')
          return report
        }
        if (command === 'save_task_diagnostic') {
          sessionStorage.setItem('diagnostic-saved-report', String(args.report))
          const mode = sessionStorage.getItem('diagnostic-save-mode')
          if (mode === 'error') throw new Error('fixture save failed; existing file preserved')
          return mode !== 'cancel'
        }
        return invoke(command, args)
      }
    },
    { report },
  )
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await page
    .getByRole('region', { name: '客户端检查任务', exact: true })
    .getByRole('article')
    .getByRole('button')
    .first()
    .click()
  return page.getByRole('dialog', { name: '检查任务详情' })
}

test('diagnostic export previews a fixed report before saving exact bytes and preserves cancellation or failure for retry', async ({
  page,
}) => {
  const detail = await setup(page)
  const section = detail.getByRole('region', { name: '诊断摘要导出' })
  await expect(section.getByRole('button', { name: '选择位置并保存' })).toHaveCount(0)
  expect(await page.evaluate(() => sessionStorage.getItem('diagnostic-prepares'))).toBeNull()
  await section.getByRole('button', { name: '生成诊断摘要', exact: true }).click()
  const preview = section.getByLabel('将保存的诊断摘要')
  await expect(preview).toHaveText(report)
  await expect(preview).toBeFocused()
  await expect(section).toContainText('不含路径、任务 ID、精确时间和原始诊断')
  expect(await page.evaluate(() => sessionStorage.getItem('diagnostic-saved-report'))).toBeNull()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(detail.locator('.task-detail-state')).toHaveText('已完成')
  await expect(preview).toHaveText(report)
  await page.setViewportSize({ width: 960, height: 780 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await preview.scrollIntoViewIfNeeded()
    expect(await detail.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/diagnostic-export-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('diagnostic-save-mode', 'cancel'))
  await section.getByRole('button', { name: '选择位置并保存' }).click()
  await expect(section.getByRole('status')).toHaveText('已取消保存，没有导出文件。')
  await expect(section.getByRole('button', { name: '选择位置并保存' })).toBeFocused()
  await expect(preview).toHaveText(report)
  await page.evaluate(() => sessionStorage.setItem('diagnostic-save-mode', 'error'))
  await section.getByRole('button', { name: '选择位置并保存' }).click()
  await expect(section.getByRole('alert')).toContainText('fixture save failed')
  await expect(section.getByRole('button', { name: '选择位置并保存' })).toBeFocused()
  await expect(section.getByRole('status')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.removeItem('diagnostic-save-mode'))
  await section.getByRole('button', { name: '选择位置并保存' }).click()
  await expect(section.getByRole('status')).toContainText('已保存到所选位置，未上传')
  expect(await page.evaluate(() => sessionStorage.getItem('diagnostic-saved-report'))).toBe(report)
  await expect(section.getByRole('alert')).toHaveCount(0)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').length),
  ).toBe(1)
})

test('failed report generation offers retry and reopening task details clears the old preview', async ({
  page,
}) => {
  const detail = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('diagnostic-prepare-error', 'yes'))
  await detail.getByRole('button', { name: '生成诊断摘要', exact: true }).click()
  await expect(detail.getByRole('alert')).toContainText('fixture record unavailable')
  await expect(detail.getByRole('button', { name: '选择位置并保存' })).toHaveCount(0)
  await page.evaluate(() => sessionStorage.removeItem('diagnostic-prepare-error'))
  await detail.getByRole('button', { name: '生成诊断摘要', exact: true }).click()
  await expect(detail.getByLabel('将保存的诊断摘要')).toHaveText(report)
  await page.keyboard.press('Escape')
  await expect(detail).toHaveCount(0)
  await page
    .getByRole('region', { name: '客户端检查任务', exact: true })
    .getByRole('article')
    .getByRole('button')
    .first()
    .click()
  await expect(detail.getByLabel('将保存的诊断摘要')).toHaveCount(0)
  await expect(detail.getByRole('button', { name: '生成诊断摘要', exact: true })).toBeEnabled()
  expect(await page.evaluate(() => sessionStorage.getItem('diagnostic-saved-report'))).toBeNull()
})
