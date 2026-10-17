import { expect, test, type Locator } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

async function expandPlanDetails(dialog: Locator) {
  for (const detail of await dialog.locator('.plan-item-details').all())
    if ((await detail.getAttribute('open')) === null)
      await detail.locator('summary').first().click()
}

test('running tasks reconnect after a WebView reload without starting a duplicate', async ({
  page,
}) => {
  await installTaskMock(page, 'resume')
  await page.goto('/')
  await expect(page.getByText('空间分析进行中', { exact: true })).toBeVisible()
  await page.reload()
  await expect(page.getByText('空间分析进行中', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.locator('.task-history .state-tag.running')).toBeVisible()
  await expect(page.getByRole('button', { name: '继续跟踪' })).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('mock-starts'))).toBeNull()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.locator('.task-history .state-tag.succeeded')).toBeVisible()
  await expect(page.getByText('空间分析进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByRole('textbox', { name: '扫描目录' })).toHaveValue('/Users/demo/Projects')
  expect(await page.evaluate(() => sessionStorage.getItem('mock-starts'))).toBeNull()
})

test('accepted tasks appear immediately in history and complete even without native events', async ({
  page,
}) => {
  await installTaskMock(page, 'new')
  await page.goto('/')
  await expect(page.getByText('系统快照进行中', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: '操作历史', exact: true }).click()
  await expect(page.locator('.task-history .state-tag.running')).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.locator('.task-history .state-tag.succeeded')).toBeVisible()
  const starts = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]'),
  )
  expect(starts).toHaveLength(1)
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await expect(page.getByText('示例 Mac', { exact: true })).toBeVisible()
})

test('browser preview labels its data and cannot execute real tasks', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto('/')
  await expect(page.getByText('浏览器预览', { exact: true })).toBeVisible()
  await expect(page.getByRole('main', { name: '概览内容' })).toBeVisible()
  const engine = page.getByRole('region', { name: 'Mole CLI 版本' })
  await expect(engine).toBeVisible()
  await expect(engine).toContainText('内置 Mole CLI')
  await expect(engine).toContainText('版本待确认')
  await expect(engine).toContainText('未加载本机引擎')
  await expect(page.getByRole('button', { name: '刷新状态' })).toBeDisabled()
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await expect(page.getByText('部分目录未完整测量', { exact: false })).toBeVisible()
  await page.getByRole('textbox', { name: '筛选目录内容' }).fill('Projects')
  await expect(page.getByRole('button', { name: 'Projects 文件夹' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Library 文件夹' })).toHaveCount(0)
  await page.getByRole('button', { name: '清理', exact: true }).click()
  await expect(page.getByText('当前只提供扫描与审阅', { exact: false })).toBeVisible()
  await expect(page.getByRole('button', { name: '重新扫描', exact: true })).toBeDisabled()
  expect(errors).toEqual([])
})

test('home engine summary shows verified version and handles unavailable engines', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => {
    const fixture = await import('/tests/e2e/fixtures/engine-summary.tsx')
    fixture.mount()
  })
  const fixture = page.locator('#engine-test-fixture')
  const engine = fixture.getByRole('region', { name: 'Mole CLI 版本' })
  await expect(engine).toContainText('v1.59.1')
  await expect(engine).toContainText('ba3a0f57f78e')
  await expect(engine).toContainText('引擎就绪')
  await fixture.getByRole('button', { name: '模拟引擎缺失' }).click()
  await expect(engine).toContainText('版本待确认')
  await expect(engine).toContainText('引擎未就绪')
  await expect(engine).not.toContainText('v1.59.1')
})

