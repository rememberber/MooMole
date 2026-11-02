import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'
async function start(page: Page, mode = 'normal') {
  await page.goto('/')
  await page.evaluate(
    async (mode) => (await import('/tests/e2e/fixtures/app-update-discovery.tsx')).mount(mode),
    mode,
  )
  const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
  await rows.first().getByRole('button', { name: '检查更新…', exact: true }).click()
  return page.getByRole('dialog', { name: '检查此应用更新' })
}
async function calls(page: Page) {
  return JSON.parse((await page.locator('#app-discovery-fixture').getAttribute('data-calls'))!) as {
    command: string
    args: Record<string, unknown>
  }[]
}
test('verified download uses an exact candidate and chosen directory without installing or opening it', async ({
  page,
}) => {
  await page.setViewportSize({ width: 960, height: 780 })
  const dialog = await start(page, 'download')
  await dialog.getByRole('button', { name: '向此地址检查', exact: true }).click()
  await dialog.getByRole('button', { name: '下载并校验更新包…', exact: true }).click()
  await expect(dialog.getByRole('region', { name: '已校验的更新包' })).toContainText(
    '签名与已安装应用声明的公钥匹配',
  )
  const invoked = await calls(page)
  const downloads = invoked.filter((c) => c.command === 'download_app_update_candidate')
  expect(downloads).toHaveLength(1)
  expect(downloads[0].args).toMatchObject({
    probeId: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    candidateId: '821324f0-d589-4873-b42a-8061782d839e',
    directory: '/Users/demo/Downloads',
    confirmed: true,
  })
  expect(downloads[0].args).not.toHaveProperty('url')
  expect(
    invoked.some(
      (c) =>
        c.command === 'execute_application_update' || c.command === 'open_app_update_candidate',
    ),
  ).toBe(false)
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await dialog.getByRole('region', { name: '已校验的更新包' }).scrollIntoViewIfNeeded()
  await page.screenshot({ path: 'test-results/app-verified-download.png' })
})
for (const lang of ['en', 'ja'])
  test(`${lang} signed download explains verification and separate installation`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/app-update-discovery.tsx')).mount(mode),
      lang === 'en' ? 'downloadEn' : 'downloadJa',
    )
    await page
      .getByRole('button', {
        name: lang === 'en' ? 'Check for updates…' : '更新を確認…',
        exact: true,
      })
      .first()
      .click()
    const dialog = page.getByRole('dialog')
    await dialog
      .getByRole('button', {
        name: lang === 'en' ? 'Check this address' : 'このアドレスを確認',
        exact: true,
      })
      .click()
    await dialog
      .getByRole('button', {
        name: lang === 'en' ? 'Download and verify package…' : 'パッケージをダウンロードして検証…',
        exact: true,
      })
      .click()
    const delivered = dialog.getByRole('region', {
      name: lang === 'en' ? 'Verified update package' : '検証済みの更新パッケージ',
    })
    await expect(delivered).toBeVisible()
    await expect(delivered).toContainText(lang === 'en' ? 'signature matches' : '公開鍵と一致')
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await delivered.scrollIntoViewIfNeeded()
    await page.screenshot({ path: info.outputPath(`verified-download-${lang}.png`) })
  })
for (const mode of [
  'downloadBadSignature',
  'downloadMismatch',
  'downloadDialogCancel',
  'downloadCancel',
])
  test(`download refuses or stops ${mode} without retries or installation`, async ({ page }) => {
    const dialog = await start(page, mode)
    await dialog.getByRole('button', { name: '向此地址检查', exact: true }).click()
    await dialog.getByRole('button', { name: '下载并校验更新包…', exact: true }).click()
    if (mode === 'downloadCancel')
      await dialog.getByRole('button', { name: '停止更新来源检查', exact: true }).click()
    if (mode !== 'downloadDialogCancel') await expect(dialog.getByRole('alert')).toBeVisible()
    await expect(dialog.getByRole('region', { name: '已校验的更新包' })).toHaveCount(0)
    const invoked = await calls(page)
    expect(invoked.filter((c) => c.command === 'download_app_update_candidate')).toHaveLength(
      mode === 'downloadDialogCancel' ? 0 : 1,
    )
    expect(invoked.some((c) => c.command === 'execute_application_update')).toBe(false)
  })
