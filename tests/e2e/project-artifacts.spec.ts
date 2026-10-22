import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { projectArtifactsReport, projectMeasurement } from './fixtures/project-artifacts'
async function install(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(
    ({ fixture, measurement }) => {
      const native = (
        window as unknown as {
          __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
        }
      ).__TAURI_INTERNALS__
      const original = native.invoke
      native.invoke = async (command, args) => {
        if (command === 'plugin:dialog|open')
          return sessionStorage.getItem('project-picker-cancel') ? null : '/fixture/projects'
        if (command === 'read_project_roots') {
          sessionStorage.setItem(
            'roots-calls',
            String(Number(sessionStorage.getItem('roots-calls') ?? 0) + 1),
          )
          sessionStorage.setItem('roots-args', JSON.stringify(args ?? null))
          if (sessionStorage.getItem('roots-fail')) throw new Error('fixture 配置读取受限')
          const result = {
            protocol: 1,
            revision: fixture.revision,
            source: '/Users/demo/.config/mole/purge_paths',
            checkedAt: Date.now(),
            state: 'ready',
            paths: ['/fixture/projects', '/fixture/second/' + 'long-name'.repeat(60)],
            canExecute: false,
          }
          if (sessionStorage.getItem('roots-missing')) {
            result.state = 'missing'
            result.paths = []
          }
          if (sessionStorage.getItem('roots-empty')) result.paths = []
          if (sessionStorage.getItem('roots-malformed'))
            result.source = '/wrong/account/purge_paths'
          if (sessionStorage.getItem('roots-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishRoots: () => resolve(result) }),
            )
          return result
        }
        if (command === 'measure_project_artifact') {
          sessionStorage.setItem('measure-args', JSON.stringify(args))
          sessionStorage.setItem(
            'measure-calls',
            String(Number(sessionStorage.getItem('measure-calls') ?? 0) + 1),
          )
          if (sessionStorage.getItem('measure-fail')) throw new Error('fixture 测量读取受限')
          const result = {
            ...measurement,
            path: (args as { path: string }).path,
            checkedAt: Date.now(),
          }
          if (sessionStorage.getItem('measure-zero'))
            Object.assign(result, { allocatedBytes: 0, files: 0 })
          if (sessionStorage.getItem('measure-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishMeasurement: () => resolve(result) }),
            )
          if (sessionStorage.getItem('measure-partial'))
            return { ...result, completion: 'partial', files: 0, allocatedBytes: 0, hardLinks: 2 }
          return result
        }
        if (command === 'inspect_project_artifacts') {
          sessionStorage.setItem('project-args', JSON.stringify(args))
          sessionStorage.setItem(
            'project-calls',
            String(Number(sessionStorage.getItem('project-calls') ?? 0) + 1),
          )
          if (sessionStorage.getItem('project-fail')) throw new Error('fixture 目录读取受限')
          const result = structuredClone({ ...fixture, checkedAt: Date.now() })
          if (sessionStorage.getItem('project-large')) {
            result.items = Array.from({ length: 65 }, (_, n) => ({
              ...result.items[0],
              path: `/fixture/projects/p${String(n).padStart(2, '0')}/node_modules`,
              projectPath: `/fixture/projects/p${String(n).padStart(2, '0')}`,
              projectHint: {
                state: 'hint' as const,
                path: fixture.root,
                kind: 'workspace' as const,
                markers: ['.git'],
              },
            }))
            result.visited = 100
          }
          if (sessionStorage.getItem('project-workspace')) {
            for (const item of result.items.slice(0, 2))
              item.projectHint = {
                state: 'hint',
                path: fixture.root,
                kind: 'workspace',
                markers: ['.git'],
              }
          }
          if (sessionStorage.getItem('project-unknown'))
            result.items[2].projectHint = { state: 'unknown', path: null, kind: null, markers: [] }
          if (sessionStorage.getItem('project-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishProjects: () => resolve(result) }),
            )
          if (sessionStorage.getItem('project-partial'))
            return { ...result, completion: 'partial', skipped: { ...fixture.skipped, links: 1 } }
          return result
        }
        return original(command, args)
      }
    },
    { fixture: projectArtifactsReport(), measurement: projectMeasurement() },
  )
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
}
test('explicit scope discovery keeps protection and unknown measurement, then opens read-only space analysis', async ({
  page,
}) => {
  await install(page)
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBeNull()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBeNull()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '工程产物结果' })
  await expect(region).toContainText('发现 3 处 Mole 命名目录')
  await expect(region.getByText('未测量', { exact: true })).toHaveCount(3)
  await expect(region).toContainText('客户端保护命中，保持现状')
  await expect(region).toContainText('客户端保护无法确认，保持现状')
  await expect(region).toContainText('执行未开放')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('project-args')!))).toEqual({
    root: '/fixture/projects',
  })
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.evaluate(() => sessionStorage.setItem('project-picker-cancel', 'yes'))
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await expect(region.getByRole('article')).toHaveCount(3)
  await region
    .getByRole('article', { name: '/fixture/projects/web/node_modules', exact: true })
    .getByRole('button', { name: '查看空间', exact: true })
    .click()
  await expect(page.getByRole('button', { name: '空间分析', exact: true })).toHaveAttribute(
    'aria-current',
    'page',
  )
  await expect
    .poll(() =>
      page.evaluate(() => JSON.parse(sessionStorage.getItem('mock-starts') ?? '[]').at(-1)),
    )
    .toMatchObject({ kind: 'analyze', path: '/fixture/projects/web/node_modules' })
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByRole('textbox', { name: '扫描目录' })).toHaveValue(
    '/fixture/projects/web/node_modules',
  )
})
test('partial results stay explicit and failed refresh retains prior evidence', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('project-partial', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '工程产物结果' })
  await expect(region).toContainText('扫描不完整')
  await expect(region).toContainText('未显示不代表不存在')
  await page.evaluate(() => sessionStorage.setItem('project-fail', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(region.getByRole('alert')).toContainText('本次未更新')
  await expect(region.getByRole('article')).toHaveCount(3)
})
test('leaving the page ignores a late scan and never restarts it automatically', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('project-defer', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(page.getByRole('button', { name: '正在扫描…', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  await page.evaluate(() => (window as unknown as { finishProjects: () => void }).finishProjects())
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(page.getByRole('region', { name: '工程产物结果' }).getByRole('article')).toHaveCount(
    0,
  )
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBe('1')
})

test('measures only the requested row, preserves evidence on retry failure and clears it after rediscovery', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('measure-args'))).toBeNull()
  const row = page.getByRole('article', { name: '/fixture/projects/web/node_modules', exact: true })
  await row.getByRole('button', { name: '测量占用', exact: true }).click()
  await expect(row).toContainText('8 KB')
  await expect(row).toContainText('已测文件占用')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('measure-args')!))).toEqual({
    root: '/fixture/projects',
    path: '/fixture/projects/web/node_modules',
  })
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.evaluate(() => sessionStorage.setItem('measure-fail', 'yes'))
  await row.getByRole('button', { name: '重新测量' }).click()
  await expect(row.getByRole('alert')).toContainText('保留上次测量，本次未更新')
  await expect(row).toContainText('8 KB')
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(row.getByText('未测量', { exact: true })).toBeVisible()
})
test('zero observed files with exclusions never becomes a confirmed zero size', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('measure-partial', 'yes'))
  const row = page.getByRole('article', { name: '/fixture/projects/web/node_modules', exact: true })
  await row.getByRole('button', { name: '测量占用', exact: true }).click()
  await expect(row).toContainText('占用未确认')
  await expect(row).toContainText('跳过共享硬链接 2 次')
  await expect(row).not.toContainText('0 B')
  await expect(row).toContainText('部分文件占用')
})
test('measurement holds scope controls and ignores late responses after leaving the page', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('measure-defer', 'yes'))
  const row = page.getByRole('article', { name: '/fixture/projects/web/node_modules', exact: true })
  await row.getByRole('button', { name: '测量占用', exact: true }).click()
  await expect(row.getByRole('button', { name: '正在测量…' })).toBeDisabled()
  await expect(page.getByRole('button', { name: '选择目录', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '系统维护', exact: true }).click()
  await page.evaluate(() =>
    (window as unknown as { finishMeasurement: () => void }).finishMeasurement(),
  )
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(row.getByText('未测量', { exact: true })).toBeVisible()
  await expect(row).not.toContainText('8 KB')
})

