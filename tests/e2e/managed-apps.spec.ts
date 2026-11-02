import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'
const planId = '7f786cf6-756c-49a7-b5a7-397fbb7cd15d'
type Call = { command: string; args: Record<string, unknown> }
async function calls(page: Page): Promise<Call[]> {
  return page.evaluate(() => JSON.parse(sessionStorage.getItem('managed-calls') ?? '[]'))
}
async function setup(page: Page, mode = 'normal', upgrade = false) {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.addInitScript((mode) => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    let draft: Record<string, unknown>, release: (v: unknown) => void
    const records = () => JSON.parse(sessionStorage.getItem('managed-rows') ?? '[]')
    const save = (rows: unknown[]) => sessionStorage.setItem('managed-rows', JSON.stringify(rows))
    native.invoke = async (command, args = {}) => {
      if (command.includes('managed_app') || command === 'cancel_task') {
        const log = JSON.parse(sessionStorage.getItem('managed-calls') ?? '[]')
        sessionStorage.setItem('managed-calls', JSON.stringify([...log, { command, args }]))
      }
      if (command === 'get_managed_apps') return records()
      if (command === 'list_executions') return []
      if (command === 'prepare_managed_app' || command === 'prepare_managed_app_upgrade') {
        if (mode === 'unsupported') throw Error('包内容不受支持，请使用原管理工具。')
        const now = Date.now() + (mode === 'future' ? 60000 : 0)
        draft = {
          id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
          taskId: args.taskId,
          index: args.index,
          name: 'Example',
          bundleId: 'org.example.same',
          path:
            mode === 'mismatch'
              ? '/Applications/Other.app'
              : '/Users/demo/Applications/Example Beta.app',
          token: 'example',
          installedVersion: '1.0',
          managerPath: '/opt/homebrew/bin/brew',
          metadataSha256: 'a'.repeat(64),
          createdAt: now,
          expiresAt: now + 120000,
        }
        if (command === 'prepare_managed_app_upgrade')
          draft.upgrade = {
            version: '2.0',
            url: 'https://example.org/owned.zip',
            sha256: 'b'.repeat(64),
            publisher: 'adHoc',
          }
        if (mode === 'upgrade-wrong-action') delete draft.upgrade
        if (mode === 'late')
          return new Promise((resolve) => {
            release = resolve
          })
        return draft
      }
      if (command === 'fixture_release') {
        release(draft)
        return null
      }
      if (command === 'fixture_finish') {
        save([{ ...records()[0], state: 'removed', phase: 'finished', finishedAt: Date.now() }])
        return null
      }
      if (command === 'discard_managed_app') return null
      if (command === 'execute_managed_app') {
        const record = {
          id: draft.id,
          draft,
          requestId: args.requestId,
          createdAt: Date.now(),
          finishedAt: ['cancel', 'lost'].includes(mode) ? null : Date.now(),
          reviewedAt: null,
          state: ['cancel', 'lost'].includes(mode)
            ? 'running'
            : draft.upgrade
              ? 'upgraded'
              : 'removed',
          ...(draft.upgrade && !['cancel', 'lost'].includes(mode)
            ? {
                upgradeResult: {
                  version: '2.0',
                  publisher: 'adHoc',
                  bundleSha256: 'c'.repeat(64),
                  receiptSha256: 'd'.repeat(64),
                },
              }
            : {}),
          phase: ['cancel', 'lost'].includes(mode) ? 'intent' : 'finished',
          reason:
            'Homebrew 命令已成功返回，且重新读取未发现该包登记或原 App 路径。未使用 zap、force 或自动移除依赖；此流程不提供废纸篓收据或应用内恢复。',
        }
        save([record])
        if (mode === 'lost') throw Error('fixture response lost; operation is still owned')
        if (mode === 'cancel')
          return new Promise((resolve) => {
            release = resolve
          })
        return record
      }
      if (command === 'cancel_task' && mode === 'cancel' && records().length) {
        const r = {
          ...records()[0],
          state: 'unknown',
          phase: 'finished',
          finishedAt: Date.now(),
          reason: '停止后结果待核实，不自动重试。',
        }
        save([r])
        release(r)
        return null
      }
      if (command === 'cancel_task' && mode === 'late') return null
      if (command === 'cancel_task' && mode === 'lost') return null
      if (command === 'acknowledge_managed_app') {
        const r = { ...records()[0], reviewedAt: Date.now() }
        save([r])
        return r
      }
      return original(command, args)
    }
  }, mode)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
  await expect(rows).toHaveCount(5)
  await rows
    .nth(1)
    .getByRole('button', { name: upgrade ? '通过 Homebrew 升级…' : '卸载…', exact: true })
    .click()
  return page.getByRole('dialog', { name: upgrade ? '通过 Homebrew 升级' : '通过 Homebrew 卸载' })
}
test('actual Homebrew row uses manager review and a separate explicit confirmation, never native Trash', async ({
  page,
}, info) => {
  const dialog = await setup(page)
  await expect(dialog).toContainText('/Users/demo/Applications/Example Beta.app')
  await expect(dialog).toContainText('不是移入废纸篓')
  await expect(dialog).toContainText('/opt/homebrew/bin/brew')
  await expect(dialog).not.toContainText('确认时间无法核对或已过期')
  const confirm = dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true })
  await expect(confirm).toBeDisabled()
  const before = await calls(page)
  const prepared = before.filter((c) => c.command === 'prepare_managed_app')
  expect(prepared).toHaveLength(1)
  expect(prepared[0].args.index).toBe(1)
  expect(prepared[0].args).not.toHaveProperty('path')
  expect(prepared[0].args).not.toHaveProperty('token')
  expect(before.filter((c) => c.command === 'execute_managed_app')).toHaveLength(0)
  await dialog.getByRole('checkbox').check()
  await expect(confirm).toBeEnabled()
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('managed-zh.png'), animations: 'disabled' })
  await confirm.click()
  await expect(dialog).toContainText('已核对包与原 App 不再存在')
  await dialog.getByRole('button', { name: '返回应用列表' }).click()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Homebrew 操作历史' })).toContainText(
    '已核对包与原 App 不再存在',
  )
  const log = await calls(page)
  expect(log.filter((c) => c.command === 'execute_managed_app')).toHaveLength(1)
  expect(log.find((c) => c.command === 'execute_managed_app')?.args).toMatchObject({
    id: planId,
    confirmed: true,
  })
  const all = (await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-native-commands') ?? '[]'),
  )) as Call[]
  expect(all.some((c) => /execute.*uninstall|prepare.*uninstall/.test(c.command))).toBe(false)
})
for (const mode of ['unsupported', 'mismatch'])
  test(`${mode} cannot fall back to ordinary App deletion`, async ({ page }) => {
    const dialog = await setup(page, mode)
    await expect(dialog.getByRole('alert')).toContainText(
      mode === 'mismatch' ? '无法核对' : '不受支持',
    )
    await expect(
      dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }),
    ).toHaveCount(0)
    expect((await calls(page)).filter((c) => c.command === 'execute_managed_app')).toHaveLength(0)
    await dialog.getByRole('button', { name: '返回应用列表' }).click()
  })