test('the actual app list routes only the retained ordinary copy into explicit discovery', async ({
  page,
}) => {
  await installTaskMock(page, 'apps', undefined, applications)
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const rows = page.getByRole('region', { name: 'Mole 应用清单' }).getByRole('article')
  await expect(rows).toHaveCount(5)
  await expect(rows.nth(1).getByRole('button', { name: '检查更新…', exact: true })).toBeDisabled()
  await page.evaluate(() => {
    const native = (
      window as unknown as {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown>
        }
      }
    ).__TAURI_INTERNALS__
    const original = native.invoke
    let probe: Record<string, unknown>
    native.invoke = async (command, args = {}) => {
      if (command === 'prepare_app_update_discovery') {
        const now = Date.now()
        probe = {
          id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
          taskId: args.taskId,
          index: args.index,
          path: '/Applications/Example.app',
          bundleId: 'org.example.same',
          currentVersion: '1.0',
          currentBuild: '10',
          feedUrl: 'https://updates.example.org/feed.xml',
          state: 'ready',
          reason: '更新地址来自此应用的 Info.plist，尚未访问网络或验证发布者。',
          createdAt: now,
          expiresAt: now + 300000,
        }
        sessionStorage.setItem('discovery-prepare', JSON.stringify(args))
        return probe
      }
      if (command === 'check_app_update_discovery') {
        sessionStorage.setItem('discovery-check', JSON.stringify(args))
        return {
          probeId: probe.id,
          requestId: args.requestId,
          checkedAt: Date.now(),
          expiresAt: probe.expiresAt,
          state: 'noNewerInFeed',
          complete: true,
          inspected: 1,
          skipped: 1,
          releases: [],
        }
      }
      if (command === 'discard_app_update_discovery') return null
      return original(command, args)
    }
  })
  await rows.first().getByRole('button', { name: '检查更新…', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '检查此应用更新' })
  await expect(dialog).toContainText('/Applications/Example.app')
  await expect(dialog.getByRole('button', { name: '向此地址检查' })).toBeEnabled()
  expect(await page.evaluate(() => sessionStorage.getItem('discovery-check'))).toBeNull()
  await dialog.getByRole('button', { name: '向此地址检查' }).click()
  await expect(dialog).toContainText('此来源未报告本机适用的较新稳定版本')
  const sent = await page.evaluate(() => JSON.parse(sessionStorage.getItem('discovery-prepare')!))
  expect(sent.index).toBe(0)
  expect(sent).not.toHaveProperty('path')
  expect(sent).not.toHaveProperty('url')
})
test('local declaration requires explicit network check, exact browser handoff and separate signed-package confirmation', async ({
  page,
}, info) => {
  const dialog = await start(page)
  await expect(dialog.getByRole('button', { name: '向此地址检查' })).toBeEnabled()
  expect((await calls(page)).map((c) => c.command)).toEqual(['prepare_app_update_discovery'])
  await dialog.getByRole('button', { name: '向此地址检查' }).click()
  await expect(dialog).toContainText('此来源报告了较新版本')
  await expect(dialog).toContainText('此版本需要另行核对授权或升级要求。')
  await page.setViewportSize({ width: 960, height: 780 })
  expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
  await page.screenshot({ path: info.outputPath('app-update-discovery.png') })
  await dialog.getByRole('button', { name: '在浏览器打开下载地址' }).click()
  await expect(dialog).toContainText('下载是否完成请在浏览器确认')
  expect(
    (await calls(page)).find((c) => c.command === 'open_app_update_candidate')?.args,
  ).toMatchObject({
    probeId: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    candidateId: '821324f0-d589-4873-b42a-8061782d839e',
  })
  expect(
    (await calls(page)).filter((c) => c.command === 'execute_application_update'),
  ).toHaveLength(0)
  await dialog.getByRole('button', { name: '使用已下载应用包…' }).click()
  const manual = page.getByRole('dialog', { name: '更新此应用' })
  await manual.getByRole('button', { name: '选择新版应用…' }).click()
  await expect(manual.getByRole('button', { name: '确认替换并保留旧版' })).toBeDisabled()
  await manual.getByRole('checkbox').check()
  await manual.getByRole('button', { name: '确认替换并保留旧版' }).click()
  await expect(manual).toContainText('已替换，旧版已保留')
  const log = await calls(page)
  expect(log.find((c) => c.command === 'check_app_update_discovery')?.args).toMatchObject({
    id: '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
    confirmed: true,
  })
  expect(log.find((c) => c.command === 'open_app_update_candidate')?.args).not.toHaveProperty('url')
  expect(log.find((c) => c.command === 'prepare_application_update')?.args).toMatchObject({
    taskId: '853bf826-4ae1-4695-bc66-85d49e1957b0',
    index: 0,
    replacement: '/Users/demo/Downloads/Example.app',
  })
  expect(log.filter((c) => c.command === 'execute_application_update')).toHaveLength(1)
})
for (const mode of ['unknown', 'noNewer'])
  test(`${mode} results do not claim every app is current`, async ({ page }) => {
    const dialog = await start(page, mode)
    await dialog.getByRole('button', { name: '向此地址检查' }).click()
    await expect(dialog).toContainText(
      mode === 'unknown' ? '来源信息不足，版本状态未知' : '此来源未报告本机适用的较新稳定版本',
    )
    await expect(dialog.getByRole('button', { name: '在浏览器打开下载地址' })).toHaveCount(0)
  })