test('structured preview supports selection, subset totals, filters and diagnostics', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '清理', exact: true }).click()
  const review = page.getByRole('region', { name: 'Mole 清理项目审阅' })
  await expect(review.getByText('3 个候选路径', { exact: false })).toBeVisible()
  const parent = review.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp',
    exact: true,
  })
  const child = review.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
    exact: true,
  })
  await child.check()
  await expect(review.getByRole('status')).toHaveText('已勾选 1 项 · 512 MB')
  await parent.check()
  await expect(review.getByRole('status')).toHaveText('已勾选 2 项 · 1.5 GB')
  await review.getByRole('combobox', { name: '清理项目状态' }).selectOption('unknown')
  await expect(parent).toHaveCount(0)
  await review
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Logs/ExampleApp', exact: true })
    .check()
  await expect(review.getByRole('status')).toHaveText('已勾选 3 项 · 1.5 GB，另有 1 项大小未知')
  await review.getByRole('combobox', { name: '清理项目状态' }).selectOption('all')
  await review.getByRole('textbox', { name: '搜索清理项目' }).fill('Images')
  await expect(child).toBeVisible()
  await expect(parent).toHaveCount(0)
  await review.locator('.review-detail > summary').click()
  await expect(review.getByText('完整预览中，大小已计入父目录', { exact: false })).toBeVisible()
  await expect(review.getByRole('button', { name: '保护此目录' })).toBeDisabled()
  await review.getByText('查看 Mole 原始日志', { exact: true }).click()
  await expect(review.getByLabel('Mole 清理预览日志')).toContainText('没有扫描本机文件')
  await page.screenshot({ path: 'test-results/clean-review.png', fullPage: true })
})

test('settings modal supports theme changes, keyboard dismissal and about copy', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '设置', exact: false }).click()
  const dialog = page.getByRole('dialog', { name: '设置', exact: true })
  await expect(dialog).toBeVisible()
  await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('dark')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await dialog.getByRole('button', { name: '关于', exact: true }).click()
  await expect(dialog.getByText('本项目仅供学习和研究使用', { exact: false })).toBeVisible()
  await expect(dialog.getByRole('button', { name: '访问 Mole 官方产品' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
})

test('protecting a selected child immediately excludes it and its parent from review', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => {
    // Test fixture is served by the development server and never bundled in the app.
    const fixture = await import('/tests/e2e/fixtures/clean-review.tsx')
    fixture.mount()
  })
  const fixture = page.locator('#review-test-fixture')
  const parent = fixture.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp',
    exact: true,
  })
  const child = fixture.getByRole('checkbox', {
    name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
    exact: true,
  })
  await parent.check()
  await child.check()
  await fixture.getByText('Images', { exact: true }).click()
  await fixture.getByRole('button', { name: '保护此目录' }).click()
  await expect(parent).toBeDisabled()
  await expect(child).toBeDisabled()
  await expect(fixture.getByRole('status')).toHaveText('已勾选 0 项 · 0 B')
  await fixture.getByRole('combobox', { name: '清理项目状态' }).selectOption('protected')
  await expect(fixture.locator('.review-item')).toHaveCount(2)
  await expect(fixture.getByRole('checkbox', { name: '勾选本页可审阅项目' })).toBeDisabled()
})

test('all primary pages render without horizontal overflow', async ({ page }) => {
  await page.goto('/')
  for (const name of ['空间分析', '清理', '应用列表', '系统状态', '操作历史', '概览']) {
    await page
      .getByRole('navigation', { name: '主导航' })
      .getByRole('button', { name, exact: true })
      .click()
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    ).toBe(true)
  }
})

test('review plan dialog saves selected scope and explains changed identities on recheck', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => {
    const fixture = await import('/tests/e2e/fixtures/clean-review.tsx')
    fixture.mount()
  })
  const fixture = page.locator('#review-test-fixture')
  await expect(fixture.getByRole('button', { name: '保存审阅计划' })).toBeDisabled()
  await fixture
    .getByRole('checkbox', {
      name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
      exact: true,
    })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await expect(dialog.getByText('1 个项目', { exact: true })).toBeVisible()
  await expect(
    dialog.getByText('/Users/demo/Library/Caches/ExampleApp/Images', { exact: true }),
  ).toBeVisible()
  await expect(dialog.getByText('当前未开放执行', { exact: false })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.getByText('本次没有需要重新确认的项目。')).toBeVisible()
  await dialog.getByRole('button', { name: '重新复核' }).click()
  await expect(dialog.getByText('目标已变化', { exact: true })).toBeVisible()
  await expect(dialog.getByText('文件身份已变化，需要重新扫描。')).toBeVisible()
  await page.screenshot({ path: 'test-results/review-plan.png', fullPage: true })
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
})

