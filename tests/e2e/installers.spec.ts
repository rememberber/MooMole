import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { installersReport, installerZipInspection } from './fixtures/installers'
async function install(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.addInitScript(
    ({ fixture, zip }) => {
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
    { fixture: installersReport(), zip: installerZipInspection() },
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

  await expect(region).toContainText('挂载与占用尚未检查，清理未开放')
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
  await page.getByRole('button', { name: '工程产物', exact: true }).click()
  await page.evaluate(() => (window as unknown as { finishZip: () => void }).finishZip())
  await page.getByRole('button', { name: '安装包', exact: true }).click()
  await expect(zip).not.toContainText('发现安装条目线索')
  await expect(zip.getByRole('button', { name: '重新检查', exact: true })).toBeVisible()
  await page.setViewportSize({ width: 960, height: 680 })
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})