test('groups workspace markers without asserting ownership and leaves uncertain evidence unresolved', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('project-workspace', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const workspace = page.getByRole('region', { name: '项目线索 /fixture/projects', exact: true })
  await expect(workspace.getByRole('article')).toHaveCount(2)
  await expect(workspace.getByRole('heading')).toContainText('工作区线索')
  const unresolved = page.getByRole('region', { name: '归属待确认', exact: true })
  await expect(unresolved).toContainText('所选范围内未找到项目标记')
  await page.evaluate(() => sessionStorage.setItem('project-unknown', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(unresolved).toContainText('项目标记无法确认；保留未知')
  await expect(workspace).toContainText('未命中客户端保护；其他清理条件未确认')
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('Mole saved roots load explicitly, select one scope without scanning, and fit a desktop toolbar', async ({
  page,
}) => {
  await install(page)
  const region = page.getByRole('region', { name: '工程产物结果' })
  const choices = page.getByRole('combobox', { name: 'Mole 已保存的项目目录' })
  expect(await page.evaluate(() => sessionStorage.getItem('roots-calls'))).toBeNull()
  await expect(choices).toHaveCount(0)
  await page.getByRole('button', { name: '读取 Mole 目录', exact: true }).click()
  await expect(region).toContainText('已读取 2 处 Mole 目录')
  await expect(region).toContainText('目录是否可访问尚未检查')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('roots-args')!))).toEqual({})
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBeNull()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await choices.selectOption('/fixture/projects')
  await expect(region).toContainText('目录已选择')
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBeNull()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(region).toContainText('发现 3 处 Mole 命名目录')
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(region).toContainText('发现 3 处 Mole 命名目录')
  expect(await page.evaluate(() => sessionStorage.getItem('roots-calls'))).toBe('1')
  const second = await choices.locator('option').last().getAttribute('value')
  await choices.selectOption(second!)
  await expect(region).toContainText('目录已选择')
  await expect(region.getByRole('article')).toHaveCount(0)
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBe('1')
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('missing and empty configs explain manual selection, and failure withdraws the prior root list', async ({
  page,
}) => {
  await install(page)
  const region = page.getByRole('region', { name: '工程产物结果' })
  const load = page.getByRole('button', { name: '读取 Mole 目录', exact: true })
  for (const [key, message] of [
    ['roots-missing', 'Mole 尚未保存项目目录'],
    ['roots-empty', 'Mole 保存的目录列表为空'],
  ]) {
    await page.evaluate((key) => sessionStorage.setItem(key, 'yes'), key)
    await load.click()
    await expect(region).toContainText(message)
    await expect(page.getByRole('combobox', { name: 'Mole 已保存的项目目录' })).toHaveCount(0)
    await page.evaluate((key) => sessionStorage.removeItem(key), key)
  }
  await load.click()
  await page
    .getByRole('combobox', { name: 'Mole 已保存的项目目录' })
    .selectOption('/fixture/projects')
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('roots-fail', 'yes'))
  await load.click()
  await expect(region.getByRole('alert')).toContainText('先前的配置列表已撤回')
  await expect(page.getByRole('combobox', { name: 'Mole 已保存的项目目录' })).toHaveCount(0)
  await expect(region).toContainText('发现 3 处 Mole 命名目录')
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeEnabled()
  await page.evaluate(() => {
    sessionStorage.removeItem('roots-fail')
    sessionStorage.setItem('roots-malformed', 'yes')
  })
  await load.click()
  await expect(region.getByRole('alert')).toContainText('配置结果无法确认')
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await expect(region).toContainText('目录已选择')
})

test('root reading holds other operations, withdraws cached options, and ignores late responses after navigation', async ({
  page,
}) => {
  await install(page)
  const load = page.getByRole('button', { name: '读取 Mole 目录', exact: true })
  await load.click()
  await page
    .getByRole('combobox', { name: 'Mole 已保存的项目目录' })
    .selectOption('/fixture/projects')
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('roots-defer', 'yes'))
  await load.click()
  await expect(page.getByRole('button', { name: '正在读取…', exact: true })).toBeDisabled()
  await expect(page.getByRole('combobox', { name: 'Mole 已保存的项目目录' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '选择目录', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  for (const button of await page.getByRole('button', { name: '测量占用', exact: true }).all())
    await expect(button).toBeDisabled()
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.evaluate(() => (window as unknown as { finishRoots: () => void }).finishRoots())
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(page.getByRole('combobox', { name: 'Mole 已保存的项目目录' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '读取 Mole 目录', exact: true })).toBeEnabled()
})

test('project filters preserve scan coverage, protection and session state without new work', async ({
  page,
}) => {
  await install(page)
  await page.evaluate(() => sessionStorage.setItem('project-partial', 'yes'))
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '工程产物结果' })
  const query = region.getByRole('textbox', { name: '筛选工程产物' })
  const context = region.getByRole('combobox', { name: '工程产物类型线索' })
  await query.fill('RUST')
  await expect(region.getByRole('article')).toHaveCount(1)
  await expect(region).toContainText('客户端保护命中，保持现状')
  await expect(region).toContainText('扫描不完整')
  await expect(region).toContainText('跳过链接 1 次')
  await context.selectOption('nodeContext')
  await expect(region).toContainText('当前筛选没有匹配目录')
  await expect(region).toContainText('本次共发现 3 处')
  await region.getByRole('button', { name: '清除筛选' }).click()
  await expect(region.getByRole('article')).toHaveCount(3)
  await context.selectOption('nameOnly')
  await expect(region).toContainText('客户端保护无法确认，保持现状')
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(context).toHaveValue('nameOnly')
  await expect(region.getByRole('article')).toHaveCount(1)
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBe('1')
  expect(await page.evaluate(() => sessionStorage.getItem('measure-calls'))).toBeNull()
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(query).toHaveValue('')
  await expect(context).toHaveValue('')
  await expect(region.getByRole('article')).toHaveCount(3)
})

test('project sorting compares observed files only within groups and keeps unknown distinct from zero', async ({
  page,
}) => {
  await install(page)
  await page.evaluate(() => sessionStorage.setItem('project-workspace', 'yes'))
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const group = page.getByRole('region', { name: '项目线索 /fixture/projects', exact: true })
  const web = group.getByRole('article', {
    name: '/fixture/projects/web/node_modules',
    exact: true,
  })
  const rust = group.getByRole('article', { name: '/fixture/projects/rust/target', exact: true })
  const sort = page.getByRole('combobox', { name: '工程产物组内排序' })
  await sort.selectOption('size')
  await web.getByRole('button', { name: '测量占用', exact: true }).click()
  await expect(web).toContainText('8 KB')
  await page.evaluate(() => sessionStorage.setItem('measure-zero', 'yes'))
  await rust.getByRole('button', { name: '测量占用', exact: true }).click()
  await expect(rust).toContainText('0 B')
  await expect(group.getByRole('article').first()).toHaveAttribute(
    'aria-label',
    '/fixture/projects/web/node_modules',
  )
  await expect(page.getByRole('region', { name: '归属待确认', exact: true })).toContainText(
    '未测量',
  )
  await expect(page.getByText('仅比较组内已测文件占用', { exact: false })).toBeVisible()
  await page.evaluate(() => {
    sessionStorage.removeItem('measure-zero')
    sessionStorage.setItem('measure-defer', 'yes')
  })
  await web.getByRole('button', { name: '重新测量', exact: true }).click()
  await expect(sort).toBeDisabled()
  await expect(page.getByRole('textbox', { name: '筛选工程产物' })).toBeDisabled()
  await page.evaluate(() =>
    (window as unknown as { finishMeasurement: () => void }).finishMeasurement(),
  )
  await expect(sort).toBeEnabled()
  expect(await page.evaluate(() => sessionStorage.getItem('measure-calls'))).toBe('3')
})

test('project pagination retains page on navigation, resets on filtering and clamps smaller refreshes', async ({
  page,
}) => {
  await install(page)
  await page.evaluate(() => sessionStorage.setItem('project-large', 'yes'))
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '工程产物结果' })
  const pages = region.getByRole('group', { name: '工程产物分页' })
  await expect(region.getByRole('article')).toHaveCount(50)
  await pages.getByRole('button', { name: '下一页' }).click()
  await expect(region.getByRole('article')).toHaveCount(15)
  await expect(pages).toContainText('第 2 / 2 页')
  await page.getByRole('button', { name: '概览', exact: true }).click()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await expect(pages).toContainText('第 2 / 2 页')
  await region.getByRole('textbox', { name: '筛选工程产物' }).fill('p00/')
  await expect(pages).toHaveCount(0)
  await expect(region.getByRole('article')).toHaveCount(1)
  await region.getByRole('button', { name: '清除筛选' }).click()
  await expect(region.getByRole('article')).toHaveCount(50)
  await pages.getByRole('button', { name: '下一页' }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('project-calls'))).toBe('1')
  await page.evaluate(() => sessionStorage.removeItem('project-large'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(pages).toHaveCount(0)
  await expect(region.getByRole('article')).toHaveCount(3)
})
