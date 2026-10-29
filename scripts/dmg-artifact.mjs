import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import { command, fileDigest, verifySignedApp } from './app-artifact.mjs'

const guideName = '安装说明.txt'
export function installationGuide(expected) {
  return `MooMole ${expected.version} · ${expected.arch === 'arm64' ? 'Apple Silicon' : 'Intel'}

将 MooMole.app 拖入 Applications（应用程序），然后从应用程序启动。
升级前先正常退出旧版本，再由你确认替换；设置和历史保存在应用数据目录。
请勿直接从此磁盘映像运行应用。安装完成后可推出此映像。

Drag MooMole.app into Applications, then launch it from there.
Quit the old version normally before replacing it. Settings and history stay in the app's data directory.
Do not run the app from this disk image. Eject the image after installation.

MooMole.app を「アプリケーション」にコピーし、インストール先から起動してください。
置き換え前に旧版を通常終了してください。設定と履歴はアプリのデータフォルダに保持されます。
このディスクイメージから直接起動せず、インストール後は取り出してください。

本包使用免费的 ad-hoc 签名，未获 Apple 开发者身份认证或公证。
若 macOS 阻止打开，请阅读系统提示并按 Apple 官方说明自行确认：
https://support.apple.com/zh-cn/102445
本应用不会关闭 Gatekeeper 或移除系统安全属性；组织策略可能限制打开。

Free ad-hoc signing; no Apple Developer ID signing or notarization.
If macOS blocks opening, review its message and Apple's guidance:
https://support.apple.com/102445
The app does not disable Gatekeeper or remove security attributes. Organization policies may restrict opening.

無料の ad-hoc 署名を使用し、Apple Developer ID 署名や公証はありません。
macOS が起動を制限した場合は、システムの説明と Apple の案内を確認してください：
https://support.apple.com/ja-jp/102445
Gatekeeper の無効化や安全属性の削除は行いません。組織のポリシーにより起動できない場合があります。

安装、升级与各支持环境的验收范围和已知限制请查阅对应版本的发布说明：
Installation/upgrade validation scope and known limits / 検証範囲と既知の制限：
https://github.com/rememberber/MooMole/releases/tag/v${expected.version}

仅供学习研究，使用前请备份重要数据；清理可能造成文件丢失。
感谢 Mole 上游。MooMole 是非官方客户端，以 Mole CLI 可视化和简单直观为宗旨。
For learning and research. Back up important data; cleanup can cause data loss.
Thanks to Mole's contributors. MooMole is an independent, unofficial graphical client for Mole CLI.
学習・研究向けです。重要なデータをバックアップしてください。クリーンアップでデータを失う可能性があります。
Mole の貢献者に感謝します。MooMole は独立した非公式の Mole CLI GUI クライアントです。
Mole 官方项目：https://github.com/tw93/Mole
日常使用建议选择 Mole 官方产品：https://mole.fit/zh/
Official Mole product / Mole 公式製品：https://mole.fit/
`
}

function plist(program, args) {
  const xml = command(program, args).stdout
  const result = spawnSync('/usr/bin/plutil', ['-convert', 'json', '-o', '-', '-'], {
    input: xml,
    encoding: 'utf8',
    timeout: 10_000,
    maxBuffer: 2 * 1024 ** 2,
  })
  if (result.error || result.status !== 0) throw new Error('Cannot decode disk image plist')
  return JSON.parse(result.stdout)
}

// The image is created by this invocation, and must not already be attached.
// Match its exact canonical path; never infer ownership from a volume name.
export function ownedImages(info, image) {
  if (!Array.isArray(info.images)) throw new Error('Missing disk image inventory')
  return info.images.filter((entry) => entry['image-path'] === image)
}

export function mountedDevice(entry, mountpoint) {
  const entities = entry['system-entities']
  if (!Array.isArray(entities)) throw new Error('Missing disk image entities')
  const roots = entities.filter((entity) => /^\/dev\/disk\d+$/.test(entity['dev-entry']))
  const mounts = entities.filter((entity) => entity['mount-point'])
  if (roots.length !== 1 || mounts.length !== 1 || mounts[0]['mount-point'] !== mountpoint)
    throw new Error('Expected one owned disk and one private mount point')
  return roots[0]['dev-entry']
}