test('Mole rule evidence is visible and cancelling a new probe clears previous eligibility', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/clean-review.tsx')).mount())
  const fixture = page.locator('#review-test-fixture')
  await fixture
    .getByRole('checkbox', {
      name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
      exact: true,
    })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByText('Mole 规则保护', { exact: true })).toBeVisible()
  await expandPlanDetails(dialog)
  await expect(dialog.getByText('Mole 有效白名单：路径与当前有效规则匹配，应保留。')).toBeVisible()
  await page.screenshot({ path: 'test-results/mole-rules.png', fullPage: true })
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('cancel'),
  )
  await fixture
    .getByRole('checkbox', {
      name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
      exact: true,
    })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('规则复查已取消。')
  await expect(dialog.getByText('规则复查未完成', { exact: true })).toBeVisible()
  await expect(dialog.getByText('本次文件信息未见变化', { exact: true })).toHaveCount(0)
})

test('purpose filters explain irreversible log loss and preserve unknown legacy impacts', async ({
  page,
}) => {
  await page.goto('/')
  await page.getByRole('button', { name: '清理', exact: true }).click()
  const review = page.getByRole('region', { name: 'Mole 清理项目审阅' })
  await review.getByRole('combobox', { name: '清理项目用途' }).selectOption('logs')
  await expect(review.locator('.review-item')).toHaveCount(1)
  await review.locator('.review-detail > summary').click()
  await expect(
    review.getByText('历史日志通常不能重新生成；未来日志不能补回过去的记录。'),
  ).toBeVisible()
  await review.getByRole('combobox', { name: '清理项目用途' }).selectOption('appCache')
  await expect(review.locator('.review-item')).toHaveCount(2)
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('legacy'),
  )
  const fixture = page.locator('#review-test-fixture')
  await fixture.getByRole('combobox', { name: '清理项目用途' }).selectOption('unknown')
  await expect(fixture.locator('.review-item')).toHaveCount(3)
  await fixture.locator('.review-detail > summary').first().click()
  await expect(
    fixture.locator('.review-item').first().getByText('用途尚未确认', { exact: true }),
  ).toBeVisible()
})

test('open plans expire automatically and disable rule probes without another user action', async ({
  page,
}) => {
  await page.clock.install({ time: new Date('2026-10-15T12:00:00Z') })
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/clean-review.tsx')).mount())
  const fixture = page.locator('#review-test-fixture')
  await fixture
    .getByRole('checkbox', {
      name: '审阅 /Users/demo/Library/Caches/ExampleApp/Images',
      exact: true,
    })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await expect(dialog.getByRole('button', { name: '复查 Mole 规则', exact: true })).toBeEnabled()
  await expandPlanDetails(dialog)
  await expect(dialog.getByText('已记录目录内 3 个子项的元数据', { exact: false })).toBeVisible()
  await page.clock.fastForward(16 * 60 * 1000)
  await expect(dialog.getByText('计划已过期', { exact: true })).toBeVisible()
  await expect(dialog.getByText('此计划已失效，之前的复查结果不再适用。')).toBeVisible()
  await expect(dialog.getByRole('button', { name: '复查 Mole 规则', exact: true })).toBeDisabled()
  await expect(fixture.getByRole('button', { name: '保存审阅计划' })).toBeDisabled()
  await expect(dialog.getByLabel('用途与影响说明')).toBeVisible()
  await page.screenshot({ path: 'test-results/impact-plan.png', fullPage: true })
})