test('expired confirmation is disabled and closing withdraws its exact plan', async ({ page }) => {
  await page.clock.install()
  const dialog = await setup(page)
  await dialog.getByRole('checkbox').check()
  await expect(
    dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }),
  ).toBeEnabled()
  await page.clock.fastForward(121000)
  await expect(dialog).toContainText('确认时间无法核对或已过期')
  await expect(
    dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }),
  ).toBeDisabled()
  await dialog.getByRole('button', { name: '返回应用列表' }).click()
  await expect
    .poll(async () => (await calls(page)).some((c) => c.command === 'discard_managed_app'))
    .toBe(true)
  expect((await calls(page)).find((c) => c.command === 'discard_managed_app')?.args).toEqual({
    id: planId,
  })
})
test('a backwards wall clock cannot authorize the current plan', async ({ page }) => {
  const dialog = await setup(page, 'future')
  await dialog.getByRole('checkbox').check()
  await expect(dialog).toContainText('确认时间无法核对或已过期')
  await expect(
    dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }),
  ).toBeDisabled()
  expect((await calls(page)).filter((c) => c.command === 'execute_managed_app')).toHaveLength(0)
})
test('closing preparation cancels its request and discards a late response', async ({ page }) => {
  const dialog = await setup(page, 'late')
  await expect(dialog.getByRole('status')).toContainText('正在检查')
  await dialog.getByRole('button', { name: '关闭弹窗' }).click()
  await expect(dialog).toHaveCount(0)
  await page.evaluate(async () => {
    await (
      window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string) => Promise<unknown> } }
    ).__TAURI_INTERNALS__.invoke('fixture_release')
  })
  await expect
    .poll(async () => (await calls(page)).some((c) => c.command === 'discard_managed_app'))
    .toBe(true)
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'prepare_managed_app')?.args.requestId,
  )
  expect(log.find((c) => c.command === 'discard_managed_app')?.args.id).toBe(planId)
  expect(log.filter((c) => c.command === 'execute_managed_app')).toHaveLength(0)
})
test('stop collects unknown result; reopen preserves it without replay and review is separate', async ({
  page,
}) => {
  const dialog = await setup(page, 'cancel')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }).click()
  await expect(dialog.getByRole('button', { name: '关闭弹窗' })).toHaveCount(0)
  await dialog.getByRole('button', { name: '停止本次 Homebrew 任务' }).click()
  await expect(dialog).toContainText('结果待核实')
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'execute_managed_app')?.args.requestId,
  )
  await dialog.getByRole('button', { name: '返回应用列表' }).click()
  await page.reload()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  const history = page.getByRole('region', { name: 'Homebrew 操作历史' })
  await expect(history).toContainText('结果待核实')
  await history.getByRole('button', { name: '已核查实际 App 与包状态' }).click()
  await expect(history.getByRole('button')).toHaveCount(0)
  expect((await calls(page)).filter((c) => c.command === 'execute_managed_app')).toHaveLength(1)
  expect((await calls(page)).find((c) => c.command === 'acknowledge_managed_app')?.args).toEqual({
    id: planId,
    confirmed: true,
  })
})
test('lost observer keeps tracking after close and reload, releases only after recorded completion and never replays', async ({
  page,
}) => {
  const dialog = await setup(page, 'lost')
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '确认通过 Homebrew 卸载', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('response lost')
  await dialog.getByRole('button', { name: '返回应用列表' }).click()
  await expect(page.getByRole('button', { name: '刷新列表', exact: true })).toBeDisabled()
  await page.reload()
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await expect(page.getByRole('button', { name: '读取应用', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await page
    .getByRole('region', { name: 'Homebrew 操作历史' })
    .getByRole('button', { name: '停止本次 Homebrew 任务' })
    .click()
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'execute_managed_app')?.args.requestId,
  )
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.evaluate(async () => {
    await (
      window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string) => Promise<unknown> } }
    ).__TAURI_INTERNALS__.invoke('fixture_finish')
  })
  await expect(page.getByRole('button', { name: '读取应用', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Homebrew 操作历史' })).toContainText(
    '已核对包与原 App 不再存在',
  )
  expect((await calls(page)).filter((c) => c.command === 'execute_managed_app')).toHaveLength(1)
})
for (const language of ['en', 'ja'] as const)
  test(`review fits in ${language}`, async ({ page }, info) => {
    await setup(page)
    await page.evaluate(async (lang) => {
      ;(await import('/src/i18n/locale.ts')).setLanguagePreference(lang)
    }, language)
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText(
      language === 'en' ? 'Uninstall with Homebrew' : 'Homebrew でアンインストール',
    )
    await page.setViewportSize({ width: 960, height: 780 })
    await expect(dialog).not.toContainText(language === 'en' ? 'Confirmation time' : '確認の時刻')
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`managed-${language}.png`),
      animations: 'disabled',
    })
  })

