import { expect, test, type Page } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'
async function setup(page: Page) {
  await installPersonalMock(page)
  await page.addInitScript(() => {
    const native = (window as any).__TAURI_INTERNALS__,
      original = native.invoke
    const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`,
      root = '/Users/demo/Library/LaunchAgents',
      suffix = '.moomole-disabled'
    let release: ((r: any) => void) | null = null
    const read = () =>
      JSON.parse(sessionStorage.getItem('startup-view') || 'null') || {
        report: null,
        records: [],
        activeRequest: null,
      }
    const save = (v: any) => sessionStorage.setItem('startup-view', JSON.stringify(v))
    native.invoke = async (command: string, args: any = {}) => {
      const calls = JSON.parse(sessionStorage.getItem('startup-calls') || '[]')
      sessionStorage.setItem('startup-calls', JSON.stringify([...calls, { command, args }]))
      if (command === 'get_startup_management') return read()
      if (command === 'open_login_item_settings') return
      if (command === 'scan_startup_management') {
        const v = read(),
          paused = !!sessionStorage.getItem('startup-paused'),
          now = Date.now()
        v.report = {
          id: uuid(1),
          root,
          createdAt: now - 1,
          expiresAt: now + 299999,
          complete: true,
          cancelled: false,
          skipped: 0,
          items: [
            {
              id: uuid(2),
              name: 'org.fixture.agent',
              label: 'org.fixture.agent',
              program: '/Applications/Fixture.app/Contents/MacOS/helper',
              arguments: ['/Applications/Fixture.app/Contents/MacOS/helper'],
              path: `${root}/org.fixture.agent.plist${paused ? suffix : ''}`,
              paused,
              eligible: true,
              reason: '',
            },
            {
              id: uuid(3),
              name: 'unrecorded',
              label: 'org.fixture.unknown',
              program: '/Applications/Unknown.app/Contents/MacOS/helper',
              arguments: ['/Applications/Unknown.app/Contents/MacOS/helper'],
              path: `${root}/unrecorded.plist${suffix}`,
              paused: true,
              eligible: false,
              reason: '缺少可核对的暂停收据。',
            },
          ],
        }
        save(v)
        return v.report
      }
      if (command === 'prepare_startup_management') {
        const v = read(),
          row = v.report?.items.find((i: any) => i.id === args.itemId)
        if (!row || !row.eligible || args.scanId !== v.report.id) throw Error('wrong source')
        const draft = {
          id: uuid(10 + v.records.length),
          scanId: v.report.id,
          itemId: row.id,
          action: args.action,
          expiresAt: Date.now() + 30000,
          path: row.path,
          target: row.paused ? row.path.slice(0, -suffix.length) : `${row.path}${suffix}`,
          label: row.label,
          program: row.program,
          arguments: row.arguments,
        }
        sessionStorage.setItem('startup-draft', JSON.stringify(draft))
        return draft
      }
      if (command === 'discard_startup_management') return
      if (command === 'execute_startup_management') {
        const d = JSON.parse(sessionStorage.getItem('startup-draft')!),
          v = read()
        if (args.id !== d.id || args.confirmed !== true) throw Error('wrong confirmation')
        const r = {
          id: d.id,
          requestId: args.requestId,
          action: d.action,
          path: d.path,
          target: d.target,
          label: d.label,
          program: d.program,
          createdAt: Date.now(),
          finishedAt: Date.now(),
          state: 'completed',
          phase: 'finished',
          configurationChanged: true,
          receipt: { destination: d.target, device: '1', inode: '2' },
          reviewedAt: null,
          reason: '配置文件保留，后台注册观察已记录。',
        }
        if (d.action === 'pause') sessionStorage.setItem('startup-paused', '1')
        else sessionStorage.removeItem('startup-paused')
        v.records.unshift(r)
        v.report = null
        if (sessionStorage.getItem('startup-partial-cancel')) {
          r.state = 'running'
          r.finishedAt = null as any
          v.activeRequest = args.requestId
          save(v)
          return new Promise((resolve) => {
            release = resolve
          })
        }
        save(v)
        return r
      }
      if (command === 'cancel_task' && release) {
        const v = read(),
          r = v.records[0]
        r.state = 'partial'
        r.finishedAt = Date.now()
        r.reason = '配置已暂停，后台状态待核实。'
        v.activeRequest = null
        save(v)
        release(r)
        release = null
        return
      }
      if (command === 'acknowledge_startup_management') {
        const v = read(),
          r = v.records.find((r: any) => r.id === args.id)
        r.reviewedAt = Date.now()
        save(v)
        return r
      }
      return original(command, args)
    }
  })
  await page.goto('/')
  await page.getByRole('navigation').getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('tab', { name: '启动与后台', exact: true }).click()
  return page.getByRole('region', { name: '启动与后台管理', exact: true })
}
test('startup pause and resume use fresh exact IDs, separate confirmations and retained configuration evidence', async ({
  page,
}) => {
  const area = await setup(page)
  await page.getByRole('button', { name: '读取启动配置', exact: true }).click()
  const list = area.getByRole('region', { name: '用户启动配置列表' })
  await expect(list.getByRole('article')).toHaveCount(2)
  await expect(list.getByRole('article').nth(1).getByRole('button')).toBeDisabled()
  for (const action of ['pause', 'resume']) {
    if (action === 'resume')
      await page.getByRole('button', { name: '读取启动配置', exact: true }).click()
    await list
      .getByRole('article')
      .first()
      .getByRole('button', {
        name: action === 'pause' ? '暂停此配置…' : '恢复此配置…',
        exact: true,
      })
      .click()
    const dialog = page.getByRole('dialog', { name: '确认启动配置操作', exact: true })
    await expect(dialog).toContainText('/Users/demo/Library/LaunchAgents/org.fixture.agent.plist')
    const confirm = dialog.getByRole('button', { name: '确认更改此配置', exact: true })
    await expect(confirm).toBeDisabled()
    await dialog.getByRole('checkbox').check()
    await confirm.click()
    const result = page.getByRole('dialog', { name: '启动操作结果', exact: true })
    await expect(result).toContainText(action === 'pause' ? '已暂停此登录配置' : '已恢复此登录配置')
    await result.getByRole('button', { name: '返回启动列表', exact: true }).click()
  }
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('startup-calls')!))
  expect(calls.filter((c: any) => c.command === 'execute_startup_management')).toHaveLength(2)
  const plans = calls.filter((c: any) => c.command === 'prepare_startup_management')
  expect(plans.map((c: any) => c.args.action)).toEqual(['pause', 'resume'])
  expect(
    plans.every((c: any) => !('path' in c.args) && !('label' in c.args) && !('program' in c.args)),
  ).toBe(true)
  expect(calls.some((c: any) => c.command === 'start_task' && c.args.kind === 'apps')).toBe(false)
})
test('cancellation after configuration change remains partial and acknowledgement never replays or changes it to success', async ({
  page,
}) => {
  const area = await setup(page)
  await page.getByRole('button', { name: '读取启动配置', exact: true }).click()
  await area.getByRole('button', { name: '暂停此配置…', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('startup-partial-cancel', '1'))
  let dialog = page.getByRole('dialog', { name: '确认启动配置操作', exact: true })
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认更改此配置', exact: true }).click()
  await dialog.getByRole('button', { name: '停止启动任务', exact: true }).click()
  dialog = page.getByRole('dialog', { name: '启动操作结果', exact: true })
  await expect(dialog).toContainText('部分完成，请核对')
  await dialog.getByRole('button', { name: '返回启动列表', exact: true }).click()
  await page
    .getByRole('region', { name: '启动管理记录', exact: true })
    .getByRole('button')
    .filter({ hasText: '部分完成，请核对' })
    .click()
  dialog = page.getByRole('dialog', { name: '启动管理记录', exact: true })
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '记录已核对', exact: true }).click()
  await expect(dialog).toContainText('部分完成，请核对')
  await expect(dialog.getByRole('checkbox')).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('startup-calls')!))
  expect(calls.filter((c: any) => c.command === 'execute_startup_management')).toHaveLength(1)
})
test('expired startup lists are not actionable and system settings use one fixed native entry', async ({
  page,
}) => {
  const area = await setup(page)
  await page.getByRole('button', { name: '读取启动配置', exact: true }).click()
  await page.evaluate(() => {
    const v = JSON.parse(sessionStorage.getItem('startup-view')!)
    v.report.createdAt = Date.now() - 10000
    v.report.expiresAt = Date.now() - 1
    sessionStorage.setItem('startup-view', JSON.stringify(v))
  })
  await page.getByRole('tab', { name: '已安装应用', exact: true }).click()
  await page.getByRole('tab', { name: '启动与后台', exact: true }).click()
  // Return through history to explicitly refresh the retained native view.
  await page.getByRole('navigation').getByRole('button', { name: '操作历史', exact: true }).click()
  await page.getByRole('navigation').getByRole('button', { name: '应用列表', exact: true }).click()
  await page.reload()
  await page.getByRole('navigation').getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('tab', { name: '启动与后台', exact: true }).click()
  await expect(area).toContainText('清单已过期')
  await expect(area.getByRole('button', { name: '暂停此配置…', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '系统登录项设置', exact: true }).click()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('startup-calls')!))
  expect(calls.filter((c: any) => c.command === 'open_login_item_settings')).toHaveLength(1)
  expect(calls.some((c: any) => c.command === 'execute_startup_management')).toBe(false)
})
