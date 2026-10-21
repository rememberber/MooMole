import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import {
  installersReport,
  installerZipInspection,
  installerConnectionInspection,
} from './fixtures/installers'
async function install(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(
    ({ fixture, zip, connection }) => {
      const native = (
        window as unknown as {
          __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> }
        }
      ).__TAURI_INTERNALS__
      const original = native.invoke
      native.invoke = async (command, args) => {
        if (command === 'plugin:dialog|open')
          return sessionStorage.getItem('installer-cancel') ? null : fixture.root
        if (command === 'reveal_path') {
          sessionStorage.setItem('installer-reveal', JSON.stringify(args))
          return null
        }
        if (command === 'inspect_installer_connection') {
          sessionStorage.setItem('connection-args', JSON.stringify(args))
          if (sessionStorage.getItem('connection-fail')) throw new Error('fixture 映像路径已变化')
          const result = structuredClone({ ...connection, checkedAt: Date.now() })
          if (sessionStorage.getItem('connection-negative')) result.state = 'pathNotListed'
          if (sessionStorage.getItem('connection-unknown')) {
            result.state = 'unknown'
            result.reason = 'listingUncertain'
          }
          if (sessionStorage.getItem('connection-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishConnection: () => resolve(result) }),
            )
          return result
        }
        if (command === 'inspect_installer_zip') {
          sessionStorage.setItem('zip-args', JSON.stringify(args))
          if (sessionStorage.getItem('zip-fail')) throw new Error('fixture ZIP 已变化，请重新扫描')
          const result = structuredClone({ ...zip, checkedAt: Date.now() })
          if (sessionStorage.getItem('zip-negative')) {
            result.state = 'noHintInWindow'
            result.matchedTypes = []
          }
          if (sessionStorage.getItem('zip-unknown')) {
            result.state = 'unknown'
            result.reason = 'listingLimit'
            result.totalEntries = null
            result.checkedEntries = 0
            result.matchedTypes = []
          }
          if (sessionStorage.getItem('zip-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishZip: () => resolve(result) }),
            )
          return result
        }
        if (command === 'inspect_installers') {
          sessionStorage.setItem('installer-args', JSON.stringify(args))
          sessionStorage.setItem(
            'installer-calls',
            String(Number(sessionStorage.getItem('installer-calls') ?? 0) + 1),
          )
          if (sessionStorage.getItem('installer-missing'))
            throw new Error('fixture 目录不存在或无权访问')
          if (sessionStorage.getItem('installer-fail')) throw new Error('fixture 安装包读取受限')
          const result = structuredClone({ ...fixture, checkedAt: Date.now() })
          if (sessionStorage.getItem('zip-enable')) result.items[1].sharing = 'single'
          if (sessionStorage.getItem('installer-partial')) {
            result.completion = 'partial'
            result.skipped.links = 2
          }
          if (sessionStorage.getItem('installer-empty')) result.items = []
          if (sessionStorage.getItem('installer-defer'))
            return new Promise((resolve) =>
              Object.assign(window, { finishInstallers: () => resolve(result) }),
            )
          return result
        }
        return original(command, args)
      }
    },
    {
      fixture: installersReport(),
      zip: installerZipInspection(),
      connection: installerConnectionInspection(),
    },
  )
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '安装包', exact: true }).click()
}
test('manual scoped inventory displays filename clues, unknown ZIP size and accurate Finder action', async ({
  page,
}) => {
  await install(page)
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  expect(await page.evaluate(() => sessionStorage.getItem('installer-calls'))).toBeNull()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '安装包扫描结果' })
  await expect(region).toContainText('发现 2 个文件线索')
  await expect(
    page.getByRole('main', { name: '安装包内容' }).getByRole('region', { name: '安装包扫描结果' }),
  ).toBeVisible()
  expect(
    (await page.getByRole('banner', { name: '安装包工具栏' }).boundingBox())!.height,
  ).toBeLessThanOrEqual(64)

  await expect(region).toContainText('文件占用尚未检查，清理未开放')
  const zip = region.getByRole('article', { name: '/fixture/downloads/archive.zip', exact: true })
  await expect(zip).toContainText('ZIP 内容未确认')
  await expect(zip).toContainText('大小未知')
  await expect(zip).toContainText('客户端保护命中')
  await expect(zip).toContainText('共享硬链接')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('installer-args')!))).toEqual({
    root: '/fixture/downloads',
  })
  await region
    .getByRole('article', { name: '/fixture/downloads/Tool.dmg', exact: true })
    .getByRole('button', { name: '在 Finder 中显示' })
    .click()
  expect(
    await page.evaluate(() => JSON.parse(sessionStorage.getItem('installer-reveal')!)),
  ).toEqual({ path: '/fixture/downloads/Tool.dmg' })
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})
test('partial coverage and failed refresh preserve prior evidence, while picker cancellation keeps results', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('installer-partial', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '安装包扫描结果' })
  await expect(region).toContainText('扫描不完整')
  await expect(region).toContainText('跳过链接 2 次')
  await page.evaluate(() => sessionStorage.setItem('installer-cancel', 'yes'))
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await expect(region.getByRole('article')).toHaveCount(2)
  await page.evaluate(() => sessionStorage.setItem('installer-fail', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(region.getByRole('alert')).toContainText('保留上次结果，本次未更新')
  await expect(region.getByRole('article')).toHaveCount(2)
})
test('leaving ignores late inventory and does not auto-restart', async ({ page }) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('installer-defer', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(page.getByRole('button', { name: '正在扫描…' })).toBeDisabled()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await page.evaluate(() =>
    (window as unknown as { finishInstallers: () => void }).finishInstallers(),
  )
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(page.getByRole('heading', { name: '目录已选择' })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('installer-calls'))).toBe('1')
})
test('empty partial results keep missing coverage visible without a cleanup claim', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => {
    sessionStorage.setItem('installer-partial', 'yes')
    sessionStorage.setItem('installer-empty', 'yes')
  })
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '安装包扫描结果' })
  await expect(region).toContainText('未显示不代表不存在')
  await expect(region).toContainText('本次未发现匹配文件')
  await expect(region.getByRole('article')).toHaveCount(0)
})

