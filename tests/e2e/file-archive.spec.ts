import { expect, test } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'
async function setup(page: import('@playwright/test').Page) {
  await installPersonalMock(page)
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    let release: ((r: any) => void) | null = null
    const save = (r: any) => sessionStorage.setItem('file-archive-records', JSON.stringify([r]))
    native.invoke = async (command: string, args: any = {}) => {
      if (
        ![
          'prepare_personal_archive',
          'execute_personal_archive',
          'discard_personal_archive',
          'get_personal_archives',
          'acknowledge_personal_archive',
          'cancel_task',
        ].includes(command)
      )
        return original(command, args)
      const calls = JSON.parse(sessionStorage.getItem('file-archive-calls') || '[]')
      sessionStorage.setItem('file-archive-calls', JSON.stringify([...calls, { command, args }]))
      if (command === 'get_personal_archives')
        return JSON.parse(sessionStorage.getItem('file-archive-records') || '[]')
      if (command === 'prepare_personal_archive') {
        if (sessionStorage.getItem('archive-name-conflict'))
          throw Error('归档名称已存在，不会覆盖。')
        const report = JSON.parse(sessionStorage.getItem('personal-report')!)
        const items = report.items.filter((i: any) => args.selectedIds.includes(i.id))
        const draft = {
          id: '52345678-1234-4123-8123-123456789abc',
          scanId: report.id,
          expiresAt: Date.now() + 30000,
          outputPath: `${args.directory}/${args.filename}`,
          items,
          names: items.map((i: any) => i.path.slice(report.root.length + 1)),
        }
        sessionStorage.setItem('file-archive-draft', JSON.stringify(draft))
        return draft
      }
      if (command === 'discard_personal_archive') return
      if (command === 'acknowledge_personal_archive') {
        const r = JSON.parse(sessionStorage.getItem('file-archive-records')!)[0]
        r.reviewedAt = Date.now()
        save(r)
        return r
      }
      if (command === 'cancel_task' && release) {
        const r = JSON.parse(sessionStorage.getItem('file-archive-records')!)[0]
        r.state = 'cancelled'
        r.phase = 'finished'
        r.finishedAt = Date.now()
        r.reason = '原文件保持原样。'
        r.entries.forEach((e: any) => (e.state = 'notArchived'))
        r.artifact = null
        save(r)
        release(r)
        release = null
        return
      }
      if (command === 'execute_personal_archive') {
        const d = JSON.parse(sessionStorage.getItem('file-archive-draft')!)
        const r = {
          id: d.id,
          requestId: args.requestId,
          createdAt: Date.now(),
          finishedAt: Date.now(),
          state: 'completed',
          phase: 'finished',
          outputPath: d.outputPath,
          temporaryPath: '/Users/demo/Documents/.MooMole-archive.partial',
          reviewedAt: null,
          reason: 'ZIP 已生成并逐项校验，原文件保持原样。',
          entries: d.items.map((i: any, index: number) => ({
            id: i.id,
            path: i.path,
            name: d.names[index],
            size: i.size,
            state: 'archived',
            sha256: 'a'.repeat(64),
          })),
          artifact: { device: 1, inode: 2, size: 120, sha256: 'b'.repeat(64) },
        }
        if (sessionStorage.getItem('archive-pending')) {
          r.state = 'running'
          r.finishedAt = null as any
          save(r)
          return new Promise((resolve) => {
            release = resolve
          })
        }
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
  await main.getByRole('button', { name: '归档为 ZIP…', exact: true }).click()
  return page.getByRole('dialog', { name: '归档所选文件', exact: true })
}
test('selected file archive uses explicit destination, separate confirmation and persistent result', async ({
  page,
}) => {
  const dialog = await setup(page)
  await expect(dialog).toContainText('原文件保留')
  await dialog.getByRole('button', { name: '检查归档计划', exact: true }).click()
  await expect(dialog).toContainText('/Users/demo/Documents/MooMole-files.zip')
  const confirm = dialog.getByRole('button', { name: '确认创建 ZIP', exact: true })
  await expect(confirm).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await confirm.click()
  await expect(dialog.getByRole('region', { name: '文件归档结果' })).toContainText(
    '归档已生成并校验',
  )
  await dialog.getByRole('button', { name: '返回文件列表', exact: true }).click()
  await page.getByRole('navigation').getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.getByRole('region', { name: '文件归档记录' })).toContainText('归档已生成并校验')
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('file-archive-calls')!))
  expect(
    calls.find((c: any) => c.command === 'prepare_personal_archive').args.selectedIds,
  ).toHaveLength(1)
  expect(calls.filter((c: any) => c.command === 'execute_personal_archive')).toHaveLength(1)
})
test('archive collision stops before execution and cancellation does not claim a completed ZIP', async ({
  page,
}) => {
  const dialog = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('archive-name-conflict', '1'))
  await dialog.getByRole('button', { name: '检查归档计划', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('不会覆盖')
  await page.evaluate(() => {
    sessionStorage.removeItem('archive-name-conflict')
    sessionStorage.setItem('archive-pending', '1')
  })
  await dialog.getByRole('button', { name: '检查归档计划', exact: true }).click()
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认创建 ZIP', exact: true }).click()
  await dialog.getByRole('button', { name: '停止归档', exact: true }).click()
  await expect(dialog.getByRole('region', { name: '文件归档结果' })).toContainText('归档已取消')
  await expect(dialog.getByRole('button', { name: '确认创建 ZIP', exact: true })).toHaveCount(0)
})
test('interrupted archive history never replays and acknowledgement leaves its outcome unknown', async ({
  page,
}) => {
  const dialog = await setup(page)
  await dialog.getByRole('button', { name: '返回文件列表', exact: true }).click()
  await page.evaluate(() =>
    sessionStorage.setItem(
      'file-archive-records',
      JSON.stringify([
        {
          id: '62345678-1234-4123-8123-123456789abc',
          requestId: '72345678-1234-4123-8123-123456789abc',
          createdAt: Date.now(),
          finishedAt: Date.now(),
          state: 'interrupted',
          phase: 'building',
          outputPath: '/Users/demo/Documents/owned.zip',
          temporaryPath: '/Users/demo/Documents/.partial',
          reviewedAt: null,
          entries: [],
          artifact: null,
          reason: '原文件保留，不会重放。',
        },
      ]),
    ),
  )
  await page.reload()
  await page.getByRole('navigation').getByRole('button', { name: '操作历史', exact: true }).click()
  const records = page.getByRole('region', { name: '文件归档记录' })
  await records.getByRole('button').filter({ hasText: '归档曾中断' }).click()
  const detail = page.getByRole('dialog', { name: '文件归档记录', exact: true })
  await detail.getByRole('checkbox').check()
  await detail.getByRole('button', { name: '记录已核对', exact: true }).click()
  await expect(detail).toContainText('归档曾中断')
  await expect(detail.getByRole('checkbox')).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('file-archive-calls')!))
  expect(calls.some((c: any) => c.command === 'execute_personal_archive')).toBe(false)
})
