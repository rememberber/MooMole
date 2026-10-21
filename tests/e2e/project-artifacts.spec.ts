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
        if (command === 'measure_project_artifact') {
          sessionStorage.setItem('measure-args', JSON.stringify(args))
          if (sessionStorage.getItem('measure-fail')) throw new Error('fixture 测量读取受限')
          const result = { ...measurement, checkedAt: Date.now() }
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
          const result = { ...fixture, checkedAt: Date.now() }
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
  await region.getByRole('button', { name: '查看空间', exact: true }).nth(0).click()
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