async function prepareZip(page: Page) {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('zip-enable', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  return page.getByRole('article', { name: '/fixture/downloads/archive.zip', exact: true })
}
test('checks ZIP only on demand and removes positive hints when a recheck fails', async ({
  page,
}) => {
  const zip = await prepareZip(page)
  expect(await page.evaluate(() => sessionStorage.getItem('zip-args'))).toBeNull()
  await zip.getByRole('button', { name: '检查 ZIP', exact: true }).click()
  await expect(zip).toContainText('发现安装条目线索（APP）')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('zip-args')!))).toEqual({
    root: '/fixture/downloads',
    path: '/fixture/downloads/archive.zip',
  })
  await page.evaluate(() => sessionStorage.setItem('zip-fail', 'yes'))
  await zip.getByRole('button', { name: '重新检查', exact: true }).click()
  await expect(zip.getByRole('alert')).toContainText('先前线索已清除')
  await expect(zip).toContainText('ZIP 内容未确认')
  await expect(zip).not.toContainText('发现安装条目线索')
})
test('negative windows and unknown limits remain different, and rediscovery clears ZIP observations', async ({
  page,
}) => {
  const zip = await prepareZip(page)
  await page.evaluate(() => sessionStorage.setItem('zip-negative', 'yes'))
  await zip.getByRole('button', { name: '检查 ZIP', exact: true }).click()
  await expect(zip).toContainText('前 50 条未见安装线索')
  await page.evaluate(() => sessionStorage.setItem('zip-unknown', 'yes'))
  await zip.getByRole('button', { name: '重新检查', exact: true }).click()
  await expect(zip).toContainText('条目列表超出检查限制')
  await expect(zip).toContainText('ZIP 内容未确认')
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(zip.getByRole('button', { name: '检查 ZIP', exact: true })).toBeVisible()
  await expect(zip).not.toContainText('上次检查')
})
test('ZIP reading holds scope controls and ignores late results after navigation', async ({
  page,
}) => {
  const zip = await prepareZip(page)
  await page.evaluate(() => sessionStorage.setItem('zip-defer', 'yes'))
  await zip.getByRole('button', { name: '检查 ZIP', exact: true }).click()
  await expect(page.getByRole('button', { name: '选择目录', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await expect(zip.getByRole('button', { name: '正在检查…', exact: true })).toBeDisabled()
  await expect(page.getByRole('textbox', { name: '筛选安装包', exact: true })).toBeDisabled()
  await expect(page.getByRole('combobox', { name: '安装包查找位置', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await page.evaluate(() => (window as unknown as { finishZip: () => void }).finishZip())
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(zip).not.toContainText('发现安装条目线索')
  await expect(zip.getByRole('button', { name: '重新检查', exact: true })).toBeVisible()
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('pinned common locations set one explicit root without scanning and retain permission failures honestly', async ({
  page,
}) => {
  await install(page)
  const picker = page.getByRole('combobox', { name: '安装包查找位置', exact: true })
  await expect(picker.locator('option')).toHaveCount(13)
  await picker.selectOption('downloads')
  await expect(page.getByRole('heading', { name: '目录已选择', exact: true })).toBeVisible()
  expect(await page.evaluate(() => sessionStorage.getItem('installer-calls'))).toBeNull()
  const toolbar = page.getByRole('banner', { name: '安装包工具栏' })
  await expect(toolbar).toContainText('/Users/demo/Downloads')
  await page.setViewportSize({ width: 960, height: 680 })
  expect((await toolbar.boundingBox())!.height).toBeLessThanOrEqual(64)
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  await page.evaluate(() => sessionStorage.setItem('installer-missing', 'yes'))
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('installer-args')!))).toEqual({
    root: '/Users/demo/Downloads',
  })
  await expect(page.getByRole('alert')).toContainText('目录不存在或无权访问')
  await expect(page.getByRole('article')).toHaveCount(0)
  await picker.selectOption('shared')
  await expect(toolbar).toContainText('/Users/Shared')
  expect(await page.evaluate(() => sessionStorage.getItem('installer-calls'))).toBe('1')
  await expect(page.getByRole('alert')).toHaveCount(0)
})
test('type and name filters preserve scan coverage, clear without rescanning and reset when scope changes', async ({
  page,
}) => {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  const region = page.getByRole('region', { name: '安装包扫描结果' })
  await expect(region.getByRole('article').first()).toHaveAttribute(
    'aria-label',
    '/fixture/downloads/Tool.dmg',
  )
  await region.getByRole('combobox', { name: '安装包类型', exact: true }).selectOption('zip')
  await expect(region).toContainText('显示 1 / 2 项')
  await expect(region).toContainText('客户端保护命中')
  await expect(region).toContainText('大小未知')
  await region.getByRole('textbox', { name: '筛选安装包', exact: true }).fill('missing')
  await expect(region).toContainText('没有符合筛选的文件')
  await expect(region).toContainText('发现 2 个文件线索')
  await region.getByRole('button', { name: '清除筛选', exact: true }).click()
  await expect(region.getByRole('article')).toHaveCount(2)
  expect(await page.evaluate(() => sessionStorage.getItem('installer-calls'))).toBe('1')
  await region.getByRole('textbox', { name: '筛选安装包', exact: true }).fill('ARCHIVE')
  await expect(region.getByRole('article')).toHaveCount(1)
  await page.getByRole('combobox', { name: '安装包查找位置', exact: true }).selectOption('desktop')
  await expect(region.getByRole('article')).toHaveCount(0)
  await expect(region.getByRole('heading', { name: '目录已选择' })).toBeVisible()
})

async function prepareConnection(page: Page) {
  await install(page)
  await page.getByRole('button', { name: '选择目录', exact: true }).click()
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  return page.getByRole('article', { name: '/fixture/downloads/Tool.dmg', exact: true })
}
test('connection checks are explicit, bound to one image, and recheck failure withdraws prior observations', async ({
  page,
}) => {
  const image = await prepareConnection(page)
  expect(await page.evaluate(() => sessionStorage.getItem('connection-args'))).toBeNull()
  await image.getByRole('button', { name: '检查连接', exact: true }).click()
  await expect(image).toContainText('系统报告此路径已连接，请先保留')
  expect(await page.evaluate(() => JSON.parse(sessionStorage.getItem('connection-args')!))).toEqual(
    { root: '/fixture/downloads', path: '/fixture/downloads/Tool.dmg' },
  )
  await expect(image).toContainText('其他清理条件未确认')
  await page.evaluate(() => sessionStorage.setItem('connection-fail', 'yes'))
  await image.getByRole('button', { name: '重新检查连接', exact: true }).click()
  await expect(image.getByRole('alert')).toContainText('先前连接观察已清除')
  await expect(image).not.toContainText('此路径已连接')
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})
test('unlisted paths and uncertain connection lists do not imply unused and rescan removes observations', async ({
  page,
}) => {
  const image = await prepareConnection(page)
  await page.evaluate(() => sessionStorage.setItem('connection-negative', 'yes'))
  await image.getByRole('button', { name: '检查连接', exact: true }).click()
  await expect(image).toContainText('本次连接列表未见此路径；文件占用仍未确认')
  await page.evaluate(() => sessionStorage.setItem('connection-unknown', 'yes'))
  await image.getByRole('button', { name: '重新检查连接', exact: true }).click()
  await expect(image).toContainText('连接状态无法确认：列表格式或路径无法识别')
  await page.getByRole('button', { name: '扫描目录', exact: true }).click()
  await expect(image.getByRole('button', { name: '检查连接', exact: true })).toBeVisible()
  await expect(image).not.toContainText('上次连接检查')
})
test('connection reading locks filters and ZIP checks and ignores late results after leaving', async ({
  page,
}) => {
  const image = await prepareConnection(page)
  await page.evaluate(() => sessionStorage.setItem('connection-defer', 'yes'))
  await image.getByRole('button', { name: '检查连接', exact: true }).click()
  await expect(page.getByRole('button', { name: '扫描目录', exact: true })).toBeDisabled()
  await expect(page.getByRole('combobox', { name: '安装包查找位置', exact: true })).toBeDisabled()
  await expect(page.getByRole('textbox', { name: '筛选安装包', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: '检查 ZIP', exact: true })).toBeDisabled()
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await page.evaluate(() =>
    (window as unknown as { finishConnection: () => void }).finishConnection(),
  )
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(image).not.toContainText('此路径已连接')
  await expect(image.getByRole('button', { name: '重新检查连接', exact: true })).toBeVisible()
  await page
    .getByRole('combobox', { name: '安装包查找位置', exact: true })
    .selectOption('downloads')
  await expect(page.getByRole('article')).toHaveCount(0)
})
