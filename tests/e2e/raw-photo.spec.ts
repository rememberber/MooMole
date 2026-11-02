import { test, expect, type Page } from '@playwright/test'
import { installSimilarMock } from './fixtures/similar-photos-native'
async function setup(page: Page, mode = 'normal') {
  await installSimilarMock(page, true)
  await page.addInitScript((mode) => sessionStorage.setItem('raw-preview-mode', mode), mode)
  await page.goto('/')
  await page
    .getByRole('navigation')
    .getByRole('button', { name: '相似照片与视频', exact: true })
    .click()
  const main = page.getByRole('main', { name: '相似照片与视频内容' })
  await main.getByRole('textbox', { name: '相似照片目录' }).fill('/Users/demo/Downloads')
  await page.getByRole('button', { name: '查找相似照片', exact: true }).click()
  const list = main.getByRole('region', { name: '个人文件列表' })
  await expect(list.getByRole('article')).toHaveCount(3)
  return { main, list }
}
test('RAW preview and comparison retain an original and require a separate exact-file confirmation', async ({
  page,
}, info) => {
  const { main, list } = await setup(page)
  await expect(list.getByText('RAW 系统预览', { exact: true })).toHaveCount(2)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await main.getByRole('combobox', { name: '个人文件类型' }).selectOption('image')
  await expect(list.getByRole('article')).toHaveCount(3)
  await list.getByRole('article').first().getByRole('button', { name: '预览内容' }).click()
  const d = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(d.getByRole('img')).toBeVisible()
  await expect(d).toContainText('RAW 原片与 JPEG 不等价')
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('raw-preview-zh.png'), animations: 'disabled' })
  await d.getByRole('button', { name: '返回文件列表' }).click()
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  await list.getByRole('article').nth(0).getByRole('checkbox').check()
  await list.getByRole('article').nth(1).getByRole('checkbox').check()
  await expect(list.getByRole('article').nth(2).getByRole('checkbox')).toBeDisabled()
  await main.getByRole('button', { name: '移入废纸篓…' }).click()
  const confirm = page.getByRole('dialog', { name: '确认处理相似照片' })
  await expect(confirm).toContainText('保留 JPEG 不能替代 RAW')
  const execute = confirm.getByRole('button', { name: '确认移入废纸篓', exact: true })
  await expect(execute).toBeDisabled()
  await confirm.getByRole('checkbox').check()
  await execute.click()
  await expect(main.getByRole('region', { name: '个人文件处理结果' })).toBeVisible()
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.filter((c: any) => c.command === 'preview_raw_photo')).toHaveLength(1)
  expect(calls.find((c: any) => c.command === 'preview_raw_photo').args).toHaveProperty('requestId')
  expect(
    calls.find((c: any) => c.command === 'prepare_similar_trash').args.selectedIds,
  ).toHaveLength(2)
  expect(calls.filter((c: any) => c.command === 'execute_similar_trash')).toHaveLength(1)
})
for (const mode of ['unsupported', 'mismatch', 'wrongKind'])
  test(`${mode} RAW data stays unselected and cannot masquerade as a decoded RAW preview`, async ({
    page,
  }) => {
    const { list } = await setup(page, mode)
    await list.getByRole('article').first().getByRole('button', { name: '预览内容' }).click()
    const d = page.getByRole('dialog', { name: '文件内容预览' })
    await expect(d.getByRole('alert')).toBeVisible()
    await expect(d.getByRole('img')).toHaveCount(0)
    await d.getByRole('button', { name: '返回文件列表' }).click()
    await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
    const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
    expect(calls.filter((c: any) => c.command === 'execute_similar_trash')).toHaveLength(0)
  })
test('closing a pending RAW preview cancels only its request and discards a late image', async ({
  page,
}) => {
  const { list } = await setup(page, 'pending')
  await list.getByRole('article').first().getByRole('button', { name: '预览内容' }).click()
  const d = page.getByRole('dialog', { name: '文件内容预览' })
  await expect(d).toContainText('关闭窗口会停止本次任务')
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          JSON.parse(sessionStorage.getItem('similar-calls')!).filter(
            (c: any) => c.command === 'preview_raw_photo',
          ).length,
      ),
    )
    .toBe(1)
  await d.getByRole('button', { name: '返回文件列表' }).click()
  await page.evaluate(() => (window as any).releaseOwnedRawPreview())
  await expect(d).toHaveCount(0)
  const calls = await page.evaluate(() => JSON.parse(sessionStorage.getItem('similar-calls')!))
  expect(calls.find((c: any) => c.command === 'cancel_task').args.taskId).toBe(
    calls.find((c: any) => c.command === 'preview_raw_photo').args.requestId,
  )
  expect(calls.filter((c: any) => c.command === 'execute_similar_trash')).toHaveLength(0)
  await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
})
for (const language of ['en', 'ja'])
  test(`${language} RAW warning keeps original path and zero selections`, async ({
    page,
  }, info) => {
    const { list } = await setup(page)
    await list.getByRole('article').first().getByRole('button', { name: '预览内容' }).click()
    await page.evaluate(
      async (language) =>
        (await import('/src/i18n/locale.ts')).setLanguagePreference(language as 'en' | 'ja'),
      language,
    )
    const d = page.getByRole('dialog', {
      name: language === 'en' ? 'File content preview' : 'ファイル内容のプレビュー',
    })
    await expect(d.getByRole('img')).toBeVisible()
    await expect(d).toContainText('/Users/demo/Downloads/one.dng')
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await d.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true)
    await page.screenshot({
      path: info.outputPath(`raw-preview-${language}.png`),
      animations: 'disabled',
    })
    await expect(list.getByRole('checkbox', { checked: true })).toHaveCount(0)
  })