export function withMountedDmg(image, inspect) {
  image = realpathSync(image)
  const inventory = () => plist('/usr/bin/hdiutil', ['info', '-plist'])
  if (ownedImages(inventory(), image).length)
    throw new Error('DMG is already attached; leave existing mounts untouched')
  const work = realpathSync(mkdtempSync(join(tmpdir(), 'moomole-dmg-mount-')))
  const mountpoint = join(work, 'volume')
  mkdirSync(mountpoint)
  let failure
  try {
    command('/usr/bin/hdiutil', [
      'attach',
      image,
      '-readonly',
      '-nobrowse',
      '-noautoopen',
      '-verify',
      '-mountpoint',
      mountpoint,
      '-plist',
    ])
    const images = ownedImages(inventory(), image)
    if (images.length !== 1) throw new Error('Expected one attached image')
    mountedDevice(images[0], mountpoint)
    if (statSync(mountpoint).dev === statSync(work).dev)
      throw new Error('DMG did not mount at the private mount point')
    const volume = plist('/usr/sbin/diskutil', ['info', '-plist', mountpoint])
    if (volume.Writable !== false || volume.FilesystemType !== 'hfs')
      throw new Error('Expected a read-only HFS+ volume')
    return inspect(mountpoint)
  } catch (error) {
    failure = error
    throw error
  } finally {
    try {
      // Also handles a partially successful attach or a failed inspector.
      for (const entry of ownedImages(inventory(), image)) {
        const entities = entry['system-entities']
        const roots = entities?.filter((entity) => /^\/dev\/disk\d+$/.test(entity['dev-entry']))
        if (roots?.length !== 1) throw new Error('Cannot identify owned image for detach')
        command('/usr/bin/hdiutil', ['detach', roots[0]['dev-entry']])
      }
      if (ownedImages(inventory(), image).length || statSync(mountpoint).dev !== statSync(work).dev)
        throw new Error('Image is still attached')
      rmSync(work, { recursive: true, force: true })
    } catch (cleanup) {
      throw new AggregateError(
        [failure, cleanup].filter(Boolean),
        `DMG cleanup could not be confirmed; preserved ${work}. Inspect the owned image ${image}.`,
      )
    }
  }
}

function appFingerprint(app) {
  const entries = []
  function walk(path, name) {
    const meta = lstatSync(path)
    if (meta.isDirectory()) {
      entries.push([name, 'directory', meta.mode & 0o777])
      for (const child of readdirSync(path).sort()) walk(join(path, child), `${name}/${child}`)
    } else if (meta.isFile()) {
      entries.push([name, 'file', meta.mode & 0o777, fileDigest(path)])
    } else throw new Error('Unexpected file type in DMG App')
  }
  walk(app, '')
  return createHash('sha512').update(JSON.stringify(entries)).digest('hex')
}

export function verifyDmg(image, expected, fingerprint) {
  if (command('/usr/bin/hdiutil', ['imageinfo', image, '-format']).stdout.trim() !== 'UDZO')
    throw new Error('Expected a compressed read-only UDZO image')
  command('/usr/bin/hdiutil', ['verify', image])
  return withMountedDmg(image, (volume) => {
    const metadata = new Set(['.fseventsd', '.Trashes', '.Spotlight-V100', '.metadata_never_index'])
    const names = readdirSync(volume)
      .filter((name) => !metadata.has(name))
      .sort()
    if (JSON.stringify(names) !== JSON.stringify(['Applications', 'MooMole.app', guideName].sort()))
      throw new Error('Unexpected DMG root layout')
    const link = join(volume, 'Applications')
    if (!lstatSync(link).isSymbolicLink() || readlinkSync(link) !== '/Applications')
      throw new Error('Applications must be a link to /Applications')
    const guide = join(volume, guideName)
    if (!lstatSync(guide).isFile() || readFileSync(guide, 'utf8') !== installationGuide(expected))
      throw new Error('DMG installation instructions differ from this build')
    const app = join(volume, 'MooMole.app')
    const report = verifySignedApp(app, expected)
    const actual = appFingerprint(app)
    if (fingerprint && actual !== fingerprint)
      throw new Error('DMG App bytes or permissions differ from the signed source')
    return {
      format: 'UDZO',
      filesystem: 'HFS+',
      readOnly: true,
      validation: 'mounted-and-detached',
      appFingerprint: actual,
      appFingerprintEncoding: 'sha512-hex',
      app: report,
    }
  })
}

export function buildDmg(app, image, expected) {
  if (process.platform !== 'darwin') throw new Error('DMG creation requires macOS')
  image = resolve(image)
  if (!image.endsWith('.dmg') || lstatSync(image, { throwIfNoEntry: false }))
    throw new Error('DMG creation requires a fresh .dmg path')
  verifySignedApp(app, expected)
  app = realpathSync(app)
  image = join(realpathSync(dirname(image)), basename(image))
  const path = relative(app, image)
  if (path === '' || (!path.startsWith(`..${sep}`) && path !== '..' && !isAbsolute(path)))
    throw new Error('DMG destination must be outside the source App')
  const fingerprint = appFingerprint(app)
  const work = mkdtempSync(join(tmpdir(), 'moomole-dmg-payload-'))
  try {
    command('/usr/bin/ditto', [app, join(work, 'MooMole.app')])
    symlinkSync('/Applications', join(work, 'Applications'))
    writeFileSync(join(work, guideName), installationGuide(expected), { flag: 'wx' })
    command('/usr/bin/hdiutil', [
      'create',
      image,
      '-srcfolder',
      work,
      '-fs',
      'HFS+',
      '-format',
      'UDZO',
      '-volname',
      `MooMole ${expected.version}`,
      '-nospotlight',
    ])
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
  const report = verifyDmg(image, expected, fingerprint)
  verifySignedApp(app, expected)
  if (appFingerprint(app) !== fingerprint) throw new Error('Source App changed during packaging')
  return { ...report, ...fileDigest(image), sha512Encoding: 'hex' }
}
