import { test, expect, type Page } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'
async function setup(page: Page, mode = 'normal') {
  await installPersonalMock(page)
  await page.addInitScript((mode) => {
    const bridge = (window as any).__TAURI_INTERNALS__,
      original = bridge.invoke
    const uuid = (n: number) => `${String(n).padStart(8, '0')}-1234-4123-8123-123456789abc`
    const root = mode === 'nested' ? '/Users/demo/Downloads/AI-fixture' : '/Users/demo/Downloads'
    const snapshot = {
      id: uuid(20),
      agentId: 'codex',
      root,
      state: 'succeeded',
      stage: '分析完成，等待审阅',
      startedAt: Date.now() - 1000,
      finishedAt: Date.now() - 500,
      scanId: null,
      expiresAt: null,
      personalExpiresAt: mode === 'expired' ? Date.now() - 1 : Date.now() + 60000,
      evidence: [
        {
          id: 'e0',
          source: 'metadata',
          path: `${root}/old.zip`,
          size: 200,
          modifiedAt: 1,
          isDirectory: false,
          category: 'file',
          candidateId: null,
          personalFile: true,
        },
      ],
      limitations: ['Only synthetic browser metadata'],
      error: null,
      report: {
        summary: 'Keep the personal archive; backup status is unknown.',
        findings: [
          {
            evidenceId: 'e0',
            recommendation: 'keep',
            reason: 'May be a unique personal copy.',
            impact: 'Preview and verify your own backup.',
          },
        ],
        limitations: [],
      },
    }
    bridge.invoke = async (command: string, args: any = {}) => {
      if (command === 'get_ai_scan') return snapshot
      if (command === 'detect_ai_agents') return []
      if (command === 'start_personal_from_ai') {
        const all = JSON.parse(sessionStorage.getItem('ai-file-calls') || '[]')
        sessionStorage.setItem('ai-file-calls', JSON.stringify([...all, { command, args }]))
        if (args.snapshotId !== snapshot.id || JSON.stringify(args.evidenceIds) !== '["e0"]')
          throw Error('wrong opaque source IDs')
        if (mode === 'changed') throw Error('AI 关注项在分析后已变化，未自动匹配替代文件。')
        const report = {
          id: uuid(21),
          root,
          createdAt: Date.now(),
          expiresAt: Date.now() + 900000,
          complete: true,
          cancelled: false,
          skipped: 0,
          visited: 2,
          items: [
            { id: uuid(22), path: `${root}/old.zip`, name: 'old.zip', size: 200, modifiedAt: 1 },
            { id: uuid(23), path: `${root}/notes.txt`, name: 'notes.txt', size: 30, modifiedAt: 2 },
          ],
          aiFocus: {
            snapshotId: snapshot.id,
            agentId: 'codex',
            analyzedAt: snapshot.finishedAt,
            items: [
              {
                evidenceId: 'e0',
                itemId: uuid(22),
                path: mode === 'mismatch' ? '/other' : `${root}/old.zip`,
                recommendation: 'keep',
                reason: snapshot.report.findings[0].reason,
                impact: snapshot.report.findings[0].impact,
              },
            ],
          },
        }
        sessionStorage.setItem('personal-report', JSON.stringify(report))
        sessionStorage.setItem(
          'personal-state',
          JSON.stringify({
            phase: 'ready',
            requestId: args.requestId,
            runId: null,
            report,
            error: null,
          }),
        )
        return root
      }
      return original(command, args)
    }
  }, mode)
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: 'AI 深度扫描', exact: true })
    .click()
}
async function transfer(page: Page) {
  const ai = page.getByRole('main', { name: 'AI 深度扫描内容' })
  await expect(ai.getByRole('button', { name: '去文件整理…' })).toBeDisabled()
  await ai.getByRole('checkbox', { name: /带入文件整理/ }).check()
  await ai.getByRole('button', { name: '去文件整理…' }).click()
  return page.getByRole('main', { name: '文件整理内容' })
}
test('AI keep advice is rescanned into fresh file IDs and requires a second selection and confirmation', async ({
  page,
}) => {
  await setup(page)
  const files = await transfer(page)
  await expect(files.getByRole('region', { name: 'AI 关注项说明' })).toBeVisible()
  await expect(files.getByRole('region', { name: '个人文件列表' }).locator('article')).toHaveCount(
    1,
  )
  await expect(
    files.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip', exact: true }),
  ).not.toBeChecked()
  await expect(files.getByRole('button', { name: '移入废纸篓…', exact: true })).toBeDisabled()
  await files.getByText('查看当时的 AI 说明', { exact: true }).click()
  await expect(files).toContainText('AI 建议保留；整理不代表可以删除。')
  await files.getByRole('checkbox', { name: '只显示本轮 AI 关注项', exact: true }).uncheck()
  await expect(files.getByRole('region', { name: '个人文件列表' }).locator('article')).toHaveCount(
    2,
  )
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => {
      document.documentElement.dataset.theme = t
    }, theme)
    await page.screenshot({
      path: `test-results/ai-personal-files-${theme}.png`,
      animations: 'disabled',
    })
  }
  await files
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip', exact: true })
    .check()
  await files.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  const d = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(d.getByRole('button', { name: '确认移入废纸篓', exact: true })).toBeDisabled()
  const before = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('personal-calls') || '[]'),
  )
  expect(before.some((c: any) => c.command === 'execute_personal_trash')).toBe(false)
  expect(before.find((c: any) => c.command === 'prepare_personal_trash').args).toMatchObject({
    scanId: '00000021-1234-4123-8123-123456789abc',
    selectedIds: ['00000022-1234-4123-8123-123456789abc'],
  })
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '确认移入废纸篓', exact: true }).click()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('ai-file-calls')!))
  expect(calls).toHaveLength(1)
  expect(calls[0].args).not.toHaveProperty('path')
  expect(calls[0].args).not.toHaveProperty('root')
})
test('browser reload reconnects the original personal folder and hints without another model or scan request', async ({
  page,
}) => {
  await setup(page, 'nested')
  await transfer(page)
  await page.reload()
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '文件整理', exact: true })
    .click()
  const files = page.getByRole('main', { name: '文件整理内容' })
  await expect(files.getByRole('textbox', { name: '个人文件目录' })).toHaveValue(
    '/Users/demo/Downloads/AI-fixture',
  )
  await expect(files.getByRole('region', { name: 'AI 关注项说明' })).toBeVisible()
  await expect(
    files.getByRole('checkbox', {
      name: '选择 /Users/demo/Downloads/AI-fixture/old.zip',
      exact: true,
    }),
  ).not.toBeChecked()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('ai-file-calls')!).length),
  ).toBe(1)
})
test('expired personal hints never dispatch a handoff', async ({ page }) => {
  await setup(page, 'expired')
  await expect(page.getByRole('checkbox', { name: /带入文件整理/ })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('ai-file-calls'))).toBeNull()
})
for (const mode of ['changed', 'mismatch'])
  test(`a ${mode} handoff never falls back to paths or execution`, async ({ page }) => {
    await setup(page, mode)
    const files = await transfer(page)
    await expect(files).toContainText(
      mode === 'changed' ? '未自动匹配替代文件' : '个人文件数据无法核对',
    )
    const calls = await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('personal-calls') || '[]'),
    )
    expect(calls.some((c: any) => /prepare_personal|execute_personal/.test(c.command))).toBe(false)
  })
