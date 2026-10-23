import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

const folder = '/fixture/需要保留的目录'
const dialog = (page: Page) => page.getByRole('dialog', { name: '设置', exact: true })
const add = (page: Page) => dialog(page).getByRole('button', { name: '添加目录', exact: true })

async function open(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await dialog(page).getByRole('button', { name: '保护列表', exact: true }).click()
}

async function setup(page: Page) {
  await installTaskMock(page, 'new')
  await page.addInitScript(() => {
    const internal = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const previous = internal.invoke
    internal.invoke = async (command, args) => {
      if (command === 'bootstrap') {
        const initial = (await previous(command, args)) as Record<string, unknown>
        const saved = sessionStorage.getItem('protection-saved')
        return saved ? { ...initial, settings: JSON.parse(saved) } : initial
      }
      if (command === 'plugin:dialog|open') {
        const id = Number(sessionStorage.getItem('picker-count') ?? '0') + 1
        sessionStorage.setItem('picker-count', String(id))
        sessionStorage.setItem('picker-options', JSON.stringify(args))
        while (!sessionStorage.getItem(`picker-result-${id}`))
          await new Promise((resolve) => setTimeout(resolve, 10))
        const result = JSON.parse(sessionStorage.getItem(`picker-result-${id}`)!)
        sessionStorage.setItem(`picker-done-${id}`, 'yes')
        if (result.error) throw new Error(result.error)
        return result.path
      }
      if (command === 'save_settings') {
        const requests = JSON.parse(sessionStorage.getItem('protection-requests') ?? '[]')
        requests.push(args?.settings)
        sessionStorage.setItem('protection-requests', JSON.stringify(requests))
        while (sessionStorage.getItem('protection-save-hold'))
          await new Promise((resolve) => setTimeout(resolve, 10))
        if (sessionStorage.getItem('protection-save-fail'))
          throw new Error('保护目录保存失败，请重试。')
        sessionStorage.setItem('protection-saved', JSON.stringify(args?.settings))
        return args?.settings
      }
      return previous(command, args)
    }
  })
  await page.goto('/')
  await open(page)
}

async function result(page: Page, id: number, value: { path?: string | null; error?: string }) {
  await page.evaluate(
    ({ id, value }) => {
      sessionStorage.setItem(`picker-result-${id}`, JSON.stringify(value))
    },
    { id, value },
  )
  await expect
    .poll(() => page.evaluate((id) => sessionStorage.getItem(`picker-done-${id}`), id))
    .toBe('yes')
}

async function saves(page: Page) {
  return page.evaluate(() => JSON.parse(sessionStorage.getItem('protection-requests') ?? '[]'))
}

test('directory selection locks all settings mutations until the save finishes', async ({
  page,
}) => {
  await setup(page)
  await page.evaluate(() => sessionStorage.setItem('protection-save-hold', 'yes'))
  await add(page).click()
  await expect(dialog(page).getByRole('status')).toHaveText('请在系统窗口中选择目录…')
  await expect(add(page)).toBeDisabled()
  await expect.poll(() => page.evaluate(() => sessionStorage.getItem('picker-count'))).toBe('1')
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('picker-options')!)),
  ).toMatchObject({
    options: { title: '选择要保护的文件夹', directory: true, multiple: false },
  })
  await dialog(page).getByRole('button', { name: '通用', exact: true }).click()
  await expect(dialog(page).getByRole('combobox', { name: '主题', exact: true })).toBeDisabled()
  await expect(
    dialog(page).getByRole('combobox', { name: '历史记录保留时间', exact: true }),
  ).toBeDisabled()
  expect(await saves(page)).toEqual([])
  await result(page, 1, { path: folder })
  await expect(dialog(page).getByRole('status')).toHaveText('正在保存…')
  await expect(dialog(page).getByRole('combobox', { name: '主题', exact: true })).toBeDisabled()
  await page.evaluate(() => sessionStorage.removeItem('protection-save-hold'))
  await expect(dialog(page).getByRole('combobox', { name: '主题', exact: true })).toBeEnabled()
  await dialog(page).getByRole('button', { name: '保护列表', exact: true }).click()
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
  expect(await saves(page)).toHaveLength(1)
  await page.reload()
  await open(page)
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
})