test('execution history distinguishes uncertain results, receipts and cancelled pending items', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/execution-history.tsx')).mount(),
  )
  const fixture = page.locator('#execution-test-fixture')
  await fixture.getByRole('button', { name: '已中断 · 64 项', exact: false }).click()
  const dialog = page.getByRole('dialog', { name: '逐项执行结果' })
  await expect(
    dialog.getByText('存在只有意图、没有可靠完成记录的项目。', { exact: false }),
  ).toBeVisible()
  await expect(dialog.getByRole('status')).toHaveText('显示 50 / 64 项')
  await dialog.getByRole('button', { name: '再显示 50 项' }).click()
  await expect(dialog.getByRole('status')).toHaveText('显示 64 / 64 项')
  await dialog.getByRole('combobox', { name: '执行项目状态' }).selectOption('issues')
  await expect(dialog.getByRole('status')).toHaveText('显示 50 / 63 项')
  await dialog.getByRole('textbox', { name: '搜索执行路径' }).fill('cache-63')
  await expect(dialog.getByRole('status')).toHaveText('显示 1 / 1 项')
  await expect(dialog.getByText('中断后未开始', { exact: true })).toBeVisible()
  await dialog.getByRole('textbox', { name: '搜索执行路径' }).fill('中文')
  await expect(dialog.getByText('没有匹配的项目。')).toBeVisible()
  await dialog.getByRole('combobox', { name: '执行项目状态' }).selectOption('moved')
  await dialog.locator('.execution-item > summary').click()
  await expect(dialog.getByText('/fixture/recovery/中文 cache *', { exact: true })).toBeVisible()
  await expect(
    dialog.getByText('设备 18446744073709551615 · inode 9007199254740993', { exact: true }),
  ).toBeVisible()
  await expect(
    dialog.getByRole('list', { name: '项目记录顺序' }).getByRole('listitem'),
  ).toHaveCount(2)
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'))
  await page.screenshot({ path: 'test-results/execution-history-dark.png', fullPage: true })
  await page.keyboard.press('Escape')
  await fixture.getByRole('button', { name: '已取消 · 2 项', exact: false }).click()
  await expect(
    dialog.getByText('取消与中断不会撤销已经完成的项目。', { exact: false }),
  ).toBeVisible()
  await expect(dialog.getByText('已移入废纸篓', { exact: true })).toBeVisible()
  await expect(
    dialog
      .locator('.execution-item')
      .filter({ hasText: '/fixture/cache-1' })
      .getByText('已取消', { exact: true }),
  ).toBeVisible()
  await expect(dialog.getByRole('button', { name: /重试操作|恢复|执行清理/ })).toHaveCount(0)
})

test('execution record read failures keep history and closing a pending read stays closed', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/execution-history.tsx')).mount(),
  )
  const fixture = page.locator('#execution-test-fixture')
  await fixture.getByRole('button', { name: '模拟列表读取失败' }).click()
  await fixture.getByRole('button', { name: '刷新记录' }).click()
  await expect(fixture.getByRole('alert')).toHaveText('fixture 列表暂时不可读取')
  await expect(fixture.getByRole('button', { name: '已中断 · 64 项', exact: false })).toBeVisible()
  await fixture.getByRole('button', { name: '模拟详情读取失败' }).click()
  await fixture.getByRole('button', { name: '已中断 · 64 项', exact: false }).click()
  const dialog = page.getByRole('dialog', { name: '逐项执行结果' })
  await expect(dialog.getByRole('alert')).toContainText('fixture 详情暂时不可读取')
  await dialog.getByRole('button', { name: '重试读取' }).click()
  await expect(dialog.getByRole('textbox', { name: '搜索执行路径' })).toBeVisible()
  await page.keyboard.press('Escape')
  await fixture.getByRole('button', { name: '已中断 · 64 项', exact: false }).click()
  await expect(dialog.getByText('正在读取本地记录…')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toHaveCount(0)
  await fixture.getByRole('button', { name: '已取消 · 2 项', exact: false }).click()
  await expect(
    dialog.getByText('已取消 · 已移入 1 / 2 项 · 待核实 0 项', { exact: true }),
  ).toBeVisible()
})

test('module checks show partial coverage and local recheck failure clears all old evidence', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('modules'),
  )
  const fixture = page.locator('#review-test-fixture')
  const checks = fixture.getByRole('checkbox', { name: /^审阅 / })
  for (let i = 0; i < (await checks.count()); i++) await checks.nth(i).check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByText('模块规则尚未适配', { exact: true })).toBeVisible()
  await expect(dialog.getByText('模块要求保留或重新确认', { exact: true })).toBeVisible()
  await expandPlanDetails(dialog)
  await expect(
    dialog.getByText('仅 Deno 约束已复查，其他模块仍需单独核对。', { exact: true }),
  ).toBeVisible()
  await expect(dialog.getByText('2 项需要重新确认', { exact: true })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(
    dialog.getByText('仅 Deno 约束已复查，其他模块仍需单独核对。', { exact: true }),
  ).toHaveCount(0)
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).uncheck()
  await expandPlanDetails(dialog)
  await dialog.getByText('检查来源与版本', { exact: true }).first().click()
  await expect(
    dialog.getByText('Mole _user_cache_deno_delete_guard · 规则 v2', { exact: true }),
  ).toBeVisible()
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'))
  await page.screenshot({
    path: 'test-results/module-rules-dark.png',
    fullPage: true,
    animations: 'disabled',
  })
  await dialog.getByRole('button', { name: '重新复核', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('fixture 本地复核读取失败')
  await expect(dialog.getByLabel('模块规则复查')).toHaveCount(0)
  await expect(dialog.getByText('无法确认身份', { exact: true })).toHaveCount(3)
})