test('Homebrew upgrade has its own review, version transition and proof, with no native uninstall fallback', async ({
  page,
}) => {
  const d = await setup(page, 'normal', true)
  await expect(d).toContainText('准备升级到')
  await expect(d).toContainText('https://example.org/owned.zip')
  await expect(d).toContainText('没有应用内回滚')
  const confirm = d.getByRole('button', { name: '确认通过 Homebrew 升级', exact: true })
  await expect(confirm).toBeDisabled()
  await d.getByRole('checkbox').check()
  await confirm.dblclick()
  await expect(d).toContainText('已核对新版包登记、App 版本与签名')
  const log = await calls(page)
  expect(log.filter((c) => c.command === 'prepare_managed_app_upgrade')).toHaveLength(1)
  expect(log.filter((c) => c.command === 'prepare_managed_app')).toHaveLength(0)
  expect(log.filter((c) => c.command === 'execute_managed_app')).toHaveLength(1)
  await d.getByRole('button', { name: '返回应用列表' }).click()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.getByRole('region', { name: 'Homebrew 操作历史' })).toContainText('1.0 → 2.0')
})
test('an uninstall confirmation cannot be substituted for requested upgrade', async ({ page }) => {
  const d = await setup(page, 'upgrade-wrong-action', true)
  await expect(d.getByRole('alert')).toContainText('无法核对')
  await expect(d.getByRole('button', { name: '确认通过 Homebrew 升级', exact: true })).toHaveCount(
    0,
  )
  expect((await calls(page)).filter((c) => c.command === 'execute_managed_app')).toHaveLength(0)
})

for (const language of ['en', 'ja'] as const)
  test(`upgrade review fits in ${language}`, async ({ page }, info) => {
    await page.setViewportSize({ width: 960, height: 780 })
    await setup(page, 'normal', true)
    await page.evaluate(
      async (lang) => (await import('/src/i18n/locale.ts')).setLanguagePreference(lang),
      language,
    )
    const dialog = page.getByRole('dialog')
    await expect(dialog).toContainText(
      language === 'en' ? 'Upgrade through Homebrew' : 'Homebrew でアップグレード',
    )
    await expect(dialog).toContainText('/Users/demo/Applications/Example Beta.app')
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true)
    await dialog.screenshot({ path: info.outputPath(`managed-upgrade-${language}.png`) })
  })

test('stopping an upgrade preserves unknown and reconnects without preparing another action', async ({
  page,
}) => {
  const d = await setup(page, 'cancel', true)
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认通过 Homebrew 升级', exact: true }).click()
  await expect(d.getByRole('button', { name: '关闭弹窗' })).toHaveCount(0)
  await d.getByRole('button', { name: '停止本次 Homebrew 任务' }).click()
  await expect(d).toContainText('结果待核实')
  await d.getByRole('button', { name: '返回应用列表' }).click()
  await page.reload()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  const history = page.getByRole('region', { name: 'Homebrew 操作历史' })
  await expect(history).toContainText('Homebrew 升级')
  await expect(history).toContainText('结果待核实')
  const log = await calls(page)
  expect(log.filter((c) => c.command === 'execute_managed_app')).toHaveLength(1)
  expect(log.filter((c) => c.command === 'prepare_managed_app_upgrade')).toHaveLength(1)
})
