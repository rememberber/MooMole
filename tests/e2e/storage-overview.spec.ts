import { expect, test } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

const volume = (index: number, name: string, available: number | null = 64 * 1024 ** 3) => ({
  id: `${index}1d8c490-1245-414b-8d62-0c6aab303555`,
  kind: index === 1 ? 'startup' : 'mounted',
  name,
  path: index === 1 ? '/' : `/Volumes/${name}`,
  fileSystem: available === null ? null : 'apfs',
  readOnly: available === null ? null : false,
  totalBytes: available === null ? null : 256 * 1024 ** 3,
  availableBytes: available,
  canAnalyze: available !== null,
})

test('overview shows every returned local disk including external drives, without account duplication or system metrics', async ({
  page,
}) => {
  await installTaskMock(page, 'new')
  const volumes = [
    volume(1, ''),
    volume(2, '移动硬盘'),
    volume(3, '备份盘'),
    volume(4, '素材盘'),
    volume(5, '无法读取', null),
    { ...volume(6, ''), kind: 'account', path: '/Users/demo' },
  ]
  await page.addInitScript(
    (volumes) => sessionStorage.setItem('mock-volumes', JSON.stringify(volumes)),
    volumes,
  )
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const overview = page.getByRole('region', { name: '所有磁盘空间' })
  await expect(overview.locator('article')).toHaveCount(5)
  await expect(overview).toContainText('移动硬盘')
  await expect(overview).toContainText('备份盘')
  await expect(overview).toContainText('素材盘')
  await expect(overview).not.toContainText('/Users/demo')
  await expect(overview.locator('article').first()).toContainText('64 GB')
  const unknown = overview.locator('article').last()
  await expect(unknown).toContainText('未知')
  await expect(unknown.getByRole('img')).toHaveAttribute('data-unknown', 'true')
  const main = page.getByRole('main', { name: '概览内容' })
  await expect(main).not.toContainText('CPU 使用率')
  await expect(main).not.toContainText('内存占用')
  await expect(main).not.toContainText('运行时间')
  await expect(main).not.toContainText('Apple Silicon')
  await expect(overview).toContainText('容量不相加')
  await expect(page.getByRole('button', { name: '开始一键清理' })).toBeEnabled()
  await expect(overview).not.toContainText('磁盘空间数据已过期')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({ path: `test-results/overview-disks-${theme}.png`, fullPage: true })
  }
})

test('refresh replaces disconnected disks, exposes incomplete reads and recovers from failures without fake capacities', async ({
  page,
}) => {
  await installTaskMock(page, 'new')
  await page.goto('/')
  const overview = page.getByRole('region', { name: '所有磁盘空间' })
  await expect(overview.locator('article')).toHaveCount(1)
  await page.evaluate(
    (volumes) => {
      sessionStorage.setItem('mock-volumes', JSON.stringify(volumes))
      sessionStorage.setItem('mock-volume-report', JSON.stringify({ complete: false }))
    },
    [volume(1, ''), volume(2, '新接入硬盘')],
  )
  await page.getByRole('button', { name: '刷新磁盘', exact: true }).click()
  await expect(overview.locator('article')).toHaveCount(2)
  await expect(overview).toContainText('新接入硬盘')
  await expect(overview).toContainText('磁盘列表不完整')
  await page.evaluate(() => sessionStorage.setItem('mock-volume-error', 'yes'))
  await page.getByRole('button', { name: '刷新磁盘', exact: true }).click()
  await expect(overview.getByRole('alert')).toContainText('磁盘读取失败')
  await expect(overview.locator('article')).toHaveCount(0)
  await expect(overview).not.toContainText('示例磁盘')
  await page.evaluate(
    (volumes) => {
      sessionStorage.removeItem('mock-volume-error')
      sessionStorage.removeItem('mock-volume-report')
      sessionStorage.setItem('mock-volumes', JSON.stringify(volumes))
    },
    [volume(1, '')],
  )
  await page.getByRole('button', { name: '刷新磁盘', exact: true }).click()
  await expect(overview.locator('article')).toHaveCount(1)
  await expect(overview).not.toContainText('新接入硬盘')
})

test('expired capacity stays explicitly old until refreshed; invalid capacity is rejected', async ({
  page,
}) => {
  await page.clock.install()
  await installTaskMock(page, 'new')
  await page.goto('/')
  const overview = page.getByRole('region', { name: '所有磁盘空间' })
  await expect(overview.locator('article')).toHaveCount(1)
  await page.clock.fastForward(121_000)
  await expect(overview).toContainText('磁盘空间数据已过期')
  await page.evaluate(
    (volumes) => sessionStorage.setItem('mock-volumes', JSON.stringify(volumes)),
    [volume(1, '', 999 * 1024 ** 3)],
  )
  await page.getByRole('button', { name: '刷新磁盘', exact: true }).click()
  await expect(overview.getByRole('alert')).toContainText('磁盘列表无法核对')
  await expect(overview.locator('article')).toHaveCount(0)
})
