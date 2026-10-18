import { expect, test, type Page } from '@playwright/test'
import { installTaskMock } from './fixtures/native-task'
import { applications } from './fixtures/application-library'

async function setup(page: Page) {
  await installTaskMock(page, 'apps', undefined, applications)
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
      if (command === 'plugin:dialog|open') return '/Applications'
      if (command === 'save_settings') {
        sessionStorage.setItem('inspection-settings', JSON.stringify(args?.settings))
        return args?.settings
      }
      if (command !== 'inspect_application') return previous(command, args)
      const calls = JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]')
      calls.push(args)
      sessionStorage.setItem('inspection-calls', JSON.stringify(calls))
      const mode = sessionStorage.getItem('inspection-mode')
      const saved = JSON.parse(sessionStorage.getItem('inspection-settings') ?? 'null')
      const protection = {
        reasons:
          saved?.protectedPaths.includes('/Applications') ||
          mode === 'protected' ||
          mode === 'officialProtected' ||
          mode === 'backgroundProtected' ||
          mode === 'moleUnknownProtected'
            ? ['clientDirectory']
            : mode === 'system'
              ? ['systemLocation']
              : mode === 'self'
                ? ['currentApplication']
                : [],
        matchedPaths:
          saved?.protectedPaths.includes('/Applications') ||
          mode === 'protected' ||
          mode === 'officialProtected' ||
          mode === 'backgroundProtected' ||
          mode === 'moleUnknownProtected'
            ? ['/Applications']
            : [],
      }
      if (mode === 'pending')
        while (!sessionStorage.getItem('inspection-release'))
          await new Promise((resolve) => setTimeout(resolve, 20))
      sessionStorage.setItem(
        'inspection-completed',
        String(Number(sessionStorage.getItem('inspection-completed') ?? 0) + 1),
      )
      if (mode === 'error') throw new Error('应用状态读取超时，结果无法确认；请稍后重试。')
      return {
        protocol: mode === 'future' ? 12 : 11,
        checkedAt: Date.now(),
        bundleId: mode === 'mismatch' ? 'org.example.changed' : 'org.example.same',
        version: mode === 'pending' ? 'old version' : '2.3',
        build: '42',
        identity: mode === 'mismatch' ? 'different' : 'matches',
        protection,
        installations: {
          version: mode === 'copiesFuture' ? 6 : 5,
          source: 'moleListRegistryRootsVolumesAndReceipts',
          roots: mode?.startsWith('roots')
            ? {
                code: 'partial',
                issue:
                  mode === 'rootsChanged'
                    ? 'changed'
                    : mode === 'rootsLimit'
                      ? 'limitReached'
                      : 'unavailable',
                rootCount: mode === 'rootsLimit' ? 3 : 9,
                entries: mode === 'rootsLimit' ? 2048 : 200,
                candidates: 80,
                firstUncertainPath: '/Applications',
              }
            : mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  rootCount: 0,
                  entries: 0,
                  candidates: 0,
                  firstUncertainPath: null,
                }
              : {
                  code: 'observed',
                  issue: null,
                  rootCount: 9,
                  entries: 40,
                  candidates: 4,
                  firstUncertainPath: null,
                },
          volumes:
            mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  rootCount: 0,
                  entries: 0,
                  candidates: 0,
                  firstUncertainPath: null,
                }
              : mode?.startsWith('volumes')
                ? {
                    code: 'partial',
                    issue:
                      mode === 'volumesChanged'
                        ? 'changed'
                        : mode === 'volumesLimit'
                          ? 'limitReached'
                          : 'unsupportedVolume',
                    rootCount: 2,
                    entries: 30,
                    candidates: 3,
                    firstUncertainPath: '/Volumes/Shared Disk',
                  }
                : {
                    code: 'observed',
                    issue: null,
                    rootCount: 2,
                    entries: 10,
                    candidates: 2,
                    firstUncertainPath: null,
                  },
          receipts:
            mode === 'mismatch'
              ? {
                  code: 'notChecked',
                  issue: null,
                  packageCount: 0,
                  checkedCount: 0,
                  entries: 0,
                  candidates: 0,
                }
              : {
                  code: mode?.startsWith('receipts') ? 'partial' : 'observed',
                  issue:
                    mode === 'receiptsChanged'
                      ? 'changed'
                      : mode === 'receiptsLimit'
                        ? 'limitReached'
                        : mode === 'receiptsUnsupported'
                          ? 'unsupportedReceipt'
                          : mode === 'receiptsFailed'
                            ? 'unavailable'
                            : null,
                  packageCount: 20,
                  checkedCount: 2,
                  entries: 40,
                  candidates: 2,
                },
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch'
              ? 'unknown'
              : mode === 'copiesPartial' ||
                  mode === 'copiesChanged' ||
                  mode?.startsWith('roots') ||
                  mode?.startsWith('volumes') ||
                  mode?.startsWith('receipts')
                ? 'partial'
                : 'observed',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode === 'copiesPartial'
                ? 'candidateUnavailable'
                : mode === 'copiesChanged'
                  ? 'candidatesChanged'
                  : mode?.startsWith('roots')
                    ? 'rootsIncomplete'
                    : mode?.startsWith('volumes')
                      ? 'volumesIncomplete'
                      : mode?.startsWith('receipts')
                        ? 'receiptsIncomplete'
                        : null,
          copies: ['copiesFound', 'copiesPartial', 'sharedRelated', 'sharedChannel'].includes(
            mode ?? '',
          )
            ? [
                {
                  path: '/Applications/Example Beta.app',
                  bundleId:
                    mode === 'sharedRelated'
                      ? 'org.example.same.share'
                      : mode === 'sharedChannel'
                        ? 'org.other.app'
                        : 'ORG.EXAMPLE.SAME',
                  relation:
                    mode === 'sharedRelated'
                      ? 'relatedIdentifier'
                      : mode === 'sharedChannel'
                        ? 'channelName'
                        : 'sameIdentifier',
                  version: '3.0',
                  build: '70',
                },
              ]
            : [],
        },
        associations: {
          version: mode === 'associationFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode?.startsWith('association') && mode !== 'associationFuture'
              ? 'observed'
              : 'unknown',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('association') && mode !== 'associationFuture'
                ? null
                : 'homeUnavailable',
          entries:
            mode?.startsWith('association') && mode !== 'associationFuture'
              ? [
                  ['support', 'Library/Application Support', ''],
                  ['cache', 'Library/Caches', ''],
                  ['logs', 'Library/Logs', ''],
                  ['preferences', 'Library/Preferences', '.plist'],
                  ['savedState', 'Library/Saved Application State', '.savedState'],
                  ['container', 'Library/Containers', ''],
                  ['webKit', 'Library/WebKit', ''],
                  ['httpStorage', 'Library/HTTPStorages', ''],
                ].map(([kind, root, suffix], index) => ({
                  kind,
                  relativePath: `${root}/org.example.same${suffix}`,
                  status:
                    mode === 'associationChanged'
                      ? 'unknown'
                      : index === 0
                        ? 'present'
                        : index === 1
                          ? 'unknown'
                          : 'notFound',
                  issue:
                    mode === 'associationChanged' ? 'changed' : index === 1 ? 'unsupported' : null,
                }))
              : [],
        },
        background: {
          version: mode === 'backgroundFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' ||
            mode?.startsWith('moleUnknown') ||
            mode === 'backgroundUnknown' ||
            mode === 'backgroundMarkerUnknown' ||
            mode === 'backgroundChanged' ||
            mode === 'backgroundProtected'
              ? 'unknown'
              : mode === 'backgroundRoot'
                ? 'directRoot'
                : mode === 'backgroundRestricted'
                  ? 'restricted'
                  : 'notRequired',
          marker:
            mode === 'backgroundMarkerUnknown'
              ? 'unknown'
              : mode?.startsWith('background') && mode !== 'backgroundFuture'
                ? 'enabled'
                : 'absent',
          rootKind: mode === 'backgroundRoot' ? 'externalApplications' : null,
          reason: mode === 'backgroundRestricted' ? 'outsideRoots' : null,
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : mode === 'backgroundUnknown' || mode === 'backgroundProtected'
                  ? 'rootsUnavailable'
                  : mode === 'backgroundMarkerUnknown'
                    ? 'markerUnknown'
                    : mode === 'backgroundChanged'
                      ? 'rootsChanged'
                      : null,
        },
        officialUninstaller: {
          version: mode === 'officialFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' || mode?.startsWith('moleUnknown') || mode === 'officialMissingName'
              ? 'unknown'
              : mode === 'official' || mode === 'officialProtected'
                ? 'officialRequired'
                : 'notMatched',
          vendor: mode === 'official' || mode === 'officialProtected' ? 'ESET' : null,
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : mode === 'officialMissingName'
                  ? 'nameUnavailable'
                  : null,
        },
        moleProtection: {
          version: mode === 'moleFuture' ? 2 : 1,
          revision: 'ba3a0f57f78e4c22818fda701219cc3dd4d6953f',
          code:
            mode === 'mismatch' || mode?.startsWith('moleUnknown')
              ? 'unknown'
              : mode === 'moleProtected'
                ? 'protected'
                : 'notMatched',
          issue:
            mode === 'mismatch'
              ? 'identityChanged'
              : mode?.startsWith('moleUnknown')
                ? 'rulesUnavailable'
                : null,
        },
        running:
          mode === 'mismatch'
            ? {
                code: 'unknown',
                pids: [],
                otherCopies: 0,
                reason: '本地应用标识与 Mole 清单不同，请重新读取列表。',
              }
            : mode === 'other'
              ? {
                  code: 'notObserved',
                  pids: [],
                  otherCopies: 1,
                  reason: '本次未发现此安装位置的匹配主应用；辅助进程、后台服务和文件占用未检查。',
                }
              : {
                  code: 'observed',
                  pids: [123],
                  otherCopies: 0,
                  reason: '本次发现此安装位置的主应用运行；未检查辅助进程或后台服务。',
                },
      }
    }
  })
  await page.goto('/')
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(page.getByText('系统快照进行中', { exact: true })).toHaveCount(0)
  await page.getByRole('button', { name: '应用列表', exact: true }).click()
  await page.getByRole('button', { name: '读取应用', exact: true }).click()
  await expect(page.getByText('应用列表进行中', { exact: true })).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  const library = page.getByRole('region', { name: 'Mole 应用清单' })
  await expect(library.getByRole('article')).toHaveCount(5)
  const row = library.getByRole('article', {
    name: 'Example · /Applications/Example.app',
    exact: true,
  })
  return { library, row, inspection: row.getByRole('region', { name: '应用状态检查' }) }
}

