import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

async function setup(page: Page) {
  await installTaskMock(page, 'new')
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
      if (command === 'bootstrap') {
        const initial = (await previous(command, args)) as Record<string, unknown>
        const saved = sessionStorage.getItem('retention-saved')
        return saved ? { ...initial, settings: JSON.parse(saved) } : initial
      }
      if (command === 'save_settings') {
        sessionStorage.setItem('retention-request', JSON.stringify(args?.settings))
        sessionStorage.setItem('retention-started', 'yes')
        while (sessionStorage.getItem('retention-hold'))
          await new Promise((resolve) => setTimeout(resolve, 20))
        if (sessionStorage.getItem('retention-fail'))
          throw new Error('设置保存失败，请重试。诊断：injected retention failure')
        sessionStorage.setItem('retention-saved', JSON.stringify(args?.settings))
        return args?.settings
      }
      if (command === 'get_history' && sessionStorage.getItem('retention-read-fail'))
        throw new Error('fixture history unavailable')
      return previous(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  return page.getByRole('dialog', { name: '设置', exact: true })
}

test('retention failure keeps the saved preference, blocks duplicate changes and allows retry', async ({
  page,
}) => {
  const dialog = await setup(page)
  const retention = dialog.getByRole('combobox', { name: '历史记录保留时间', exact: true })
  const theme = dialog.getByRole('combobox', { name: '主题', exact: true })
  await expect(retention).toHaveValue('30')
  await page.evaluate(() => {
    sessionStorage.setItem('retention-hold', 'yes')
    sessionStorage.setItem('retention-fail', 'yes')
  })
  await retention.selectOption('7')
  await expect(dialog.getByRole('status')).toContainText('正在保存')
  await expect(retention).toBeDisabled()
  await expect(theme).toBeDisabled()
  // The controlled value only changes after the backend transaction succeeds.
  await expect(retention).toHaveValue('30')
  await page.evaluate(() => sessionStorage.removeItem('retention-hold'))
  await expect(dialog.getByRole('alert')).toContainText('设置保存失败，请重试')
  await expect(retention).toBeEnabled()
  await expect(theme).toBeEnabled()
  await expect(retention).toHaveValue('30')
  expect(await page.evaluate(() => sessionStorage.getItem('retention-saved'))).toBeNull()
  await page.keyboard.press('Escape')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await expect(retention).toHaveValue('30')
  await page.evaluate(() => sessionStorage.removeItem('retention-fail'))
  await retention.selectOption('7')
  await expect(retention).toHaveValue('7')
  await expect(dialog.getByRole('alert')).toHaveCount(0)
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('retention-saved')!).retentionDays),
  ).toBe(7)
  await page.reload()
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await expect(retention).toHaveValue('7')
  expect(
    await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').map(
        (task: { kind: string }) => task.kind,
      ),
    ),
  ).toEqual(['status']) // Only the normal startup snapshot; saving never starts another task.
})

test('a history refresh failure after commit is not reported as a failed settings save', async ({
  page,
}) => {
  const dialog = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('retention-read-fail', 'yes'))
  const retention = dialog.getByRole('combobox', { name: '历史记录保留时间', exact: true })
  await retention.selectOption('7')
  await expect(retention).toHaveValue('7')
  await expect(retention).toBeEnabled()
  await expect(dialog.getByRole('alert')).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(page.getByRole('alert')).toContainText('设置已保存，但历史记录无法刷新')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await expect(retention).toHaveValue('7')
})

test('retention scope stays compact and its exceptions expand by keyboard in both themes', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 720 })
  const dialog = await setup(page)
  await expect(dialog).toContainText('启动或保存设置时回收过期记录，不删除你的文件')
  const summary = dialog.locator('.settings-retention > summary')
  const details = dialog.locator('.settings-retention')
  await expect(details).not.toHaveAttribute('open', '')
  await expect(dialog.getByText('记录可能包含路径和检查结果，不保存文件正文。')).toBeHidden()
  await summary.focus()
  await page.keyboard.press('Enter')
  await expect(details).toHaveAttribute('open', '')
  await expect(details).toContainText('扫描最多保留 10 份，关联审阅计划最多 20 份')
  await expect(details).toContainText('可能早于所选期限回收')
  await expect(details).toContainText('等待或进行中的任务会保留')
  await expect(details).toContainText('结果待确认或中断的执行凭证')
  await expect(details).toContainText('Mole 原始历史、独立备份与导出文件不在此回收范围内')
  expect(await page.evaluate(() => sessionStorage.getItem('retention-started'))).toBeNull()
  for (const theme of ['light', 'dark']) {
    // Browser fixture preferences only; no native database is used.
    await dialog.getByRole('combobox', { name: '主题', exact: true }).selectOption(theme)
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
    await expect(summary).toBeVisible()
    expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
      true,
    )
    expect(
      await dialog
        .locator('.settings-content')
        .evaluate((element) => element.scrollWidth <= element.clientWidth),
    ).toBe(true)
    await page.screenshot({ path: `test-results/settings-retention-${theme}.png` })
  }
  await summary.focus()
  await page.keyboard.press('Enter')
  await expect(details).not.toHaveAttribute('open', '')
  expect(
    await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').map(
        (task: { kind: string }) => task.kind,
      ),
    ),
  ).toEqual(['status'])
})
