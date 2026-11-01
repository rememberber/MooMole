import { expect, test } from '@playwright/test'
import { installMoleCleanMock } from './fixtures/mole-cleanup'
const calls = async (page: import('@playwright/test').Page) =>
  page.evaluate(
    () =>
      JSON.parse(sessionStorage.getItem('mole-calls') ?? '[]') as {
        command: string
        args: Record<string, unknown>
      }[],
  )
async function open(page: import('@playwright/test').Page) {
  await page.goto('/')
  await page.getByRole('button', { name: '开始一键清理' }).click()
  const dialog = page.getByRole('dialog', { name: 'Mole 一键清理' })
  await expect(dialog).toBeVisible()
  return dialog
}
const consent = '我已了解永久删除和废纸篓处理不可撤销，确认执行上述完整范围。'
test('overview opens the complete Mole operation scope without launching a narrow scan or deleting before consent', async ({
  page,
}) => {
  await installMoleCleanMock(page)
  const dialog = await open(page)
  await expect(dialog.locator('.mole-clean-scopes li')).toHaveCount(15)
  await expect(dialog).toContainText('浏览器缓存')
  await expect(dialog).toContainText('开发工具与包管理器缓存')
  await expect(dialog).toContainText('失败的 Time Machine 备份')
  await expect(dialog).toContainText('会永久删除')
  await expect(dialog).toContainText('当前账户废纸篓')
  const confirm = dialog.getByRole('button', { name: '确认执行完整清理' })
  await expect(confirm).toBeDisabled()
  expect((await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')).toHaveLength(0)
  await dialog.getByRole('checkbox', { name: consent }).check()
  await expect(confirm).toBeEnabled()
  const taskCalls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(taskCalls.map((c: { kind: string }) => c.kind)).toEqual(['status'])
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => (document.documentElement.dataset.theme = t), theme)
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/mole-clean-confirm-${theme}.png`,
      animations: 'disabled',
    })
  }
})
test('system choice creates a new bound confirmation and expiry withdraws execution', async ({
  page,
}) => {
  await page.clock.install()
  await installMoleCleanMock(page)
  const dialog = await open(page)
  await dialog.getByRole('checkbox', { name: consent }).check()
  await dialog.getByRole('checkbox', { name: '包括需要管理员权限的系统项目' }).check()
  await expect(dialog.getByRole('checkbox', { name: consent })).not.toBeChecked()
  await expect(dialog.getByRole('button', { name: '确认执行完整清理' })).toBeDisabled()
  expect(
    (await calls(page))
      .filter((c) => c.command === 'prepare_mole_cleanup')
      .map((c) => c.args.system),
  ).toEqual([false, true])
  await dialog.getByRole('checkbox', { name: consent }).check()
  await page.clock.fastForward(121_000)
  await expect(dialog).toContainText('完整清理确认已过期')
  await expect(dialog.getByRole('button', { name: '确认执行完整清理' })).toBeDisabled()
  expect((await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')).toHaveLength(0)
})
test('one confirmed execution survives reload, resumes its original progress and cancels only its own request', async ({
  page,
}) => {
  await installMoleCleanMock(page)
  let dialog = await open(page)
  await dialog.getByRole('checkbox', { name: consent }).check()
  await dialog.getByRole('button', { name: '确认执行完整清理' }).click()
  await expect(dialog.getByRole('button', { name: '停止本次清理' })).toBeVisible()
  const executions = (await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')
  expect(executions).toHaveLength(1)
  expect(Object.keys(executions[0].args).sort()).toEqual([
    'permanentConfirmed',
    'planId',
    'requestId',
    'trashConfirmed',
  ])
  const id = executions[0].args.requestId
  await dialog.getByRole('button', { name: '在后台继续' }).click()
  await expect(page.getByRole('region', { name: '完整清理状态' })).toBeVisible()
  await page.reload()
  await page.getByRole('button', { name: '查看清理进度' }).click()
  dialog = page.getByRole('dialog', { name: 'Mole 一键清理' })
  await expect(dialog.getByRole('button', { name: '停止本次清理' })).toBeVisible()
  expect((await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')).toHaveLength(1)
  await dialog.getByRole('button', { name: '停止本次清理' }).click()
  await expect(dialog.getByRole('status')).toContainText('清理已停止')
  expect((await calls(page)).find((c) => c.command === 'cancel_mole_cleanup')?.args.requestId).toBe(
    id,
  )
})
test('rejected or untracked requests never automatically submit a second cleanup', async ({
  page,
}) => {
  await installMoleCleanMock(page)
  const dialog = await open(page)
  await page.evaluate(() => sessionStorage.setItem('mole-reject', 'yes'))
  await dialog.getByRole('checkbox', { name: consent }).check()
  await dialog.getByRole('button', { name: '确认执行完整清理' }).click()
  await expect(dialog.getByRole('alert')).toContainText('保护规则已变化')
  await expect(dialog.getByRole('button', { name: '确认执行完整清理' })).toHaveCount(0)
  expect((await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')).toHaveLength(1)
})
test('administrator input is cleared after submission and never becomes a persisted call or plan field', async ({
  page,
}) => {
  await installMoleCleanMock(page, 'authorization')
  const dialog = await open(page)
  await dialog.getByRole('checkbox', { name: '包括需要管理员权限的系统项目' }).check()
  await dialog.getByRole('checkbox', { name: consent }).check()
  await dialog.getByRole('button', { name: '确认执行完整清理' }).click()
  await dialog.getByLabel('当前账户密码').fill('owned-fixture-input')
  await dialog.getByRole('button', { name: '提交本次授权' }).click()
  await expect(dialog.getByLabel('当前账户密码')).toHaveCount(0)
  expect(JSON.stringify(await calls(page))).not.toContain('owned-fixture-input')
  expect(await page.evaluate(() => sessionStorage.getItem('mole-record'))).not.toContain(
    'owned-fixture-input',
  )
  await dialog.getByRole('button', { name: '停止本次清理' }).click()
  await expect(dialog.getByRole('status')).toContainText('清理已停止')
})
test('uncertain records require explicit review, successful saving and separate confirmation before history can be removed', async ({
  page,
}) => {
  await installMoleCleanMock(page, 'uncertain')
  await page.goto('/')
  await page.getByRole('button', { name: '清理记录', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Mole 一键清理' })
  await expect(dialog).toContainText('清理结果待核查')
  await dialog.getByText('保存与核查记录', { exact: true }).click()
  const reviewed = dialog.getByRole('checkbox', {
    name: '我已核查本批次的实际结果，了解不会撤销或重新执行。',
  })
  await reviewed.check()
  await dialog.getByRole('button', { name: '标记为已核查' }).click()
  const clear = dialog.getByRole('button', { name: '清除已保存的历史记录' })
  await expect(clear).toBeDisabled()
  await dialog
    .getByRole('checkbox', { name: '我已保存导出的记录，确认永久清除本批次历史。' })
    .check()
  await expect(clear).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('mole-save-cancelled', 'yes'))
  await dialog.getByRole('button', { name: '导出完整清理记录' }).click()
  await expect(clear).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('mole-save-cancelled'))
  await dialog.getByRole('button', { name: '导出完整清理记录' }).click()
  await expect(clear).toBeEnabled()
  await clear.click()
  await expect(dialog).toContainText('尚无完整 Mole 清理记录')
  expect((await calls(page)).filter((c) => c.command === 'execute_mole_cleanup')).toHaveLength(0)
})
