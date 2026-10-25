import { test, expect, type Page } from '@playwright/test'
async function mount(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/quicklook-refresh.tsx')).mount(mode),
    mode,
  )
}
async function prepare(page: Page) {
  await page.getByRole('button', { name: '刷新预览…', exact: true }).click()
  const d = page.getByRole('dialog', { name: '刷新预览', exact: true })
  await d.getByRole('button', { name: '检查所选步骤' }).click()
  return d
}
test('read-only prepare needs explicit acknowledgement and submits exactly once', async ({
  page,
}) => {
  await mount(page)
  const d = await prepare(page)
  const action = d.getByRole('button', { name: '开始刷新', exact: true })
  await expect(action).toBeDisabled()
  await expect(page.getByLabel('维护提交次数')).toHaveText('0')
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await action.dblclick()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await expect(result.getByRole('status')).toHaveText('所选命令已完成')
  await expect(page.getByLabel('维护提交次数')).toHaveText('1')
  await result.getByRole('button', { name: '完成', exact: true }).click()
  await page
    .getByRole('region', { name: '维护记录' })
    .getByRole('button', { name: /项完成/ })
    .click()
  await expect(page.getByRole('dialog', { name: '维护结果' })).toContainText('缩略图缓存')
  await expect(page.getByLabel('维护提交次数')).toHaveText('1')
})
test('expired and invalid scope responses cannot execute', async ({ page }) => {
  await page.clock.install()
  await mount(page)
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await page.clock.fastForward(31_000)
  await expect(d.getByRole('button', { name: '开始刷新', exact: true })).toBeDisabled()
  await expect(d).toContainText('确认已过期')
  await mount(page, 'invalid')
  const bad = await prepare(page)
  await expect(bad.getByRole('alert')).toContainText('无法核对')
  await expect(bad.getByRole('button', { name: '开始刷新', exact: true })).toBeDisabled()
})
test('closing a pending preparation discards its late confirmation', async ({ page }) => {
  await mount(page, 'slow')
  const d = await prepare(page)
  await d.getByRole('button', { name: '关闭弹窗' }).click()
  await page.evaluate(() => window.refreshFixture.release())
  await expect(page.getByLabel('维护撤销次数')).toHaveText('1')
  await expect(page.getByRole('dialog', { name: '刷新预览', exact: true })).toHaveCount(0)
  await expect(page.getByLabel('维护提交次数')).toHaveText('0')
})
test('in-flight close retains the worker and exact cancellation preserves first success', async ({
  page,
}) => {
  await mount(page, 'pending')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  let result = page.getByRole('dialog', { name: '维护结果' })
  await result.getByRole('button', { name: '后台继续' }).click()
  await expect(page.getByLabel('维护撤销次数')).toHaveText('0')
  await page.getByRole('button', { name: '查看维护结果' }).click()
  result = page.getByRole('dialog', { name: '维护结果' })
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await expect(result.getByRole('status')).toHaveText('已取消后续步骤')
  await expect(result).toContainText('已完成 1/2 项')
  const calls = await page.evaluate(() => window.refreshFixture.calls)
  expect(calls.find((c) => c.command === 'cancel')?.id).toBe(
    calls.find((c) => c.command === 'execute')?.id,
  )
})
test('cancellation is unavailable before admission and then targets only the observed request', async ({
  page,
}) => {
  await mount(page, 'admission')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await expect(result.getByRole('button', { name: '取消后续步骤' })).toHaveCount(0)
  expect(
    (await page.evaluate(() => window.refreshFixture.calls)).filter((c) => c.command === 'cancel'),
  ).toHaveLength(0)
  await page.evaluate(() => window.refreshFixture.admit())
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await expect(result.getByRole('status')).toHaveText('已取消后续步骤')
  const calls = await page.evaluate(() => window.refreshFixture.calls)
  expect(calls.filter((c) => c.command === 'execute')).toHaveLength(1)
  expect(calls.filter((c) => c.command === 'cancel')).toHaveLength(1)
  expect(
    new Set(calls.filter((c) => ['execute', 'cancel'].includes(c.command)).map((c) => c.id)).size,
  ).toBe(1)
})
test('a late cancellation error cannot overwrite a completed result', async ({ page }) => {
  await mount(page, 'late-cancel-error')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await page.evaluate(() => window.refreshFixture.finish())
  await expect(result.getByRole('status')).toHaveText('所选命令已完成')
  await page.evaluate(() => window.refreshFixture.rejectCancel())
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  )
  expect(await result.getByRole('alert').count()).toBe(0)
  await expect(result.getByRole('status')).toHaveText('所选命令已完成')
  await expect(page.getByLabel('维护提交次数')).toHaveText('1')
})
test('an old cancellation error cannot affect a newly submitted request', async ({ page }) => {
  await mount(page, 'late-cancel-error')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await page.evaluate(() => window.refreshFixture.finish())
  await expect(result.getByRole('status')).toHaveText('所选命令已完成')
  await result.getByRole('button', { name: '完成', exact: true }).click()
  const again = await prepare(page)
  await again.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await again.getByRole('button', { name: '开始刷新', exact: true }).click()
  await expect(result.getByRole('button', { name: '取消后续步骤' })).toBeEnabled()
  await page.evaluate(() => window.refreshFixture.rejectCancel())
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  )
  expect(await result.getByRole('alert').count()).toBe(0)
  const calls = await page.evaluate(() => window.refreshFixture.calls)
  const executions = calls.filter((c) => c.command === 'execute')
  expect(executions).toHaveLength(2)
  expect(executions[0].id).not.toBe(executions[1].id)
  expect(calls.filter((c) => c.command === 'cancel')).toEqual([
    { command: 'cancel', id: executions[0].id },
  ])
})
test('a cancellation failure for the current running request stays visible and can be retried', async ({
  page,
}) => {
  await mount(page, 'cancel-error')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await expect(result.getByRole('alert')).toContainText('取消响应暂时不可用')
  await expect(result.getByRole('button', { name: '取消后续步骤' })).toBeEnabled()
  await result.getByRole('button', { name: '取消后续步骤' }).click()
  await expect(result.getByRole('alert')).toContainText('取消响应暂时不可用')
  const calls = await page.evaluate(() => window.refreshFixture.calls)
  expect(calls.filter((c) => c.command === 'execute')).toHaveLength(1)
  expect(calls.filter((c) => c.command === 'cancel')).toHaveLength(2)
  expect(
    new Set(calls.filter((c) => ['execute', 'cancel'].includes(c.command)).map((c) => c.id)).size,
  ).toBe(1)
})
test('restart observation and human review keep unknown facts and never execute again', async ({
  page,
}) => {
  await mount(page, 'resume')
  await page
    .getByRole('region', { name: '维护记录' })
    .getByRole('button', { name: /维护已中断/ })
    .click()
  const result = page.getByRole('dialog', { name: '维护结果' })
  await expect(result).toContainText('结果待核实')
  await expect(result.getByRole('button', { name: '记录已核查' })).toBeDisabled()
  await result.getByLabel('我已核查结果，了解未知步骤不会自动重试。').check()
  await result.getByRole('button', { name: '记录已核查' }).click()
  await expect(result).toContainText('人工核查，原结果保持')
  await expect(result).toContainText('结果待核实')
  await expect(page.getByLabel('维护提交次数')).toHaveText('0')
})
test('lost submission response observes the original request without replay', async ({ page }) => {
  await mount(page, 'lost')
  const d = await prepare(page)
  await d.getByLabel('我已了解预览影响、缓存重建及无法撤销的边界。').check()
  await d.getByRole('button', { name: '开始刷新', exact: true }).click()
  await page.evaluate(() => window.refreshFixture.finish())
  await expect(page.getByRole('dialog', { name: '维护结果' }).getByRole('status')).toHaveText(
    '所选命令已完成',
  )
  await expect(page.getByLabel('维护提交次数')).toHaveText('1')
})
test('compact light and dark dialogs scroll internally without horizontal overflow', async ({
  page,
}) => {
  await page.setViewportSize({ width: 680, height: 580 })
  await mount(page)
  const d = await prepare(page)
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    const bounds = await d.evaluate((el) => ({
      width: el.getBoundingClientRect().width,
      height: el.getBoundingClientRect().height,
      overflow: el.scrollWidth > el.clientWidth + 1,
    }))
    expect(bounds.width).toBeLessThanOrEqual(680)
    expect(bounds.height).toBeLessThanOrEqual(540)
    expect(bounds.overflow).toBe(false)
    await d.screenshot({ path: `/tmp/moomole-phase133-quicklook-${theme}.png` })
  }
})
test('startup recovers unresolved database history even without a session-storage handle', async ({
  page,
}) => {
  await mount(page, 'bootstrap')
  await expect(page.getByRole('region', { name: '维护状态' })).toContainText('需要核对')
  await page.getByRole('button', { name: '查看维护结果' }).click()
  await expect(page.getByRole('dialog', { name: '维护结果' })).toContainText('结果待核实')
  await expect(page.getByLabel('维护提交次数')).toHaveText('0')
})
