import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'

async function mount(page: Page) {
  await page.goto('/')
  await page.evaluate(async () => (await import('/tests/e2e/fixtures/task-motion.tsx')).mount())
  await page.getByRole('button', { name: '查看移入废纸篓范围' }).click()
}
async function progress(page: Page, movedCount: number, state = 'running', uncertainCount = 0) {
  await page.evaluate(
    (detail) => window.dispatchEvent(new CustomEvent('motion-progress', { detail })),
    { movedCount, state, uncertainCount },
  )
}

test('dust follows committed move evidence, disappears on full completion and never predicts progress', async ({
  page,
}) => {
  await mount(page)
  const dialog = page.getByRole('dialog')
  const cloud = dialog.locator('.dust-cloud')
  const particles = cloud.locator('[data-remaining="true"]')
  await expect(particles).toHaveCount(32)
  await dialog.getByRole('checkbox').check()
  await dialog.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await expect(cloud).toHaveAttribute('data-phase', 'working')
  await expect(dialog.locator('.motion-text[data-active="true"]')).toBeVisible()
  await page.clock.install()
  await page.clock.fastForward(5000)
  await expect(particles).toHaveCount(32)
  await progress(page, 1)
  await expect(particles).toHaveCount(16)
  await progress(page, 2, 'succeeded')
  await expect(particles).toHaveCount(0)
  await expect(cloud).toHaveAttribute('data-empty', 'true')
  await expect(dialog).toContainText('已移入 2 项 · 其余 0 项未确认移入')
  await expect(
    dialog.locator('[data-scanning="true"], .motion-text[data-active="true"]'),
  ).toHaveCount(0)
})

test('interrupted and unknown results keep remaining dust and stop scanning', async ({ page }) => {
  await mount(page)
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  await progress(page, 1, 'interrupted', 1)
  const dialog = page.getByRole('dialog', { name: '清理结果' })
  await expect(dialog.locator('.dust-particle[data-remaining="true"]')).toHaveCount(16)
  await expect(dialog.locator('.dust-cloud')).toHaveAttribute('data-phase', 'settled')
  await expect(dialog).toContainText('本批结果需要核实')
  await expect(dialog.locator('[data-scanning="true"]')).toHaveCount(0)
})

test('motion respects reduced motion, high contrast and readable text in both themes', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await mount(page)
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: '移入废纸篓', exact: true }).click()
  const dialog = page.getByRole('dialog')
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    const text = dialog.locator('.motion-text[data-active="true"]')
    expect(await text.evaluate((el) => getComputedStyle(el).animationName)).toBe('none')
    expect(await text.evaluate((el) => getComputedStyle(el).webkitTextFillColor)).not.toBe(
      'rgba(0, 0, 0, 0)',
    )
    expect(
      await dialog
        .locator('.dust-particle i')
        .first()
        .evaluate((el) => getComputedStyle(el).animationName),
    ).toBe('none')
    expect(
      await dialog
        .locator('.motion-scan')
        .evaluate((el) => getComputedStyle(el, '::after').content),
    ).toBe('none')
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({ path: `test-results/motion-${theme}-reduced.png` })
  }
  await page.emulateMedia({ reducedMotion: 'no-preference', forcedColors: 'active' })
  await expect(dialog.locator('.dust-cloud')).toBeHidden()
  expect(
    await dialog
      .locator('.motion-text[data-active="true"]')
      .evaluate((el) => getComputedStyle(el).webkitTextFillColor),
  ).not.toBe('rgba(0, 0, 0, 0)')
})

test('offscreen clouds pause and candidate summaries remain compact across themes', async ({
  page,
}) => {
  await page.goto('/')
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '清理', exact: true })
    .click()
  const cloud = page.locator('.clean-review .dust-cloud')
  await expect(cloud).toHaveAttribute('data-visible', 'true')
  await expect(cloud).toHaveAttribute('aria-hidden', 'true')
  expect(
    await cloud
      .locator('i')
      .first()
      .evaluate((el) => getComputedStyle(el).animationPlayState),
  ).toBe('running')
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: true })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  expect(
    await cloud
      .locator('i')
      .first()
      .evaluate((el) => getComputedStyle(el).animationPlayState),
  ).toBe('paused')
  await page.evaluate(() => {
    Object.defineProperty(document, 'hidden', { configurable: true, value: false })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  for (const theme of ['light', 'dark']) {
    await page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme
    }, theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/motion-candidates-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => scrollTo(0, document.body.scrollHeight))
  await expect(cloud).toHaveAttribute('data-visible', 'false')
  expect(
    await cloud
      .locator('i')
      .first()
      .evaluate((el) => getComputedStyle(el).animationPlayState),
  ).toBe('paused')
})

test('running global task feedback keeps fixed positioning and never squeezes the workspace', async ({
  page,
}) => {
  await installTaskMock(page, 'analyze')
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '空间分析', exact: true }).click()
  await page.getByRole('textbox', { name: '扫描目录' }).fill('/fixture')
  await page.getByRole('button', { name: '开始分析', exact: true }).click()
  await page
    .getByRole('navigation', { name: '主导航' })
    .getByRole('button', { name: '清理', exact: true })
    .click()
  const toast = page.locator('.task-toast')
  await expect(toast).toBeVisible()
  for (const width of [960, 1200]) {
    await page.setViewportSize({ width, height: 800 })
    expect(await toast.evaluate((el) => getComputedStyle(el).position)).toBe('fixed')
    expect((await page.locator('.workspace').boundingBox())!.width).toBeGreaterThan(width - 180)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
  }
  await toast.getByRole('button', { name: '取消', exact: true }).click()
  await expect(toast).toHaveCount(0)
})
