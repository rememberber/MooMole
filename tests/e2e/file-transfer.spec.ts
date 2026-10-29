import { expect, test, type Page } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'
async function setup(page: Page) {
  await installPersonalMock(page)
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    let release: ((r: any) => void) | null = null
    const read = () => JSON.parse(sessionStorage.getItem('transfer-records') || '[]'),
      save = (r: any) => sessionStorage.setItem('transfer-records', JSON.stringify([r]))
    native.invoke = async (command: string, args: any = {}) => {
      const calls = JSON.parse(sessionStorage.getItem('transfer-calls') || '[]')
      sessionStorage.setItem('transfer-calls', JSON.stringify([...calls, { command, args }]))
      if (command === 'personal_move_strategy') return 'crossVolume'
      if (command === 'get_personal_transfers')
        return { records: read(), activeRequests: release ? [read()[0].requestId] : [] }
      if (command === 'prepare_personal_transfer') {
        const report = JSON.parse(sessionStorage.getItem('personal-report')!),
          items = report.items.filter((i: any) => args.selectedIds.includes(i.id))
        const draft = {
          id: '52345678-1234-4123-8123-123456789abc',
          scanId: args.scanId,
          expiresAt: Date.now() + 30000,
          directory: args.destination,
          conflict: args.conflict,
          items,
          targets: items.map((i: any) => ({
            id: i.id,
            destination: `${args.destination}/${i.name}`,
          })),
        }
        sessionStorage.setItem('transfer-draft', JSON.stringify(draft))
        return draft
      }
      if (command === 'discard_personal_transfer') return
      if (command === 'execute_personal_transfer') {
        const d = JSON.parse(sessionStorage.getItem('transfer-draft')!)
        const r = {
          protocol: 1,
          revision: 'MooMole.cross-volume-move.v1',
          id: d.id,
          requestId: args.requestId,
          createdAt: Date.now(),
          finishedAt: Date.now(),
          state: 'completed',
          reviewedAt: null,
          reason: '目标副本与原件收据分别保存。',
          items: d.items.map((i: any) => ({
            id: i.id,
            path: i.path,
            target: d.targets.find((t: any) => t.id === i.id).destination,
            temporaryPath: `${d.directory}/.owned.partial`,
            state: 'moved',
            reason: 'done',
            copyReceipt: {
              destination: d.targets.find((t: any) => t.id === i.id).destination,
              device: '2',
              inode: '20',
            },
            sourceTrashReceipt: {
              destination: `/Users/demo/.Trash/${i.name}`,
              device: '1',
              inode: '10',
            },
            sha256: 'a'.repeat(64),
            metadataSha256: 'b'.repeat(64),
          })),
        }
        if (sessionStorage.getItem('transfer-stop-after-copy')) {
          r.state = 'running'
          r.finishedAt = null as any
          r.items.forEach((i: any) => {
            i.state = 'copyVerified'
            i.sourceTrashReceipt = null
          })
          save(r)
          return new Promise((resolve) => {
            release = resolve
          })
        }
        save(r)
        return r
      }
      if (command === 'cancel_task' && release) {
        const r = read()[0]
        r.state = 'partial'
        r.finishedAt = Date.now()
        r.items.forEach((i: any) => {
          i.state = 'copyOnly'
          i.reason = '已停止，副本保留，未处理原件。'
        })
        save(r)
        release(r)
        release = null
        return
      }
      if (command === 'acknowledge_personal_transfer') {
        const r = read()[0]
        r.reviewedAt = Date.now()
        save(r)
        return r
      }
      return original(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '文件整理', exact: true }).click()
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  const main = page.getByRole('main', { name: '文件整理内容' })
  await main.getByRole('article').filter({ hasText: 'notes.txt' }).getByRole('checkbox').check()
  await main
    .getByRole('textbox', { name: '个人文件移动目标' })
    .fill('/Volumes/Owned Fixture/Target')
  await main.getByRole('button', { name: '移动到目录…', exact: true }).click()
  return page.getByRole('dialog', { name: '跨磁盘移动确认与结果', exact: true })
}
test('another disk routes to explicit verified-copy and original-Trash confirmation with both receipts', async ({
  page,
}) => {
  const d = await setup(page)
  await expect(d.getByRole('region', { name: '跨磁盘确认文件' })).toContainText(
    '/Volumes/Owned Fixture/Target/notes.txt',
  )
  const confirm = d.getByRole('button', { name: '确认跨磁盘移动', exact: true })
  await expect(confirm).toBeDisabled()
  await d.getByRole('checkbox').check()
  await confirm.click()
  await expect(d).toContainText('所选文件已完成跨磁盘移动')
  await expect(d).toContainText('/Users/demo/.Trash/notes.txt')
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('transfer-calls')!))
  expect(calls.filter((c: any) => c.command === 'execute_personal_transfer')).toHaveLength(1)
  expect(calls.some((c: any) => c.command === 'execute_personal_move')).toBe(false)
  expect(
    calls.find((c: any) => c.command === 'prepare_personal_transfer').args.selectedIds,
  ).toHaveLength(1)
})
test('stopping after verified copying keeps a copy-only result and review does not turn it into a moved file', async ({
  page,
}) => {
  const d = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('transfer-stop-after-copy', '1'))
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认跨磁盘移动', exact: true }).click()
  await d.getByRole('button', { name: '停止跨磁盘移动', exact: true }).click()
  await expect(d).toContainText('副本已建立，原件未完成移动')
  await expect(d).not.toContainText('原件废纸篓位置：')
  await d.getByRole('button', { name: '返回文件列表', exact: true }).click()
  await page.getByRole('navigation').getByRole('button', { name: '操作历史', exact: true }).click()
  const records = page.getByRole('region', { name: '跨磁盘移动记录', exact: true })
  await records.getByRole('button').filter({ hasText: '部分完成' }).click()
  const detail = page.getByRole('dialog', { name: '跨磁盘移动记录', exact: true })
  await detail.getByRole('checkbox').check()
  await detail.getByRole('button', { name: '记录已核对', exact: true }).click()
  await expect(detail).toContainText('部分完成')
  await expect(detail.getByRole('checkbox')).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('transfer-calls')!))
  expect(calls.filter((c: any) => c.command === 'execute_personal_transfer')).toHaveLength(1)
})
test('reloading reads retained transfer evidence and does not submit another move', async ({
  page,
}) => {
  const d = await setup(page)
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认跨磁盘移动', exact: true }).click()
  await expect(d).toContainText('所选文件已完成跨磁盘移动')
  await page.reload()
  await page.getByRole('navigation').getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.getByRole('region', { name: '跨磁盘移动记录', exact: true })).toContainText(
    '所选文件已完成跨磁盘移动',
  )
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('transfer-calls')!))
  expect(calls.filter((c: any) => c.command === 'execute_personal_transfer')).toHaveLength(1)
})