test('cancel and picker failure leave settings unchanged and allow a new attempt', async ({
  page,
}) => {
  await setup(page)
  await add(page).click()
  await result(page, 1, { path: null })
  await expect(add(page)).toBeEnabled()
  await expect(dialog(page).getByRole('status')).toHaveCount(0)
  await expect(add(page)).toBeFocused()
  await add(page).click()
  await result(page, 2, { error: '系统目录窗口暂时无法打开' })
  await expect(dialog(page).getByRole('alert')).toContainText('系统目录窗口暂时无法打开')
  await expect(add(page)).toBeEnabled()
  await expect(add(page)).toBeFocused()
  expect(await saves(page)).toEqual([])
  await add(page).click()
  await expect(dialog(page).getByRole('alert')).toHaveCount(0)
  await result(page, 3, { path: folder })
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
  expect(await saves(page)).toHaveLength(1)
})

test('duplicate directory explains the result without writing settings or pruning history again', async ({
  page,
}) => {
  await setup(page)
  await add(page).click()
  await result(page, 1, { path: folder })
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
  await add(page).click()
  await expect(dialog(page).getByRole('button', { name: `移除保护 ${folder}` })).toBeDisabled()
  await result(page, 2, { path: folder })
  await expect(dialog(page).getByRole('status')).toHaveText('此目录已在保护列表中。')
  await expect(add(page)).toBeEnabled()
  await expect(add(page)).toBeFocused()
  expect(await saves(page)).toHaveLength(1)
})

test('save failure keeps the old protection list and retry uses a new explicit selection', async ({
  page,
}) => {
  await setup(page)
  await page.evaluate(() => sessionStorage.setItem('protection-save-fail', 'yes'))
  await add(page).click()
  await result(page, 1, { path: folder })
  await expect(dialog(page).getByRole('alert')).toContainText('保护目录保存失败，请重试')
  await expect(dialog(page).getByText('还没有添加保护目录')).toBeVisible()
  await expect(add(page)).toBeEnabled()
  await expect(add(page)).toBeFocused()
  await page.evaluate(() => sessionStorage.removeItem('protection-save-fail'))
  await add(page).click()
  await result(page, 2, { path: folder })
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
  await expect(dialog(page).getByRole('alert')).toHaveCount(0)
  expect(await saves(page)).toHaveLength(2)
})

test('a previous explicit save finishing after reopen is preserved by the later directory selection', async ({
  page,
}) => {
  await setup(page)
  await page.evaluate(() => sessionStorage.setItem('protection-save-hold', 'yes'))
  await dialog(page).getByRole('button', { name: '通用', exact: true }).click()
  await dialog(page).getByRole('combobox', { name: '主题', exact: true }).selectOption('dark')
  await expect(dialog(page).getByRole('status')).toHaveText('正在保存…')
  await page.keyboard.press('Escape')
  await open(page)
  await add(page).click()
  await page.evaluate(() => sessionStorage.removeItem('protection-save-hold'))
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
  await result(page, 1, { path: folder })
  await expect(dialog(page).getByText(folder, { exact: true })).toBeVisible()
  const requests = await saves(page)
  expect(requests).toHaveLength(2)
  expect(requests[1]).toMatchObject({ theme: 'dark', protectedPaths: [folder] })
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
})

for (const late of [{ path: folder }, { error: '迟到的系统错误' }]) {
  test(`closed settings discard a late picker ${late.path ? 'result' : 'error'} without affecting a new session`, async ({
    page,
  }) => {
    await setup(page)
    await add(page).click()
    await page.keyboard.press('Escape')
    await expect(dialog(page)).toHaveCount(0)
    await open(page)
    await add(page).click()
    await result(page, 1, late)
    await expect(dialog(page).getByRole('status')).toHaveText('请在系统窗口中选择目录…')
    await expect(dialog(page).getByRole('alert')).toHaveCount(0)
    await expect(add(page)).toBeDisabled()
    expect(await saves(page)).toEqual([])
    await result(page, 2, { path: '/fixture/新选择' })
    await expect(dialog(page).getByText('/fixture/新选择', { exact: true })).toBeVisible()
    await expect(dialog(page).getByText(folder, { exact: true })).toHaveCount(0)
    expect(await saves(page)).toHaveLength(1)
  })
}
