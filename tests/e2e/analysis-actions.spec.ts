import { test, expect } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { analysis } from './fixtures/analysis-explorer'

test('analysis hands the exact directory to file organization and duplicates without starting file operations', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze', undefined, undefined, analysis)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录', exact: true }).fill(analysis.path)
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await expect(explorer).toBeVisible()
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button', { name: /^查看 Projects/ })
    .click()
  await explorer
    .getByRole('region', { name: '选中项目详情' })
    .getByRole('button', { name: '整理此文件夹', exact: true })
    .click()
  await expect(
    page.getByRole('main', { name: '文件整理内容' }).getByRole('textbox', { name: '个人文件目录' }),
  ).toHaveValue('/fixture/analysis/Projects')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '空间分析', exact: true })
    .click()
  await expect(explorer.getByRole('region', { name: '选中项目详情' })).toContainText('Projects')
  await explorer.getByRole('button', { name: '查找当前目录的重复文件', exact: true }).click()
  await expect(
    page.getByRole('main', { name: '重复文件内容' }).getByRole('textbox', { name: '重复文件目录' }),
  ).toHaveValue(analysis.path)
  const commands = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('mock-native-commands')!),
  )
  expect(
    commands.filter((r: { command: string }) =>
      /^(start_personal_scan|start_duplicate_scan|execute_personal_|execute_duplicate_)/.test(
        r.command,
      ),
    ),
  ).toHaveLength(0)
})

test('a selected file opens its containing directory and root actions keep the selected mode', async ({
  page,
}) => {
  await page.goto('/')
  await page.evaluate(async () =>
    (await import('/tests/e2e/fixtures/analysis-explorer.tsx')).mount(false, undefined, true),
  )
  const explorer = page.getByRole('region', { name: 'Mole 空间分析结果' })
  await explorer.getByRole('button', { name: '整理当前目录', exact: true }).click()
  await expect(page.getByLabel('fixture 路径请求')).toHaveText('files:/fixture/analysis')
  await explorer
    .getByRole('group', { name: '当前筛选的空间图' })
    .getByRole('button', { name: /^查看 video.mp4/ })
    .click()
  await explorer
    .getByRole('region', { name: '选中项目详情' })
    .getByRole('button', { name: '整理所在目录', exact: true })
    .click()
  await expect(page.getByLabel('fixture 路径请求')).toHaveText('files:/fixture/analysis')
  await explorer
    .getByRole('region', { name: '选中项目详情' })
    .getByRole('button', { name: '查找同目录的重复文件', exact: true })
    .click()
  await expect(page.getByLabel('fixture 路径请求')).toHaveText('duplicates:/fixture/analysis')
})
