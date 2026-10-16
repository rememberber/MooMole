import { expect, test } from '@playwright/test'

test('permission findings explain limited checks and include restricted unknown and legacy items in the problem filter', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await expect(dialog.getByText('5 项需要重新确认', { exact: true })).toBeVisible()
  const clear = dialog.locator('article').filter({ hasText: '/fixture/clear.log' })
  await expect(clear).toContainText('不代表已经获准清理')
  await clear.getByText('观察项目与边界', { exact: true }).click()
  await expect(clear).toContainText('未检查目录后代、废纸篓目标、完整隐私授权或实际移动结果')
  await expect(clear.locator('.permission-finding')).toHaveCount(6)
  const restricted = dialog.locator('article').filter({ hasText: '/fixture/restricted.log' })
  await restricted.getByText('观察项目与边界', { exact: true }).click()
  await expect(restricted).toContainText('系统诊断代码：13')
  await expect(restricted).toContainText('不能仅据此区分普通权限与隐私授权')
  for (const name of ['legacy', 'future', 'incomplete']) {
    const item = dialog.locator('article').filter({ hasText: `/fixture/${name}.log` })
    await expect(item).toContainText('没有可识别的权限观察')
    await expect(item).not.toContainText('不代表已经获准清理')
  }
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.locator('article')).toHaveCount(5)
  await expect(clear).toHaveCount(0)
  await expect(
    dialog.getByRole('button', { name: /执行清理|移动到废纸篓|申请管理员权限/ }),
  ).toHaveCount(0)
  await page.screenshot({ path: 'test-results/permissions-review.png', fullPage: true })
})

test('local refresh replaces old permissions and a failed refresh keeps no positive evidence', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount('error'),
  )
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await expect(
    dialog.getByText('本机权限观察 · 未发现已检查范围中的限制', { exact: true }),
  ).toHaveCount(1)
  await dialog.getByRole('button', { name: '重新复核', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('fixture 权限复核暂不可用')
  await expect(
    dialog.getByText('本机权限观察 · 未发现已检查范围中的限制', { exact: true }),
  ).toHaveCount(0)
  await expect(dialog.getByText('本机权限观察 · 权限尚未确认', { exact: true })).toHaveCount(6)
})

test('successful local refresh updates permission observations for all selected items', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await dialog.getByRole('button', { name: '重新复核', exact: true }).click()
  await expect(
    dialog.getByText('本机权限观察 · 未发现已检查范围中的限制', { exact: true }),
  ).toHaveCount(6)
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.getByText('本次没有需要重新确认的项目。')).toBeVisible()
})

test('cancelled rule rechecks discard previous permission observations', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(
    dialog.getByText('本机权限观察 · 未发现已检查范围中的限制', { exact: true }),
  ).toHaveCount(0)
  await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
  await expect(dialog.getByRole('alert')).toContainText('fixture 规则复查已取消')
  await expect(dialog.getByText('本机权限观察 · 权限尚未确认', { exact: true })).toHaveCount(6)
})

test('expiry discards positive permission observations without another request', async ({
  page,
}) => {
  await page.clock.install()
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/permission-plan.tsx')).mount())
  const dialog = page.getByRole('dialog', { name: '审阅计划' })
  await page.clock.fastForward(901_000)
  await expect(dialog.getByText('此计划已失效，之前的复查结果不再适用。')).toBeVisible()
  await expect(
    dialog.getByText('本机权限观察 · 未发现已检查范围中的限制', { exact: true }),
  ).toHaveCount(0)
  await expect(dialog.getByText('本机权限观察 · 权限尚未确认', { exact: true })).toHaveCount(6)
})
