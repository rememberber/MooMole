import { expect, test, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/folder-schedules.tsx')).mount(mode),
    mode,
  )
  return page.getByRole('region', { name: '目录定期扫描', exact: true })
}
async function add(page: Page, enabled = false) {
  const section = page.getByRole('region', { name: '目录定期扫描', exact: true })
  await section.getByRole('button', { name: '添加扫描目录…' }).click()
  const dialog = page.getByRole('dialog', { name: '目录扫描范围与周期' })
  await expect(dialog.getByRole('checkbox')).not.toBeChecked()
  await dialog.getByRole('button', { name: '选择扫描目录…' }).click()
  if (enabled) await dialog.getByRole('checkbox').check()
  await dialog.getByRole('combobox').selectOption('15')
  await dialog.getByRole('button', { name: '保存扫描范围' }).click()
  await expect(dialog).not.toBeVisible()
  return section
}
async function calls(page: Page) {
  return JSON.parse(
    (await page.locator('#folder-schedules-fixture').getAttribute('data-calls'))!,
  ) as { command: string; args?: any }[]
}
test('scope and recurrence require saving and reloading never resubmits; organizing only passes the root', async ({
  page,
}) => {
  await mount(page)
  let section = await add(page, true)
  await expect(section).toContainText('定期目录扫描已开启')
  expect((await calls(page)).filter((c) => c.command !== 'read')).toEqual([
    {
      command: 'configure',
      args: { id: null, root: '/Users/owned/Downloads', enabled: true, intervalMinutes: 15 },
    },
  ])
  section = await mount(page)
  await expect(section).toContainText('定期目录扫描已开启')
  expect((await calls(page)).every((c) => c.command === 'read')).toBe(true)
  await section.getByRole('button', { name: '查看并整理' }).click()
  await expect(page.getByLabel('fixture organize root')).toHaveText('/Users/owned/Downloads')
  expect((await calls(page)).filter((c) => c.command === 'scan')).toEqual([])
  await section.getByRole('button', { name: '暂停扫描', exact: true }).click()
  await expect(section).toContainText('定期目录扫描已暂停')
})
test('a paused directory can scan once and exact cancellation retains an incomplete observation', async ({
  page,
}) => {
  await mount(page)
  const section = await add(page)
  await section.getByRole('button', { name: '立即扫描' }).click()
  await expect(section.getByRole('button', { name: '停止目录扫描' })).toBeVisible()
  await section.getByRole('button', { name: '停止目录扫描' }).click()
  await expect(section.getByRole('button', { name: '停止目录扫描' })).toHaveCount(0)
  await section.getByText('目录扫描记录', { exact: true }).click()
  await expect(section).toContainText('目录扫描已取消')
  await expect(section).toContainText('扫描不完整，不比较缺失文件，也不替换完整基线。')
  const items = await calls(page),
    scan = items.find((c) => c.command === 'scan')!,
    cancel = items.find((c) => c.command === 'cancel')!
  expect(cancel.args.id).toBe(scan.args.requestId)
  await expect(section).toContainText('定期目录扫描已暂停')
  await section.getByRole('button', { name: '移除扫描任务' }).click()
  await expect(section).toContainText('尚未配置扫描目录')
  await expect(section).toContainText('目录扫描已取消')
})
test('saved comparison clues explain not-seen and do not grant file-processing authority', async ({
  page,
}) => {
  const section = await mount(page, 'changes')
  await section.getByText('目录扫描记录', { exact: true }).click()
  await expect(section).toContainText('新增 1 · 属性变化 1 · 未再见 1')
  await section.getByText('查看变化线索', { exact: true }).click()
  await expect(section).toContainText('/Users/owned/Downloads/old.txt')
  await expect(section).toContainText('不代表文件已删除')
  expect((await calls(page)).every((c) => c.command === 'read')).toBe(true)
})
test('unsupported observation data cannot enable scheduling controls', async ({ page }) => {
  const section = await mount(page, 'invalid')
  await expect(section.getByRole('alert')).toContainText('目录定期扫描数据无法核对')
  await expect(section.getByRole('button', { name: '添加扫描目录…' })).toBeDisabled()
})
