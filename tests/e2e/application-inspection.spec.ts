import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'

async function setup(page: Page) {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const previous = internal.invoke
    internal.invoke = async (command, args) => {
      if (command !== 'inspect_application') return previous(command, args)
      const calls = JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]')
      calls.push(args)
      sessionStorage.setItem('inspection-calls', JSON.stringify(calls))
      const mode = sessionStorage.getItem('inspection-mode')
      if (mode === 'pending')
        while (!sessionStorage.getItem('inspection-release'))
          await new Promise((resolve) => setTimeout(resolve, 20))
      sessionStorage.setItem(
        'inspection-completed',
        String(Number(sessionStorage.getItem('inspection-completed') ?? 0) + 1),
      )
      if (mode === 'error') throw new Error('应用状态读取超时，结果无法确认；请稍后重试。')
      return {
        protocol: mode === 'future' ? 2 : 1,
        checkedAt: Date.now(),
        bundleId: mode === 'mismatch' ? 'org.example.changed' : 'org.example.same',
        version: mode === 'pending' ? 'old version' : '2.3',
        build: '42',
        identity: mode === 'mismatch' ? 'different' : 'matches',
        running:
          mode === 'mismatch'
            ? {
                code: 'unknown',
                pids: [],
                otherCopies: 0,
                reason: '本地应用标识与 Mole 清单不同，请重新读取列表。',
              }
            : mode === 'other'
              ? {
                  code: 'notObserved',
                  pids: [],
                  otherCopies: 1,
                  reason: '本次未发现此安装位置的匹配主应用；辅助进程、后台服务和文件占用未检查。',
                }
              : {
                  code: 'observed',
                  pids: [123],
                  otherCopies: 0,
                  reason: '本次发现此安装位置的主应用运行；未检查辅助进程或后台服务。',
                },
      }
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await expect(page.getByText('应用列表进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(5)
  const row = library.getByRole('article', {
    name: 'Example · /Applications/Example.app',
    exact: true,
  })
  return { library, row, inspection: row.getByRole('region', { name: '应用状态检查' }) }
}

test('opening details checks only the chosen task entry and distinguishes running, other copies and unknown identity', async ({
  page,
}) => {
  const { library, row, inspection } = await setup(page)
  expect(await page.evaluate(() => sessionStorage.getItem('inspection-calls'))).toBeNull()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await expect(inspection).toContainText('2.3 · 构建 42')
  await expect(inspection).toContainText('未验证签名')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(Object.keys(calls[0]).sort()).toEqual(['index', 'taskId'])
  expect(calls[0].index).toBe(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await row.getByText('位置与来源', { exact: true }).click()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await expect(inspection).toContainText('另有 1 个同标识安装位置')
  await expect(inspection).toContainText('不代表可卸载')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'mismatch'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toHaveText('运行状态无法确认')
  await expect(inspection).toContainText('本地应用标识与 Mole 清单不同')
  await expect(inspection).not.toContainText('主应用 PID')
  await expect(library.getByRole('button', { name: /退出应用|卸载|删除/ })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-inspection-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
})

test('a refreshed list discards a delayed observation from its old task', async ({ page }) => {
  const { library, row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection.getByRole('button', { name: '检查中…' })).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('inspection-completed')))
    .toBe('2')
  await expect(inspection).not.toContainText('old version')
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(calls).toHaveLength(2)
  expect(calls[0].taskId).not.toBe(calls[1].taskId)
})

test('read errors and unrecognized protocol stay unknown and retry without keeping a positive result', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'error'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('读取超时')
  await expect(inspection.getByRole('status')).toHaveText('应用状态无法确认')
  await expect(inspection).not.toContainText('主应用 PID')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'future'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await page.evaluate(() => sessionStorage.removeItem('inspection-mode'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})

test('an open observation becomes visibly older and rechecking withdraws the old conclusion while pending', async ({
  page,
}) => {
  await page.clock.install()
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await page.clock.fastForward(60001)
  await expect(inspection.getByRole('status')).toHaveText('较早结果 · 发现主应用运行')
  await expect(inspection).toContainText('请重新检查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection).not.toContainText('主应用 PID')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})
