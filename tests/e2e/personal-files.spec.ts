import { expect, test, type Page } from '@playwright/test'
import { installPersonalMock } from './fixtures/personal-files-native'

async function open(page: Page) {
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '文件整理', exact: true })
    .click()
  return page.getByRole('main', { name: '文件整理内容' })
}
test('an interrupted move retains its planned destination and never calls it a Trash result', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  await expect(page.getByRole('region', { name: '个人文件列表' })).toBeVisible()
  await page.evaluate(() => {
    const report = JSON.parse(sessionStorage.getItem('personal-report')!)
    const items = report.items.slice(0, 2)
    sessionStorage.setItem('personal-selected', JSON.stringify(items))
    sessionStorage.setItem(
      'personal-movement',
      JSON.stringify({
        directory: '/Users/demo/Organized',
        conflict: 'keepBoth',
        targets: items.map((i: { id: string; name: string }) => ({
          id: i.id,
          destination: `/Users/demo/Organized/${i.name}`,
        })),
      }),
    )
    sessionStorage.setItem('personal-interrupted', 'yes')
    sessionStorage.setItem(
      'personal-state',
      JSON.stringify({
        phase: 'failed',
        requestId: '00000009-1234-4123-8123-123456789abc',
        runId: '00000007-1234-4123-8123-123456789abc',
        report: null,
        error: null,
      }),
    )
  })
  await page.reload()
  const notice = page.getByRole('region', { name: '文件处理中断与待核实结果' })
  await expect(notice).toContainText('有 1 项文件处理结果待核实')
  await expect(notice).toContainText('已记录移动 1 项、移入废纸篓 0 项')
  await notice.getByRole('button', { name: '前往执行记录' }).click()
  await page
    .getByRole('region', { name: '执行记录', exact: true })
    .getByRole('button')
    .filter({ hasText: '已中断' })
    .click()
  const dialog = page.getByRole('dialog', { name: '逐项执行结果' })
  await expect(
    dialog.getByRole('checkbox', {
      name: '我已核查源文件、确认目标与逐项结果，了解未知结果保持且不会自动重试。',
    }),
  ).toBeVisible()
  await expect(dialog).toContainText('核对源文件、确认目标与逐项结果')
  await expect(dialog.getByText('确认移入废纸篓', { exact: true })).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.filter((c: { command: string }) => c.command.startsWith('execute_personal_')),
  ).toHaveLength(0)
})
test('moving requires explicit destinations and acknowledgement, keeps both names and tracks cancellation in history', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  const main = await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  await main
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip', exact: true })
    .check()
  await main
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/recent.pdf', exact: true })
    .check()
  const move = main.getByRole('button', { name: '移动到目录…', exact: true })
  await expect(move).toBeDisabled()
  await main.getByRole('textbox', { name: '个人文件移动目标' }).fill('/Users/demo/Organized')
  await main.getByRole('combobox', { name: '移动重名处理' }).selectOption('keepBoth')
  await move.click()
  const dialog = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(dialog.getByText('/Users/demo/Organized/old (1).zip', { exact: true })).toBeVisible()
  await expect(dialog.getByText('/Users/demo/Organized/recent.pdf', { exact: true })).toBeVisible()
  const confirm = dialog.getByRole('button', { name: '确认移动文件', exact: true })
  await expect(confirm).toBeDisabled()
  await expect(dialog.getByText(/仅将下面列出的普通文件移入废纸篓/)).toHaveCount(0)
  await dialog.getByRole('checkbox').check()
  await confirm.click()
  const result = main.getByRole('region', { name: '个人文件处理结果' })
  await expect(result.getByText('已移动到目标目录', { exact: true })).toBeVisible()
  await expect(result.getByRole('region', { name: '文件移动大小' })).toBeVisible()
  await expect(result.getByText('确认移入废纸篓', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '停止后续项目', exact: true }).click()
  await expect(result.getByText('开始前取消', { exact: true })).toBeVisible()
  await main.getByRole('button', { name: '查看处理历史' }).click()
  await page.getByRole('button', { name: /已取消 · 2 项/ }).click()
  const history = page.getByRole('dialog', { name: '逐项执行结果' })
  await history.locator('.execution-item summary').first().click()
  await expect(
    history.getByText('/Users/demo/Organized/old (1).zip', { exact: true }),
  ).toBeVisible()
  await expect(history.getByText('已移入废纸篓', { exact: true })).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_personal_move'),
  ).toHaveLength(1)
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_personal_trash'),
  ).toHaveLength(0)
})
test('skip conflicts are visible before execution and leaving confirmation does not move files', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  const main = await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  await main
    .getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip', exact: true })
    .check()
  await main.getByRole('textbox', { name: '个人文件移动目标' }).fill('/Users/demo/Organized')
  await main.getByRole('button', { name: '移动到目录…', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(dialog.getByText('跳过：目标重名或已在目标目录', { exact: true })).toBeVisible()
  await dialog.getByRole('button', { name: '返回选择' }).click()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.filter((c: { command: string }) => c.command.startsWith('execute_personal_')),
  ).toHaveLength(0)
})
test('language changes preserve selected personal-file IDs and raw paths without executing', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  const main = await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  const path = '/Users/demo/Downloads/old.zip'
  await main.getByRole('checkbox', { name: `选择 ${path}` }).check()
  for (const [language, typeLabel, selectLabel] of [
    ['en', 'Personal file type', `Select ${path}`],
    ['ja', '個人ファイルの種類', `${path} を選択`],
  ]) {
    await page.evaluate(
      async (language) => (await import('/src/i18n/locale.ts')).setLanguagePreference(language),
      language,
    )
    await expect(page.getByRole('combobox', { name: typeLabel })).toBeVisible()
    await expect(page.getByRole('checkbox', { name: selectLabel, exact: true })).toBeChecked()
    await expect(page.getByText('~/Downloads/old.zip', { exact: true })).toBeVisible()
  }
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.filter(
      (c: { command: string }) =>
        c.command === 'prepare_personal_trash' || c.command === 'execute_personal_trash',
    ),
  ).toHaveLength(0)
})
test('personal selection combines filters, preserves exact IDs and requires an explicit final acknowledgement', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  const main = await open(page)
  await expect(page.getByRole('button', { name: '扫描文件', exact: true })).toBeEnabled()
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  expect(
    await page.evaluate(() =>
      JSON.parse(sessionStorage.getItem('personal-calls') || '[]').filter(
        (c: { command: string }) => c.command === 'start_personal_scan',
      ),
    ),
  ).toHaveLength(1)
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.locator('article')).toHaveCount(4)
  await expect(
    list.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/protected.mov' }),
  ).toBeDisabled()
  await expect(main.getByRole('button', { name: '移入废纸篓…', exact: true })).toBeDisabled()
  await main.getByRole('combobox', { name: '个人文件类型' }).selectOption('archive')
  await main
    .getByRole('combobox', { name: '个人文件最小大小' })
    .selectOption(String(100 * 1024 ** 2))
  await main.getByRole('combobox', { name: '个人文件修改时间' }).selectOption('90')
  await expect(list.locator('article')).toHaveCount(1)
  await list.getByRole('checkbox').check()
  await main.getByRole('combobox', { name: '个人文件类型' }).selectOption('all')
  await main.getByRole('combobox', { name: '个人文件最小大小' }).selectOption('0')
  await main.getByRole('combobox', { name: '个人文件修改时间' }).selectOption('0')
  await list.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/notes.txt' }).check()
  await main.getByRole('searchbox', { name: '搜索个人文件' }).fill('nothing matches')
  await expect(main.getByText('已选 2 项', { exact: false })).toBeVisible()
  await main.getByRole('button', { name: '移入废纸篓…', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '确认处理个人文件' })
  await expect(dialog.locator('article')).toHaveCount(2)
  await expect(dialog).toContainText('不能保证文件空闲')
  const execute = dialog.getByRole('button', { name: '确认移入废纸篓', exact: true })
  await expect(execute).toBeDisabled()
  expect(await page.evaluate(() => sessionStorage.getItem('personal-executed'))).toBeNull()
  await dialog.getByRole('checkbox').check()
  await execute.click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toContainText(
    '已移入 1 / 2 项',
  )
  await page.getByRole('button', { name: '停止后续项目', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toContainText('开始前取消')
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.find((c: { command: string }) => c.command === 'prepare_personal_trash').args.selectedIds,
  ).toEqual(['00000002-1234-4123-8123-123456789abc', '00000004-1234-4123-8123-123456789abc'])
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_personal_trash'),
  ).toHaveLength(1)
  await main.getByRole('button', { name: '查看处理历史' }).click()
  const history = page.getByRole('region', { name: '执行记录', exact: true })
  await history.getByRole('button').filter({ hasText: '已取消' }).click()
  await expect(page.getByRole('dialog', { name: '逐项执行结果' })).toContainText(
    '来源：个人文件整理',
  )
})
test('directory changes withdraw selection and the minimum window keeps settings reachable', async ({
  page,
}) => {
  await installPersonalMock(page)
  await page.goto('/')
  const main = await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  await main.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip' }).check()
  await page.setViewportSize({ width: 960, height: 680 })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await main.evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true)
    const settings = await page.getByRole('button', { name: '设置', exact: true }).boundingBox()
    expect(settings!.y + settings!.height).toBeLessThanOrEqual(680)
    await page.screenshot({
      path: `test-results/personal-files-${theme}.png`,
      animations: 'disabled',
    })
  }
  await main.getByRole('textbox', { name: '个人文件目录' }).fill('/Users/demo/Documents')
  await expect(main.getByRole('button', { name: '移入废纸篓…', exact: true })).toBeDisabled()
  await main.getByRole('textbox', { name: '个人文件目录' }).fill('/Users/demo/Downloads')
  await expect(
    main.getByRole('checkbox', { name: '选择 /Users/demo/Downloads/old.zip' }),
  ).not.toBeChecked()
})
test('reload reconnects a pending scan and cancellation keeps partial results without a second scan', async ({
  page,
}) => {
  await installPersonalMock(page, true)
  await page.goto('/')
  await open(page)
  await page.getByRole('button', { name: '扫描文件', exact: true }).click()
  await expect(page.getByRole('button', { name: '停止扫描', exact: true })).toBeVisible()
  await page.reload()
  const main = await open(page)
  await page.getByRole('button', { name: '停止扫描', exact: true }).click()
  await expect(main.getByRole('region', { name: '个人文件列表' }).locator('article')).toHaveCount(4)
  await expect(main).toContainText('扫描已停止。')
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('personal-calls')!))
  expect(
    calls.filter((c: { command: string }) => c.command === 'start_personal_scan'),
  ).toHaveLength(1)
  expect(
    calls.filter((c: { command: string }) => c.command === 'execute_personal_trash'),
  ).toHaveLength(0)
})
