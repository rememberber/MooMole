// Read-only OS settings probe. Never requests permission or submits a notification.
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import assert from 'node:assert/strict'
const root = resolve(import.meta.dirname, '..')
const temporary = mkdtempSync(join(tmpdir(), 'moomole-notification-probe-'))
function run(program, args, timeout = 30000) {
  const r = spawnSync(program, args, { encoding: 'utf8', timeout })
  if (r.error || r.status !== 0) throw Error(`${program}: ${r.error?.message ?? r.stderr}`)
  return r.stdout
}
try {
  const app = join(temporary, 'MooMole.app'),
    mac = join(app, 'Contents/MacOS')
  mkdirSync(mac, { recursive: true })
  writeFileSync(
    join(app, 'Contents/Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>io.github.rememberber.moomole</string><key>CFBundleExecutable</key><string>probe</string><key>CFBundleName</key><string>MooMole</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>CFBundleShortVersionString</key><string>0.2.0</string><key>LSUIElement</key><true/></dict></plist>`,
  )
  const source = join(temporary, 'main.m')
  writeFileSync(
    source,
    `#import <Foundation/Foundation.h>
extern char *moomole_storage_notification(const char*,const char*,const char*,const char*,const char*);
int main(){dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{@autoreleasepool{char *r=moomole_storage_notification("read","io.github.rememberber.moomole","","","");if(!r)exit(2);puts(r);free(r);exit(0);}});dispatch_main();}`,
  )
  run('/usr/bin/clang', [
    '-fobjc-arc',
    '-fblocks',
    '-framework',
    'Foundation',
    '-framework',
    'UserNotifications',
    join(root, 'src-tauri/native/storage_notifications.m'),
    source,
    '-o',
    join(mac, 'probe'),
  ])
  run('/usr/bin/codesign', [
    '--force',
    '--sign',
    '-',
    '--identifier',
    'io.github.rememberber.moomole',
    app,
  ])
  const permission = JSON.parse(run(join(mac, 'probe'), [], 15000))
  assert(
    ['notDetermined', 'denied', 'authorized', 'provisional', 'unknown'].includes(
      permission.authorization,
    ),
  )
  assert.equal(typeof permission.alerts, 'boolean')
  assert.equal(typeof permission.center, 'boolean')
  mkdirSync(join(root, '.cache'), { recursive: true })
  writeFileSync(
    join(root, '.cache/storage-notifications-native.json'),
    JSON.stringify({
      probe: 'production-native-adapter-in-signed-temporary-App',
      permission,
      permissionRequested: false,
      notificationsSubmitted: 0,
      temporaryBundleRemoved: true,
    }) + '\n',
  )
  console.log(
    JSON.stringify({
      actualNativePermission: permission,
      permissionRequested: false,
      notificationsSubmitted: 0,
    }),
  )
} finally {
  rmSync(temporary, { recursive: true, force: true })
}
