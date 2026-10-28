import { expect, test } from '@playwright/test'

test('AI menu shows native agent detection, scope and explicit model consent', async ({ page }) => {
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: 'AI 深度扫描' })
    .click()
  await expect(page.getByRole('main', { name: 'AI 深度扫描内容' })).toBeVisible()
  await expect(page.getByRole('button', { name: '开始深度扫描' })).toBeDisabled()
  await expect(
    page.getByRole('checkbox', { name: '允许将本次路径', exact: false }),
  ).not.toBeChecked()
  await expect(
    page.getByText('Agent 在本机运行不代表模型离线运行。', { exact: false }),
  ).toBeVisible()
  await page.getByText('Agent 检测与内置 Skill', { exact: true }).click()
  await expect(page.getByText('MooMole deep scan', { exact: false })).toBeVisible()
})

test('Mole suggestions require selection; metadata stays advisory; protections revoke selection', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/ai-scan.tsx')).mount())
  const fixture = page.locator('#ai-fixture')
  const candidate = fixture.getByRole('checkbox', {
    name: '选择 AI 建议 /Users/demo/Library/Logs/a.log',
  })
  const extra = fixture.getByRole('checkbox', {
    name: '选择 AI 建议 /Users/demo/Downloads/archive.zip',
  })
  await expect(candidate).not.toBeChecked()
  await expect(extra).toBeDisabled()
  await expect(fixture.getByRole('button', { name: '审阅所选 0 项' })).toBeDisabled()
  await candidate.check()
  await fixture.getByRole('button', { name: '审阅所选 1 项' }).click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('ai-plan')!))).toEqual({
    scanId: 'scan',
    ids: ['scan:0'],
  })
  await fixture.getByRole('button', { name: '新增保护' }).click()
  await expect(candidate).not.toBeChecked()
  await expect(candidate).toBeDisabled()
  await fixture.getByRole('textbox', { name: '搜索 AI 建议' }).fill('archive')
  await expect(candidate).toHaveCount(0)
  await expect(extra).toBeVisible()
})

test('expired AI results cannot submit original Mole suggestions', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/ai-scan.tsx')).mount())
  const fixture = page.locator('#ai-fixture')
  await fixture
    .getByRole('checkbox', { name: '选择 AI 建议 /Users/demo/Library/Logs/a.log' })
    .check()
  await fixture.getByRole('button', { name: '使清单过期' }).click()
  await expect(fixture.getByRole('button', { name: '审阅所选 0 项' })).toBeDisabled()
})

test('native transport requires consent, retains running scan across navigation and stops without replay', async ({
  page,
}) => {
  const { installTaskMock } = await import('./fixtures/native-task')
  await installTaskMock(page, 'new')
  await page.addInitScript(() => {
    const bridge = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const original = bridge.invoke
    bridge.invoke = async (command, args = {}) => {
      if (command === 'detect_ai_agents')
        return [
          {
            id: 'codex',
            name: 'Codex CLI',
            path: '/test/codex',
            supported: true,
            note: '登录待验证',
          },
        ]
      if (command === 'get_ai_scan')
        return JSON.parse(sessionStorage.getItem('ai-snapshot') ?? 'null')
      if (command === 'start_ai_scan') {
        sessionStorage.setItem(
          'ai-start-count',
          String(Number(sessionStorage.getItem('ai-start-count') ?? '0') + 1),
        )
        sessionStorage.setItem('ai-start-payload', JSON.stringify(args))
        const value = {
          id: 'ai-request',
          agentId: args.agentId,
          root: args.root,
          state: 'running',
          stage: 'Agent 正在分析…',
          startedAt: Date.now(),
          finishedAt: null,
          scanId: null,
          expiresAt: null,
          evidence: [],
          limitations: [],
          report: null,
          error: null,
        }
        sessionStorage.setItem('ai-snapshot', JSON.stringify(value))
        return value
      }
      if (command === 'cancel_ai_scan') {
        sessionStorage.setItem('ai-cancel', String(args.requestId))
        const value = JSON.parse(sessionStorage.getItem('ai-snapshot')!)
        sessionStorage.setItem(
          'ai-snapshot',
          JSON.stringify({
            ...value,
            state: 'cancelled',
            stage: '扫描已停止',
            finishedAt: Date.now(),
            error: '已停止；没有执行清理。',
          }),
        )
        return
      }
      return original(command, args)
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const nav = page.getByRole('navigation', { name: '主导航' })
  await nav.getByRole('button', { name: 'AI 深度扫描' }).click()
  const start = page.getByRole('button', { name: '开始深度扫描' })
  const consent = page.getByRole('checkbox', { name: '允许将本次路径', exact: false })
  await expect(page.getByRole('combobox', { name: '分析 Agent' })).toHaveValue('codex')
  await expect(start).toBeDisabled()
  await consent.check()
  await page.getByRole('textbox', { name: '扩展扫描目录' }).fill('/Users/demo/Downloads/synthetic')
  await expect(consent).not.toBeChecked()
  await consent.check()
  await expect(start).toBeEnabled()
  await start.click()
  await expect(page.getByText('Agent 正在分析…', { exact: true })).toBeVisible()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('ai-start-payload')!)),
  ).toEqual({ agentId: 'codex', root: '/Users/demo/Downloads/synthetic', metadataConsent: true })
  await nav.getByRole('button', { name: '概览', exact: true }).click()
  await nav.getByRole('button', { name: 'AI 深度扫描' }).click()
  await expect(page.getByRole('button', { name: '停止扫描' })).toBeVisible()
  await page.getByRole('button', { name: '停止扫描' }).click()
  await expect(page.getByText('扫描已停止', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('ai-cancel'))).toBe('ai-request')
  await page.reload()
  await nav.getByRole('button', { name: 'AI 深度扫描' }).click()
  await expect(page.getByText('扫描已停止', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('ai-start-count'))).toBe('1')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => (document.documentElement.dataset.theme = value), theme)
    await page.screenshot({ path: `test-results/ai-native-transport-${theme}.png`, fullPage: true })
  }
})
