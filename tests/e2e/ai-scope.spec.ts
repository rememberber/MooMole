import { test, expect, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
async function setup(page: Page, engine = true) {
  await installTaskMock(page)
  await page.addInitScript((engine) => {
    const b = (window as any).__TAURI_INTERNALS__,
      original = b.invoke
    b.invoke = async (command: string, args: any = {}) => {
      if (command === 'bootstrap') {
        const v = await original(command, args)
        return {
          ...v,
          engine: { ...v.engine, available: engine, reason: engine ? null : 'unavailable fixture' },
        }
      }
      if (command === 'detect_ai_agents')
        return [
          { id: 'codex', name: 'Codex CLI', path: '/test/codex', supported: true, note: 'fixture' },
        ]
      if (command === 'get_ai_scan')
        return JSON.parse(sessionStorage.getItem('scope-result') || 'null')
      if (command === 'start_ai_scan') {
        sessionStorage.setItem('scope-request', JSON.stringify(args))
        const result = {
          id: 'scope-request',
          agentId: args.agentId,
          root: args.root,
          includeMole: args.includeMole,
          state: 'running',
          stage: 'scope fixture running',
          startedAt: Date.now(),
          finishedAt: null,
          scanId: null,
          expiresAt: null,
          evidence: [],
          limitations: [],
          report: null,
          error: null,
        }
        sessionStorage.setItem('scope-result', JSON.stringify(result))
        return result
      }
      if (command === 'cancel_ai_scan') {
        const v = JSON.parse(sessionStorage.getItem('scope-result')!)
        v.state = 'cancelled'
        v.stage = '扫描已停止'
        v.finishedAt = Date.now()
        sessionStorage.setItem('scope-result', JSON.stringify(v))
        return
      }
      return original(command, args)
    }
  }, engine)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: 'AI 深度扫描', exact: true })
    .click()
  return page.getByRole('main', { name: 'AI 深度扫描内容' })
}
for (const include of [false, true])
  test(`analysis scope ${include ? 'combined' : 'folder-only'} is explicit and bound into the immutable report`, async ({
    page,
  }) => {
    const main = await setup(page)
    const scope = main.getByRole('combobox', { name: 'AI 分析范围' })
    await expect(scope).toHaveValue('folder')
    await main
      .getByRole('textbox', { name: '扩展扫描目录' })
      .fill('/Users/demo/Downloads/owned-scope')
    const consent = main.getByRole('checkbox', { name: /允许将本次路径/ })
    await consent.check()
    await scope.selectOption('combined')
    await expect(consent).not.toBeChecked()
    await consent.check()
    await scope.selectOption('folder')
    await expect(consent).not.toBeChecked()
    if (include) await scope.selectOption('combined')
    await consent.check()
    await page.getByRole('button', { name: '开始深度扫描', exact: true }).click()
    await expect(scope).toBeDisabled()
    expect(
      await page.evaluate(() => JSON.parse(sessionStorage.getItem('scope-request')!)),
    ).toMatchObject({ includeMole: include, metadataConsent: true })
    await expect(main.getByRole('region', { name: 'AI 扫描结果' })).toContainText(
      include ? '本报告范围：Mole 常用清理位置与所选目录。' : '本报告范围：仅所选目录。',
    )
    await page.reload()
    await page
      .getByRole('navigation', { name: '主导航' })
      .getByRole('button', { name: 'AI 深度扫描', exact: true })
      .click()
    await expect(scope).toHaveValue(include ? 'combined' : 'folder')
    await expect(main.getByRole('textbox', { name: '扩展扫描目录' })).toHaveValue(
      '/Users/demo/Downloads/owned-scope',
    )
    await page.getByRole('button', { name: '停止扫描', exact: true }).click()
  })
test('folder-only metadata analysis works with an unavailable Mole engine while combined analysis stays disabled', async ({
  page,
}) => {
  const main = await setup(page, false)
  const consent = main.getByRole('checkbox', { name: /允许将本次路径/ }),
    start = page.getByRole('button', { name: '开始深度扫描', exact: true })
  await consent.check()
  await expect(start).toBeEnabled()
  await main.getByRole('combobox', { name: 'AI 分析范围' }).selectOption('combined')
  await consent.check()
  await expect(start).toBeDisabled()
  await main.getByRole('combobox', { name: 'AI 分析范围' }).selectOption('folder')
  await consent.check()
  await start.click()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('scope-request')!)),
  ).toMatchObject({ includeMole: false })
})