test('cancelling a repeated module check clears previous module observations', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('module-cancel'),
  )
  const fixture = page.locator('#review-test-fixture')
  await fixture
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Caches/ExampleApp', exact: true })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('模块规则复查')).toHaveCount(1)
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('模块规则复查')).toHaveCount(0)
  await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('规则复查已取消。')
  await expect(dialog.getByText('规则复查未完成', { exact: true })).toBeVisible()
  await expect(dialog.getByLabel('模块规则复查')).toHaveCount(0)
})

test('recorded callers survive plan creation and mixed sources never imply full module coverage', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('origins'),
  )
  const fixture = page.locator('#review-test-fixture')
  await fixture
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Caches/ExampleApp', exact: true })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await expandPlanDetails(dialog)
  await dialog.getByText('扫描调用来源 · 2 种', { exact: true }).click()
  await expect(
    dialog.getByText(
      'record_dry_run_cleanup_target → _safe_clean_impl → safe_clean → fixture_other_module',
      { exact: true },
    ),
  ).toBeVisible()
  await expect(
    dialog.getByText(
      'record_dry_run_cleanup_target → _safe_clean_impl → safe_clean_guarded → clean_user_essentials',
      { exact: true },
    ),
  ).toBeVisible()
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByText('模块规则尚未适配', { exact: true })).toBeVisible()
  await expect(
    dialog.getByText('同一对象存在混合模块来源，不能只按路径选择检查。', { exact: true }),
  ).toBeVisible()
  await page.screenshot({
    path: 'test-results/origin-binding.png',
    fullPage: true,
    animations: 'disabled',
  })
})

test('old scans stay visible but require rescan before saving a new plan', async ({ page }) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('old-scan'),
  )
  const fixture = page.locator('#review-test-fixture')
  await expect(
    fixture.getByText('此旧扫描没有绑定调用来源，请重新扫描后保存新计划。'),
  ).toBeVisible()
  await fixture
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Caches/ExampleApp', exact: true })
    .check()
  await expect(fixture.getByRole('button', { name: '保存审阅计划' })).toBeDisabled()
})

test('additional activity observations distinguish busy and unknown and clear on cancellation', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('activity'),
  )
  const fixture = page.locator('#review-test-fixture')
  const checks = fixture.getByRole('checkbox', { name: /^审阅 / })
  for (let i = 0; i < (await checks.count()); i++) await checks.nth(i).check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('附加占用检查')).toHaveCount(3)
  await expect(dialog.getByText('发现文件占用', { exact: true })).toBeVisible()
  await expect(dialog.getByText('文件占用尚未确认', { exact: true })).toBeVisible()
  await expandPlanDetails(dialog)
  await expect(
    dialog.getByText('本次未发现打开句柄；不是持续空闲保证，也不是执行许可。'),
  ).toBeVisible()
  await expect(dialog.getByText('2 项需要重新确认', { exact: true })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.getByLabel('附加占用检查')).toHaveCount(2)
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).uncheck()
  await expandPlanDetails(dialog)
  await dialog.getByText('观察来源与版本', { exact: true }).first().click()
  await expect(
    dialog
      .getByText('客户端附加观察 · Mole _mole_container_cache_has_open_handle · 观察 v1', {
        exact: true,
      })
      .first(),
  ).toBeVisible()
  await page.screenshot({
    path: 'test-results/activity-checks.png',
    fullPage: true,
    animations: 'disabled',
  })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('附加占用检查')).toHaveCount(0)
  await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('规则复查已取消。')
  await expect(dialog.getByLabel('附加占用检查')).toHaveCount(0)
  await expect(dialog.getByText('规则复查未完成', { exact: true })).toHaveCount(3)
})

