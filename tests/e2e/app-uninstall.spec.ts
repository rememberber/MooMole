import { test, expect } from '@playwright/test'
import { target } from '../fixtures/uninstall'
async function mount(page: import('@playwright/test').Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/app-uninstall.tsx')).mount(mode),
    mode,
  )
}
async function review(page: import('@playwright/test').Page) {
  await page
    .getByRole('article', { name: `${target.name} · ${target.path}` })
    .getByRole('button', { name: '卸载…' })
    .click()
  return page.getByRole('dialog', { name: '确认卸载' })
}
test('one App is checked without executing; acknowledgement permits exactly one submit', async ({
  page,
}) => {
  await mount(page)
  await expect(
    page
      .getByRole('article', { name: '受管理 App · /Applications/Managed.app' })
      .getByRole('button', { name: '卸载…' }),
  ).toBeDisabled()
  const dialog = await review(page)
  await expect(dialog.getByText('确认已过期，请重新检查；不会自动执行。')).toHaveCount(0)
  await expect(
    dialog.getByText('包外设置、缓存和个人数据全部保留。', { exact: false }),
  ).toBeVisible()
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  const action = dialog.getByRole('button', { name: '移入废纸篓', exact: true })
  await expect(action).toBeDisabled()
  await dialog.getByRole('checkbox').check()
  await action.dblclick()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await expect(result.getByRole('status')).toContainText('已移入废纸篓')
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
  await result.getByText('当时记录的移入位置').click()
  await expect(result).toContainText('/Users/demo/.Trash/示例 中文 App.app')
  await result.getByRole('button', { name: '完成', exact: true }).click()
  await expect(page.getByRole('region', { name: '卸载状态' })).toHaveCount(0)
  await page
    .getByRole('region', { name: '卸载记录' })
    .getByRole('button', { name: /示例中文 App/ })
    .click()
  await expect(page.getByRole('dialog', { name: '卸载结果' })).toContainText('当前没有应用内恢复')
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
})
test('expiry and malformed/unknown checks cannot submit', async ({ page }) => {
  await page.clock.install()
  await mount(page)
  const dialog = await review(page)
  await dialog.getByRole('checkbox').check()
  await page.clock.fastForward(31000)
  await expect(dialog.getByRole('button', { name: '移入废纸篓', exact: true })).toBeDisabled()
  await expect(dialog).toContainText('确认已过期')
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  for (const mode of ['invalid', 'unknown']) {
    await mount(page, mode)
    const next = await review(page)
    await expect(next.getByRole('button', { name: '移入废纸篓', exact: true })).toBeDisabled()
    await expect(next.getByRole(mode === 'invalid' ? 'alert' : 'status')).toBeVisible()
    await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  }
})
test('closing before submit withdraws the plan; a late plan cannot revive the dialog', async ({
  page,
}) => {
  await mount(page, 'slow')
  const dialog = await review(page)
  await dialog.getByRole('button', { name: '关闭弹窗' }).click()
  await page.evaluate(() => window.uninstallFixture.releasePrepare())
  await expect(page.getByLabel('计划撤下次数')).toHaveText('1')
  await expect(page.getByRole('dialog', { name: '确认卸载' })).toHaveCount(0)
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
})
test('closing an in-flight window does not withdraw its plan and cancel uses exact request ID', async ({
  page,
}) => {
  await mount(page, 'pending')
  const dialog = await review(page)
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await page
    .getByRole('dialog', { name: '卸载结果' })
    .getByRole('button', { name: '后台继续' })
    .click()
  await expect(page.getByLabel('计划撤下次数')).toHaveText('0')
  await page.getByRole('button', { name: '查看卸载结果' }).click()
  await page.getByRole('button', { name: '取消卸载', exact: true }).click()
  await expect(page.getByRole('dialog', { name: '卸载结果' }).getByRole('status')).toContainText(
    '已取消',
  )
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
  const calls = await page.evaluate(() => window.uninstallFixture.calls)
  const submit = calls.find((c) => c.command === 'execute')!.args as { requestId: string }
  const cancel = calls.find((c) => c.command === 'cancel')!.args as { requestId: string }
  expect(cancel.requestId).toBe(submit.requestId)
})
test('consecutive uninstall requests can each be cancelled once', async ({ page }) => {
  await mount(page, 'pending')
  for (let n = 0; n < 2; n++) {
    const d = await review(page)
    await d.getByRole('checkbox').check()
    await d.getByRole('button', { name: '移入废纸篓', exact: true }).click()
    const result = page.getByRole('dialog', { name: '卸载结果' })
    await result.getByRole('button', { name: '取消卸载', exact: true }).click()
    await expect(result.getByRole('status')).toContainText('已取消')
    await result.getByRole('button', { name: '完成', exact: true }).click()
  }
  const calls = await page.evaluate(() => window.uninstallFixture.calls)
  const executed = calls
    .filter((c) => c.command === 'execute')
    .map((c) => (c.args as { requestId: string }).requestId)
  const cancelled = calls
    .filter((c) => c.command === 'cancel')
    .map((c) => (c.args as { requestId: string }).requestId)
  expect(executed).toHaveLength(2)
  expect(new Set(executed).size).toBe(2)
  expect(cancelled).toEqual(executed)
})
test('current cancellation failure remains visible after its automatic result read', async ({
  page,
}) => {
  await page.clock.install()
  await mount(page, 'cancelError')
  const d = await review(page)
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  await expect(result.getByRole('alert')).toContainText('取消响应暂不可用')
  const beforeReads = (await page.evaluate(() => window.uninstallFixture.calls)).filter(
    (c) => c.command === 'read',
  ).length
  await page.clock.fastForward(1600)
  await expect
    .poll(
      async () =>
        (await page.evaluate(() => window.uninstallFixture.calls)).filter(
          (c) => c.command === 'read',
        ).length,
    )
    .toBeGreaterThan(beforeReads)
  await page.clock.runFor(50)
  await expect(result.getByRole('alert')).toContainText('取消响应暂不可用')
  await expect(result.getByRole('button', { name: '取消卸载', exact: true })).toBeEnabled()
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  await expect(result.getByRole('alert')).toContainText('取消响应暂不可用')
  const calls = await page.evaluate(() => window.uninstallFixture.calls)
  expect(calls.filter((c) => c.command === 'cancel')).toHaveLength(2)
  expect(
    (await page.evaluate(() => window.uninstallFixture.calls)).filter(
      (c) => c.command === 'execute',
    ),
  ).toHaveLength(1)
  await result.getByRole('button', { name: '后台继续' }).click()
  await expect(page.getByRole('region', { name: '卸载状态' })).toContainText('取消响应暂不可用')
  await page.getByRole('button', { name: '查看卸载结果' }).click()
  await page.evaluate(() => window.uninstallFixture.finish())
  await expect(result.getByRole('status')).toContainText('已移入废纸篓')
  expect(await result.getByRole('alert').count()).toBe(0)
})
test('an old cancellation error cannot release the next request cancellation guard', async ({
  page,
}) => {
  await mount(page, 'lateCancel')
  const first = await review(page)
  await first.getByRole('checkbox').check()
  await first.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  const original = await page.evaluate(
    () =>
      (
        window.uninstallFixture.calls.find((c) => c.command === 'execute')!.args as {
          requestId: string
        }
      ).requestId,
  )
  await page.evaluate(() => window.uninstallFixture.finish('cancelled'))
  await expect(result.getByRole('status')).toContainText('已取消')
  await result.getByRole('button', { name: '完成', exact: true }).click()
  const second = await review(page)
  await second.getByRole('checkbox').check()
  await second.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  await expect(
    result.getByRole('button', { name: '已请求取消，等待实际结果…', exact: true }),
  ).toBeDisabled()
  await page.evaluate((id) => window.uninstallFixture.rejectCancel(id), original)
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  )
  await expect(
    result.getByRole('button', { name: '已请求取消，等待实际结果…', exact: true }),
  ).toBeDisabled()
  const calls = await page.evaluate(() => window.uninstallFixture.calls)
  const executed = calls
    .filter((c) => c.command === 'execute')
    .map((c) => (c.args as { requestId: string }).requestId)
  expect(executed).toHaveLength(2)
  expect(new Set(executed).size).toBe(2)
  expect(
    calls
      .filter((c) => c.command === 'cancel')
      .map((c) => (c.args as { requestId: string }).requestId),
  ).toEqual(executed)
})
test('known terminal cancellation feedback stays retired while re-opening its history is still reading', async ({
  page,
}) => {
  await mount(page, 'lateCancel')
  const d = await review(page)
  await d.getByRole('checkbox').check()
  await d.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  const id = await page.evaluate(
    () =>
      (
        window.uninstallFixture.calls.find((c) => c.command === 'execute')!.args as {
          requestId: string
        }
      ).requestId,
  )
  await page.evaluate(() => window.uninstallFixture.finish('cancelled'))
  await expect(result.getByRole('status')).toContainText('已取消')
  await result.getByRole('button', { name: '完成', exact: true }).click()
  await page.evaluate(() => window.uninstallFixture.holdReads())
  await page
    .getByRole('region', { name: '卸载记录' })
    .getByRole('button', { name: /示例中文 App/ })
    .click()
  await page.evaluate((id) => window.uninstallFixture.rejectCancel(id), id)
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  )
  expect(await result.getByRole('alert').count()).toBe(0)
  expect(
    (await page.evaluate(() => window.uninstallFixture.calls)).filter(
      (c) => c.command === 'execute',
    ),
  ).toHaveLength(1)
})
test('lost response reads recorded facts; late cancel cannot erase a move or resubmit', async ({
  page,
}) => {
  await mount(page, 'lost')
  const dialog = await review(page)
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await expect(result.getByRole('alert')).toContainText('不要重复提交')
  await result.getByRole('button', { name: '取消卸载', exact: true }).click()
  await result.getByRole('button', { name: '重新读取结果' }).click()
  await expect(result.getByRole('status')).toContainText('已移入废纸篓')
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
})
test('window reload resumes by ID through reads only; a rejected admission does not stay busy', async ({
  page,
}) => {
  await mount(page, 'resume')
  await page.getByRole('button', { name: '查看卸载结果' }).click()
  const result = page.getByRole('dialog', { name: '卸载结果' })
  await expect(result.getByRole('button', { name: '取消卸载', exact: true })).toBeVisible()
  await expect(page.getByLabel('卸载提交次数')).toHaveText('0')
  await page.evaluate(() => window.uninstallFixture.finish('uncertain'))
  await result.getByRole('button', { name: '重新读取结果' }).click()
  await expect(result.getByRole('status')).toContainText('结果待核实')
  await expect(result).toContainText('不要按原路径重复执行')
  await mount(page, 'reject')
  const next = await review(page)
  await next.getByRole('checkbox').check()
  await next.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await expect(
    page
      .getByRole('dialog', { name: '卸载结果' })
      .getByRole('button', { name: '完成', exact: true }),
  ).toBeVisible()
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
})
test('minimum desktop light/dark layout keeps scope, acknowledgement and actions visible', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 800 })
  await mount(page)
  const dialog = await review(page)
  await expect(dialog.getByRole('checkbox')).toBeVisible()
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/app-uninstall-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('rebuilding a pending observer reads the same request without revoking or re-executing', async ({
  page,
}) => {
  await mount(page, 'pending')
  const dialog = await review(page)
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await page
    .getByRole('dialog', { name: '卸载结果' })
    .getByRole('button', { name: '后台继续' })
    .click()
  await page.getByRole('button', { name: '重建观察界面' }).click()
  await page.getByRole('button', { name: '查看卸载结果' }).click()
  await expect(page.getByRole('button', { name: '取消卸载', exact: true })).toBeVisible()
  await expect(page.getByLabel('卸载提交次数')).toHaveText('1')
  await expect(page.getByLabel('计划撤下次数')).toHaveText('0')
  await page.evaluate(() => window.uninstallFixture.finish())
  await page.getByRole('button', { name: '重新读取结果' }).click()
  await expect(page.getByRole('dialog', { name: '卸载结果' }).getByRole('status')).toContainText(
    '已移入废纸篓',
  )
})
declare global {
  interface Window {
    uninstallFixture: {
      calls: { command: string; args: unknown }[]
      finish: (state?: string) => void
      rejectCancel: (request: string) => void
      holdReads: () => void
      releasePrepare: () => void
    }
  }
}
