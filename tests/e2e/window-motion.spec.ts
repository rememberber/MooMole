import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

async function startAnalysis(page: Page) {
  await installTaskMock(page, 'analyze')
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await expect(page.locator('.workspace-atmosphere')).toHaveAttribute('data-phase', 'idle')
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录' }).fill('/fixture')
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await expect(page.locator('.workspace-atmosphere')).toHaveAttribute('data-phase', 'scanning')
}

test('workspace backdrop follows task state, stays behind content and never blocks input or changes layout', async ({
  page,
}) => {
  await startAnalysis(page)
  const atmosphere = page.locator('.workspace-atmosphere')
  await expect(atmosphere).toHaveAttribute('aria-hidden', 'true')
  await expect(atmosphere).toHaveCSS('opacity', '1')
  await expect(atmosphere).toHaveCSS('pointer-events', 'none')
  await expect(atmosphere).toHaveCSS('position', 'fixed')
  await expect(atmosphere).toHaveCSS('z-index', '-1')
  await expect(atmosphere.locator('.workspace-sweep')).toHaveCSS('animation-play-state', 'running')
  // Inspect a middle frame without sleeping or changing any task facts.
  await atmosphere.locator('.workspace-sweep').evaluate((el) => {
    const animation = el.getAnimations()[0]
    animation.pause()
    animation.currentTime = 4500
  })
  for (const [theme, width] of [
    ['light', 960],
    ['dark', 1200],
  ] as const) {
    await page.setViewportSize({ width, height: 800 })
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    const bounds = (await atmosphere.boundingBox())!
    expect(bounds.x).toBe((await page.locator('.sidebar').boundingBox())!.width)
    expect(bounds.width + bounds.x).toBe(width)
    expect(bounds.y + bounds.height).toBe(800)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.getByRole('textbox', { name: '扫描目录' }).click()
    await expect(page.getByRole('textbox', { name: '扫描目录' })).toBeFocused()
    await page.screenshot({ path: `test-results/window-atmosphere-${theme}.png` })
  }
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await expect(atmosphere).toHaveAttribute('data-phase', 'idle')
  await expect(atmosphere).toHaveCSS('opacity', '0')
  await expect(atmosphere.locator('.workspace-grain')).toHaveCSS('animation-play-state', 'paused')
})

test('window blur and modal opening quiet the background without stopping the task or changing focus return', async ({
  page,
}) => {
  await startAnalysis(page)
  const atmosphere = page.locator('.workspace-atmosphere')
  const sweep = atmosphere.locator('.workspace-sweep')
  const starts = await page.evaluate(() => sessionStorage.getItem('mock-starts'))
  await page.evaluate(() => window.dispatchEvent(new Event('blur')))
  await expect(page.locator('html')).toHaveAttribute('data-window-inactive', 'true')
  await expect(sweep).toHaveCSS('animation-play-state', 'paused')
  await expect(atmosphere).toHaveCSS('opacity', '0')
  await expect(atmosphere).toHaveAttribute('data-phase', 'scanning')
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await expect(sweep).toHaveCSS('animation-play-state', 'running')
  await expect(atmosphere).toHaveCSS('opacity', '1')
  const settings = page.getByRole('button', { name: '设置', exact: true })
  await settings.click()
  const modal = page.getByRole('dialog', { name: '设置' })
  await expect(modal).toBeVisible()
  await expect(modal).toHaveCSS('animation-name', 'dialog-arrive')
  await expect(sweep).toHaveCSS('animation-play-state', 'paused')
  await expect(atmosphere).toHaveCSS('opacity', '0')
  await page.keyboard.press('Escape')
  await expect(modal).toHaveCount(0)
  await expect(settings).toBeFocused()
  await expect(sweep).toHaveCSS('animation-play-state', 'running')
  expect(await page.evaluate(() => sessionStorage.getItem('mock-starts'))).toBe(starts)
})

test('reduced motion and forced colors disable atmosphere and keep modal controls immediate and readable', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await startAnalysis(page)
  await expect(page.locator('.workspace-atmosphere')).toBeHidden()
  await page.getByRole('button', { name: '设置', exact: true }).click()
  const modal = page.getByRole('dialog', { name: '设置' })
  await expect(modal).toHaveCSS('animation-name', 'none')
  expect(await modal.evaluate((el) => getComputedStyle(el, '::backdrop').animationName)).toBe(
    'none',
  )
  await expect(modal).toHaveCSS('opacity', '1')
  await page.keyboard.press('Escape')
  await page.emulateMedia({ reducedMotion: 'no-preference', forcedColors: 'active' })
  await expect(page.locator('.workspace-atmosphere')).toBeHidden()
  await expect(page.getByRole('button', { name: '取消', exact: true })).toBeVisible()
})
