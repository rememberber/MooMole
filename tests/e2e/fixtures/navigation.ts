import type { Page } from '@playwright/test'

const groupFor: Record<string, string> = {
  软件清理: '清理工具',
  开发缓存: '清理工具',
  安装包: '清理工具',
  工程产物: '清理工具',
  'AI 深度扫描': '清理工具',
  重复文件: '文件查找',
  相似照片与视频: '文件查找',
  'Similar photos and videos': 'File search tools',
  ソフトウェアのクリーンアップ: 'クリーンアップツール',
  系统状态: '系统监测',
  持续管理: '系统监测',
  'AI deep scan': 'Cleanup tools',
  'Developer caches': 'Cleanup tools',
  'Project artifacts': 'Cleanup tools',
  Installers: 'Cleanup tools',
  'System status': 'Monitoring tools',
}

export async function navigate(page: Page, name: string) {
  const nav = page.locator('.main-nav')
  const pageButton = nav.getByRole('button', { name, exact: true })
  const group = groupFor[name]
  if (group) {
    const disclosure = nav.getByRole('button', { name: group, exact: true })
    if ((await disclosure.getAttribute('aria-expanded')) === 'false') await disclosure.click()
  }
  await pageButton.click()
}
