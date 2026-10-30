import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'

async function setup(page: Page) {
  await installTaskMock(page, 'analyze', undefined, undefined, analysis)
  await page.goto('/')
  await page.evaluate(() => {
    sessionStorage.setItem('mock-done', 'yes')
    const native = (window as any).__TAURI_INTERNALS__
    const base = native.invoke
    native.invoke = async (command: string, args: any) => {
      if (!['inspect_space_item', 'open_storage_settings'].includes(command))
        return base(command, args)
      const calls = JSON.parse(sessionStorage.getItem('space-calls') ?? '[]')
      sessionStorage.setItem('space-calls', JSON.stringify([...calls, { command, args }]))
      if (command === 'open_storage_settings') return
      const mode = sessionStorage.getItem('space-mode')
      if (mode === 'unavailable') throw new Error('当前位置不可读取；旧结果不再适用。')
      return {
        version: 1,
        path: mode === 'mismatch' ? '/unselected' : args.path,
        checkedAt: Date.now(),
        isDirectory: args.path.endsWith('Projects'),
        logicalBytes: args.path.endsWith('Projects') ? null : 67108864,
        allocatedBytes: args.path.endsWith('Projects') ? null : 0,
        dataless: mode === 'placeholder',
        location: mode === 'placeholder' ? 'providerLocation' : 'unclassified',
      }
    }
  })
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '空间分析', exact: true })
    .click()
}

async function selected(page: Page, name = 'video.mp4') {
  await page.getByRole('textbox', { name: '扫描目录', exact: true }).fill(analysis.path)
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button', { name: new RegExp(`^查看 ${name}`) })
    .click()
  await explorer.getByRole('button', { name: '解释空间占用', exact: true }).click()
  return page.getByRole('dialog', { name: '空间占用说明' })
}

test('space guide works before scanning and leads to review without automatic processing', async ({
  page,
}) => {
  await setup(page)
  await page.getByRole('button', { name: '空间占用说明', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '空间占用说明' })
  await expect(dialog).toContainText('不对应一个可以整体清空的文件夹')
  await expect(dialog).toContainText('删除 iCloud Drive 文件可能同步删除其他设备上的文件')
  expect(await page.evaluate(() => sessionStorage.getItem('space-calls'))).toBeNull()
  await dialog.getByRole('button', { name: '打开系统设置核对分类', exact: true }).click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('space-calls')!))).toEqual([
    { command: 'open_storage_settings', args: {} },
  ])
  await dialog.getByRole('button', { name: '审阅整份设备备份', exact: true }).click()
  const backups = page.getByRole('dialog', { name: '设备备份', exact: true })
  await expect(backups).toBeVisible()
  await backups.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await page.getByRole('button', { name: '空间占用说明', exact: true }).click()
  await dialog.getByRole('button', { name: '审阅常用清理候选', exact: true }).click()
  await expect(page.getByRole('main', { name: '速战速决内容', exact: true })).toBeVisible()
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-native-commands')!),
  )
  expect(
    calls.some((c: any) =>
      /^(execute_|start_device_backup_scan|start_clean_preview)/.test(c.command),
    ),
  ).toBe(false)
})

test('exact selected item reports native metadata independently of analysis sizes and clears stale results on failure', async ({
  page,
}) => {
  await setup(page)
  const dialog = await selected(page)
  const attributes = dialog.getByRole('region', { name: '当前项目的空间属性' })
  await expect(attributes).toContainText('/fixture/analysis/video.mp4')
  await expect(attributes).toContainText('64 MB')
  await expect(attributes).toContainText('0 B')
  await expect(attributes).toContainText('不能证明同步完成')
  await expect(attributes).toContainText('不等于独占空间或可释放空间')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await dialog.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/space-explanation-${theme}.png`,
      animations: 'disabled',
    })
  }
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('space-calls')!))
  expect(calls).toEqual([
    { command: 'inspect_space_item', args: { path: '/fixture/analysis/video.mp4' } },
  ])
  await page.evaluate(() => sessionStorage.setItem('space-mode', 'unavailable'))
  await dialog.getByRole('button', { name: '重新读取属性', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('旧结果不再适用')
  await expect(attributes).not.toContainText('64 MB')
  await dialog.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await expect(page.getByRole('region', { name: '选中项目详情' })).toContainText('video.mp4')
})

test('placeholder hints and unknown folder totals stay explicit; mismatched observations are rejected', async ({
  page,
}) => {
  await setup(page)
  await page.evaluate(() => sessionStorage.setItem('space-mode', 'placeholder'))
  const dialog = await selected(page, 'Projects')
  const attributes = dialog.getByRole('region', { name: '当前项目的空间属性' })
  await expect(attributes).toContainText('这是文件夹，未汇总内部内容')
  await expect(attributes).toContainText('检测到无本地完整数据的占位标记')
  await expect(attributes).toContainText('云文件提供程序目录')
  expect(await attributes.locator('dd').allTextContents()).toEqual(['—', '—'])
  await page.evaluate(() => sessionStorage.setItem('space-mode', 'mismatch'))
  await dialog.getByRole('button', { name: '重新读取属性', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('空间属性无法核对')
  await expect(attributes).not.toContainText('云文件提供程序目录')
})