test('opening details checks only the chosen task entry and distinguishes running, other copies and unknown identity', async ({
  page,
}) => {
  const { library, row, inspection } = await setup(page)
  expect(await page.evaluate(() => sessionStorage.getItem('inspection-calls'))).toBeNull()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await expect(inspection).toContainText('2.3 · 构建 42')
  await expect(inspection).toContainText('未验证签名')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('完整安装副本、共享数据及个人数据归属仍待核实')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(Object.keys(calls[0]).sort()).toEqual(['index', 'taskId'])
  expect(calls[0].index).toBe(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await row.getByText('位置与来源', { exact: true }).click()
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await expect(inspection).toContainText('另有 1 个同标识安装位置')
  await expect(inspection).toContainText('不代表可卸载')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'mismatch'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toHaveText('运行状态无法确认')
  await expect(inspection).toContainText('本地应用标识与 Mole 清单不同')
  await expect(inspection).not.toContainText('主应用 PID')
  await expect(library.getByRole('button', { name: /退出应用|卸载|删除/ })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-inspection-${theme}.png`,
      animations: 'disabled',
      fullPage: true,
    })
  }
})

test('a refreshed list discards a delayed observation from its old task', async ({ page }) => {
  const { library, row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection.getByRole('button', { name: '检查中…' })).toBeDisabled()
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await page.getByRole('button', { name: '刷新列表', exact: true }).click()
  await expect(library.getByText('正在重新读取，当前显示上次结果。')).toBeVisible()
  await page.evaluate(() => sessionStorage.setItem('mock-done', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('inspection-completed')))
    .toBe('2')
  await expect(inspection).not.toContainText('old version')
  await expect(inspection.getByRole('status')).toHaveText('未发现匹配主应用')
  const calls = await page.evaluate(() =>
    JSON.parse(sessionStorage.getItem('inspection-calls') ?? '[]'),
  )
  expect(calls).toHaveLength(2)
  expect(calls[0].taskId).not.toBe(calls[1].taskId)
})

test('background-only restrictions keep root observations, unknowns and client protection separate', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('未标记为后台专用应用')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundRoot'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('后台专用应用，位于 Mole 搜索目录（外接卷 Applications）')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('不代表可卸载')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundRestricted'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留 · 当前仅供查看')
  await expect(inspection).toContainText('此后台应用受 Mole 目录范围限制')
  await expect(inspection).toContainText('请保留并通过所属应用处理')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    await page.screenshot({
      path: `test-results/application-background-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  for (const [mode, reason] of [
    ['backgroundUnknown', '应用目录、搜索位置或访问状态无法确认'],
    ['backgroundChanged', '搜索目录或访问状态在检查期间发生变化'],
    ['backgroundMarkerUnknown', '后台专用标记格式无法识别'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(inspection).toContainText('后台应用条件无法确认')
    await expect(inspection).toContainText(reason)
    await expect(inspection).toContainText('归属待确认')
    await expect(inspection).not.toContainText('Mole 要求保留')
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('后台应用条件无法确认')
  await expect(inspection).toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'backgroundFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('official uninstaller hints explain the next step and preserve protection and unknown boundaries', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('本次未命中官方卸载器限制')
  await expect(inspection).toContainText('归属待确认')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'official'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('请使用官方卸载器 · 当前仅供查看')
  await expect(inspection).toContainText('Mole 要求使用 ESET 官方卸载器')
  await expect(inspection).toContainText('组织管理的应用请联系管理员')
  await expect(inspection).not.toContainText('归属待确认')
  await inspection.getByText('规则来源', { exact: true }).click()
  await expect(inspection).toContainText('official_uninstaller_vendor')
  await expect(inspection).toContainText('厂商身份未认证')
  await inspection.getByText('规则来源', { exact: true }).click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-official-uninstaller-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).toContainText('Mole 要求使用 ESET 官方卸载器')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialMissingName'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('官方卸载器要求无法确认')
  await expect(inspection).toContainText('Mole 清单名称缺失或无法识别，请重新读取列表')
  await expect(inspection).not.toContainText('ESET')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknownProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('官方卸载器要求无法确认')
  await expect(
    inspection.getByText('内置规则资源未通过校验或无法读取，请检查引擎状态后重试。', {
      exact: true,
    }),
  ).toHaveCount(1)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'officialFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('Mole protections, no-match and unavailable rules remain distinct while client guards survive', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('本次未命中系统组件禁用规则')
  await inspection.getByText('规则来源', { exact: true }).click()
  await expect(inspection.getByText(/should_protect_from_uninstall/)).toBeVisible()
  await expect(inspection).toContainText('ba3a0f57f78e')
  await inspection.getByText('规则来源', { exact: true }).click()
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 要求保留 · 当前仅供查看')
  await expect(inspection).toContainText('命中 Mole 系统组件保护')
  await expect(inspection).not.toContainText('归属待确认')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-mole-rules-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknownProtected'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection).toContainText('Mole 系统组件规则无法确认')
  await expect(inspection).toContainText('内置规则资源未通过校验或无法读取')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).not.toContainText('命中 Mole 系统组件保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'moleUnknown'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).toContainText('Mole 系统组件规则无法确认')
})

test('guards explain system location, current client and protected directories without claiming uninstall eligibility', async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  for (const [mode, reason] of [
    ['system', '位于系统目录'],
    ['self', '当前客户端或同标识应用（未验证签名）'],
    ['protected', '与已保存保护目录重叠'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(inspection).toContainText('已命中客户端保护 · 当前仅供查看')
    await expect(inspection).toContainText(reason)
    await expect(inspection).not.toContainText('归属待确认')
    await expect(inspection).toContainText('共享数据及个人数据归属仍待核实')
  }
  await expect(inspection).toContainText('/Applications')
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await page.screenshot({
      path: `test-results/application-protection-${theme}.png`,
      fullPage: true,
      animations: 'disabled',
    })
  }
})

test('saved protection changes invalidate an in-flight observation and its delayed result', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await page.getByRole('button', { name: '设置', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: '设置', exact: true })
  await dialog.getByRole('button', { name: '保护列表', exact: true }).click()
  await dialog.getByRole('button', { name: '添加目录', exact: true }).click()
  await expect(dialog.getByText('/Applications', { exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).toContainText('/Applications')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect
    .poll(() => page.evaluate(() => sessionStorage.getItem('inspection-completed')))
    .toBe('2')
  await expect(inspection).toContainText('已命中客户端保护')
  await expect(inspection).not.toContainText('old version')
  await page.getByRole('button', { name: '设置', exact: true }).click()
  await dialog.getByRole('button', { name: '保护列表', exact: true }).click()
  await dialog.getByRole('button', { name: '移除保护 /Applications', exact: true }).click()
  await page.keyboard.press('Escape')
  await expect(inspection).toContainText('归属待确认')
  await expect(inspection).not.toContainText('已命中客户端保护')
})

test('read errors and unrecognized protocol stay unknown and retry without keeping a positive result', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'protected'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await expect(inspection).toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'error'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('读取超时')
  await expect(inspection.getByRole('status')).toHaveText('应用状态无法确认')
  await expect(inspection).not.toContainText('主应用 PID')
  await expect(inspection).not.toContainText('已命中客户端保护')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'future'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await page.evaluate(() => sessionStorage.removeItem('inspection-mode'))
  await inspection.getByRole('button', { name: '重试检查' }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})

test('an open observation becomes visibly older and rechecking withdraws the old conclusion while pending', async ({
  page,
}) => {
  await page.clock.install()
  const { row, inspection } = await setup(page)
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
  await page.clock.fastForward(60001)
  await expect(inspection.getByRole('status')).toHaveText('较早结果 · 发现主应用运行')
  await expect(inspection).toContainText('较早保护结果 · 归属待确认')
  await expect(inspection).toContainText('请重新检查')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'pending'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('status')).toContainText('正在检查')
  await expect(inspection).not.toContainText('主应用 PID')
  await page.evaluate(() => sessionStorage.setItem('inspection-release', 'yes'))
  await expect(inspection.getByRole('status')).toHaveText('发现主应用运行')
})

test('exact association locations stay retained, partial and revocable', async ({ page }) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationObserved'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(
    inspection.getByText('关联位置 · 1 处发现 · 1 处无法确认', { exact: true }),
  ).toBeVisible()
  await inspection.locator('.application-associations summary').click()
  const associations = inspection.locator('.application-associations')
  await expect(associations).toContainText('全部保留')
  await expect(associations).toContainText('缓存也可能含有个人数据')
  await expect(associations).toContainText('发现位置 · 归属待确认')
  await expect(associations).toContainText('~/Library/Application Support/org.example.same')
  await expect(associations).toContainText('链接、挂载或文件类型暂不支持')
  await expect(associations).toContainText('6 处常见位置本次未发现，不代表没有其他关联文件')
  await expect(associations.getByRole('checkbox')).toHaveCount(0)
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await associations.screenshot({
      path: `test-results/application-associations-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('0 处发现 · 8 处无法确认')
  await expect(associations).toContainText('检查期间发生变化，请重查')
  await expect(associations).not.toContainText('发现位置 · 归属待确认')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'mismatch'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(associations).toContainText('本地标识已变化，请重新读取应用列表')
  await expect(associations.locator('.association-path')).toHaveCount(0)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'associationFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection.locator('.application-associations')).toHaveCount(0)
})

test('registered installation copies explain shared data and keep incomplete or changed results conservative', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesFound'))
  await row.getByText('位置与来源', { exact: true }).click()
  await expect(inspection).toContainText('关联数据可能共用')
  const copies = inspection.locator('.application-installations')
  await copies.locator('summary').click()
  await expect(copies).toContainText('发现 1 个其他位置')
  await expect(copies).toContainText('/Applications/Example Beta.app')
  await expect(copies).toContainText('版本 3.0 · 构建 70')
  await expect(copies).toContainText('可能共用关联数据，请保留')
  await expect(copies).toContainText('网络卷与其他安装位置尚未完整覆盖')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await copies.screenshot({
      path: `test-results/application-copies-${theme}.png`,
      animations: 'disabled',
    })
  }
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesPartial'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies.locator('summary')).toContainText('检查不完整')
  await expect(copies).toContainText('结果不完整')
  await expect(copies).toContainText('/Applications/Example Beta.app')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesChanged'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies).toContainText('检查期间发生变化，已撤下对应结果')
  await expect(copies).not.toContainText('/Applications/Example Beta.app')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'other'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(copies).toContainText('本次未命中不代表数据独占')
  await expect(inspection).toContainText('另有 1 个同标识安装位置的主应用在运行')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'copiesFuture'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await expect(inspection.getByRole('alert')).toContainText('无法识别')
  await expect(inspection.locator('.application-installations')).toHaveCount(0)
  await expect(inspection.getByRole('button', { name: /卸载|退出|删除/ })).toHaveCount(0)
})

test('Mole retention reasons distinguish derived identifiers and channel names without claiming ownership', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'sharedRelated'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(inspection).toContainText('关联数据可能共用')
  await expect(shared).toContainText('派生标识，需保留关联数据')
  await expect(shared).toContainText('核对 Mole 清单前 64 项、系统登记前 32 项')
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'sharedChannel'))
  await inspection.getByRole('button', { name: '重新检查状态' }).click()
  await shared.locator('summary').click()
  await expect(shared).toContainText('渠道包名命中，需保留关联数据')
  await expect(shared).not.toContainText('派生标识，需保留关联数据')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-shared-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(shared.getByRole('checkbox')).toHaveCount(0)
  await expect(shared.getByRole('button')).toHaveCount(0)
})

test('installation-root discovery exposes inaccessible, changed and limited scope without clearing retention', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'rootsUnavailable'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared.locator('summary')).toContainText('检查不完整')
  await expect(shared).toContainText('已探查 9 个常用安装根')
  await expect(shared).toContainText('部分目录无法访问或布局不支持')
  await expect(shared).toContainText('/Applications')
  await expect(shared).toContainText('本次未命中不代表数据独占')
  for (const [mode, reason] of [
    ['rootsChanged', '目录在检查期间发生变化，请重查'],
    ['rootsLimit', '扫描达到数量或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared.locator('summary')).toContainText('检查不完整')
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-roots-${theme}.png`,
      animations: 'disabled',
    })
  }
  await expect(shared.getByRole('button')).toHaveCount(0)
  await expect(shared.getByRole('checkbox')).toHaveCount(0)
})

test('volume limitations keep data retained and show network, change and budget reasons', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'volumesNetwork'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared).toContainText('检查不完整')
  await expect(shared).toContainText('网络卷或自动挂载位置暂不支持')
  await expect(shared).toContainText('/Volumes/Shared Disk')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const [mode, reason] of [
    ['volumesChanged', '挂载或卷目录发生变化，请重查'],
    ['volumesLimit', '卷扫描达到数量或时间上限'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-volumes-${theme}.png`,
      animations: 'disabled',
    })
  }
})

test('receipt discovery keeps failure, changes, bounds and unsupported records visible without clearing retention', async ({
  page,
}) => {
  const { row, inspection } = await setup(page)
  await page.evaluate(() => sessionStorage.setItem('inspection-mode', 'receiptsFailed'))
  await row.getByText('位置与来源', { exact: true }).click()
  const shared = inspection.locator('.application-installations')
  await shared.locator('summary').click()
  await expect(shared.locator('summary')).toContainText('检查不完整')
  await expect(shared).toContainText('已核对 2 份非 Apple 安装收据')
  await expect(shared).toContainText('部分收据未能完整读取，请重试')
  await expect(shared).toContainText('只补查 /usr/local、/opt')
  await expect(shared).toContainText('未命中不代表数据独占')
  for (const [mode, reason] of [
    ['receiptsChanged', '安装收据在检查期间变化，请重查'],
    ['receiptsLimit', '收据核对达到数量或时间上限'],
    ['receiptsUnsupported', '部分收据格式或路径暂不支持'],
  ]) {
    await page.evaluate((mode) => sessionStorage.setItem('inspection-mode', mode), mode)
    await inspection.getByRole('button', { name: '重新检查状态' }).click()
    await expect(shared).toContainText(reason)
  }
  if ((await shared.getAttribute('open')) === null) await shared.locator('summary').click()
  for (const theme of ['light', 'dark']) {
    await page.setViewportSize({ width: 960, height: 780 })
    await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
    await expect(inspection.getByRole('button', { name: '重新检查状态' })).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(33, 34, 39)' : 'rgb(255, 255, 255)',
    )
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
    await shared.screenshot({
      path: `test-results/application-receipts-${theme}.png`,
      animations: 'disabled',
    })
  }
})