test('activity budget exhaustion and missing legacy evidence never imply idle', async ({
  page,
}) => {
  for (const mode of ['activity-budget', 'modules']) {
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/clean-review.tsx')).mount(mode),
      mode,
    )
    const fixture = page.locator('#review-test-fixture')
    const checks = fixture.getByRole('checkbox', { name: /^审阅 / })
    for (let i = 0; i < (await checks.count()); i++) await checks.nth(i).check()
    await fixture.getByRole('button', { name: '保存审阅计划' }).click()
    const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
    await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
    await expandPlanDetails(dialog)
    if (mode === 'activity-budget')
      await expect(
        dialog.getByText('本批附加占用检查的 5 秒预算已用完，此项尚未确认；请缩小选择后复查。'),
      ).toBeVisible()
    else {
      await expect(
        dialog.getByText('这份结果缺少可识别的占用证据，请重新复查；不能视为空闲。'),
      ).toHaveCount(3)
      await dialog.getByRole('button', { name: '重新复核', exact: true }).click()
      await expect(dialog.getByLabel('附加占用检查')).toHaveCount(0)
    }
    await expect(dialog.getByRole('button', { name: /执行清理|关闭应用|提权/ })).toHaveCount(0)
  }
})

test('descendant path protection explains blocked children, bounded scope and clears on cancellation', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('descendants'),
  )
  const fixture = page.locator('#review-test-fixture')
  const checks = fixture.getByRole('checkbox', { name: /^审阅 / })
  for (let i = 0; i < (await checks.count()); i++) await checks.nth(i).check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(3)
  await expect(dialog.getByText('目录后代受保护', { exact: true })).toBeVisible()
  await expect(dialog.getByText('目录后代保护尚未确认', { exact: true })).toBeVisible()
  await expandPlanDetails(dialog)
  await expect(
    dialog.getByText('/Users/demo/Library/Caches/ExampleApp/deep/保留 中文\n[*].log', {
      exact: true,
    }),
  ).toBeVisible()
  await expect(
    dialog.getByText('后代路径超过本批规则检查的剩余额度；本次未完成后代保护检查。'),
  ).toBeVisible()
  await expect(dialog.getByText('2 项需要重新确认', { exact: true })).toBeVisible()
  await dialog.getByText('检查范围与来源', { exact: true }).first().click()
  await expect(dialog.getByText('已核对 2 项后代路径结果 · 观察 v1')).toBeVisible()
  await expect(dialog.getByText('Mole 有效白名单', { exact: true })).toBeVisible()
  await dialog.getByRole('checkbox', { name: '只看需要确认的项目' }).check()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(2)
  await page.screenshot({
    path: 'test-results/descendant-rules.png',
    fullPage: true,
    animations: 'disabled',
  })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(0)
  await dialog.getByRole('button', { name: '取消复查', exact: true }).click()
  await expect(dialog.getByRole('alert')).toHaveText('规则复查已取消。')
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(0)
})

test('local refresh and expiry discard descendant evidence', async ({ page }) => {
  await page.clock.install({ time: new Date('2026-10-16T12:00:00Z') })
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('descendants'),
  )
  const fixture = page.locator('#review-test-fixture')
  await fixture
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Caches/ExampleApp', exact: true })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  const dialog = page.getByRole('dialog', { name: '审阅计划', exact: true })
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(1)
  await dialog.getByRole('button', { name: '重新复核', exact: true }).click()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(0)
  await expect(dialog.getByText('目标已变化', { exact: true })).toBeVisible()
  // A fresh fixture starts a separate plan; old evidence must also vanish when
  // the still-open dialog reaches its expiry, without a further user action.
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/clean-review.tsx')).mount('descendants'),
  )
  await fixture
    .getByRole('checkbox', { name: '审阅 /Users/demo/Library/Caches/ExampleApp', exact: true })
    .check()
  await fixture.getByRole('button', { name: '保存审阅计划' }).click()
  await dialog.getByRole('button', { name: '复查 Mole 规则', exact: true }).click()
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(1)
  await page.clock.fastForward(16 * 60 * 1000)
  await expect(dialog.getByLabel('目录后代路径保护')).toHaveCount(0)
  await expect(dialog.getByText('计划已过期', { exact: true })).toBeVisible()
})