for (const mode of ['expired', 'mismatch', 'unsupported'])
  test(`${mode} cannot check or open an update`, async ({ page }) => {
    const dialog = await start(page, mode)
    if (mode === 'expired')
      await expect(dialog.getByRole('button', { name: '向此地址检查' })).toBeDisabled()
    else await expect(dialog.getByRole('button', { name: '向此地址检查' })).toHaveCount(0)
    if (mode === 'mismatch') await expect(dialog.getByRole('alert')).toContainText('无法核对')
    await expect(dialog.getByRole('button', { name: '使用已下载应用包…' })).toBeEnabled()
    expect(
      (await calls(page)).filter((c) => c.command === 'check_app_update_discovery'),
    ).toHaveLength(0)
  })
test('stop only cancels its own request and closing withdraws the source', async ({ page }) => {
  const dialog = await start(page, 'cancel')
  await dialog.getByRole('button', { name: '向此地址检查' }).click()
  await dialog.getByRole('button', { name: '停止更新来源检查' }).click()
  await expect(dialog.getByRole('alert')).toContainText('已停止')
  await dialog.getByRole('button', { name: '关闭更新检查' }).click()
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'check_app_update_discovery')?.args.requestId,
  )
  expect(log.find((c) => c.command === 'discard_app_update_discovery')?.args.id).toBe(
    '7f786cf6-756c-49a7-b5a7-397fbb7cd15d',
  )
})
test('a late local probe after unmount is cancelled and discarded, never used for network access', async ({
  page,
}) => {
  const dialog = await start(page, 'late')
  await page.evaluate(async () => {
    const { invoke } = (
      window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string) => Promise<void> } }
    ).__TAURI_INTERNALS__
    await invoke('fixture_unmount')
  })
  await expect(dialog).toHaveCount(0)
  await page.evaluate(async () => {
    await (
      window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string) => Promise<void> } }
    ).__TAURI_INTERNALS__.invoke('fixture_late')
  })
  await expect
    .poll(async () => (await calls(page)).some((c) => c.command === 'discard_app_update_discovery'))
    .toBe(true)
  const log = await calls(page)
  expect(log.find((c) => c.command === 'cancel_task')?.args.taskId).toBe(
    log.find((c) => c.command === 'prepare_app_update_discovery')?.args.requestId,
  )
  expect(log.filter((c) => c.command === 'check_app_update_discovery')).toHaveLength(0)
})
for (const lang of ['en', 'ja'])
  test(`${lang} discovery explains source limits and fits the dialog`, async ({ page }, info) => {
    await page.goto('/')
    await page.evaluate(
      async (mode) => (await import('/tests/e2e/fixtures/app-update-discovery.tsx')).mount(mode),
      lang,
    )
    await page
      .getByRole('button', {
        name: lang === 'en' ? 'Check for updates…' : '更新を確認…',
        exact: true,
      })
      .first()
      .click()
    const dialog = page.getByRole('dialog')
    await dialog
      .getByRole('button', {
        name: lang === 'en' ? 'Check this address' : 'このアドレスを確認',
        exact: true,
      })
      .click()
    await expect(dialog).toContainText(
      lang === 'en'
        ? 'This source reports a newer version'
        : 'この更新元から新しいバージョンの報告があります',
    )
    await page.setViewportSize({ width: 960, height: 780 })
    expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true)
    await page.screenshot({ path: info.outputPath(`app-update-${lang}.png`) })
  })
