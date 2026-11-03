// Exercises only this script's unique temporary main App, then unregisters it.
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import assert from 'node:assert/strict'
const root = resolve(import.meta.dirname, '..'),
  temp = mkdtempSync(join(tmpdir(), 'moomole-login-owned-'))
const bundle = 'io.github.rememberber.moomole.loginowned.' + randomUUID().replaceAll('-', ''),
  app = join(temp, 'Owned.app'),
  mac = join(app, 'Contents/MacOS'),
  binary = join(mac, 'probe')
let attempted = false,
  clean = false,
  inFlight = false,
  sequence = 0
function run(program, args, timeout = 20000) {
  const r = spawnSync(program, args, { encoding: 'utf8', timeout })
  if (r.error || r.status !== 0) throw Error(`${program}: ${r.error?.message ?? r.stderr}`)
  return r.stdout
}
function operation(action) {
  const out = join(temp, `result-${sequence++}.json`)
  inFlight = true
  run('/usr/bin/open', ['-n', '-g', '-W', app, '--args', String(action), out])
  const r = JSON.parse(readFileSync(out, 'utf8'))
  inFlight = false
  return r
}
try {
  mkdirSync(mac, { recursive: true })
  writeFileSync(
    join(app, 'Contents/Info.plist'),
    `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${bundle}</string><key>CFBundleExecutable</key><string>probe</string><key>CFBundleName</key><string>Owned login probe</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleVersion</key><string>1</string><key>LSUIElement</key><true/></dict></plist>`,
  )
  const source = join(temp, 'main.m')
  writeFileSync(
    source,
    `#import <AppKit/AppKit.h>
extern char *moomole_main_app_login(int,const char*);
int main(int argc,char**argv){[NSApplication sharedApplication];int op=argc>=2?atoi(argv[1]):0;NSString *out=argc>=3?[NSString stringWithUTF8String:argv[2]]:nil;dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{@autoreleasepool{char *r=moomole_main_app_login(op,"${bundle}");if(!r)exit(2);if(out)[[NSData dataWithBytes:r length:strlen(r)] writeToFile:out atomically:YES];else puts(r);free(r);exit(0);}});[NSApp run];}`,
  )
  run('/usr/bin/clang', [
    '-fobjc-arc',
    '-fblocks',
    '-framework',
    'AppKit',
    '-framework',
    'Foundation',
    '-framework',
    'ServiceManagement',
    join(root, 'src-tauri/native/main_app_login.m'),
    source,
    '-o',
    binary,
  ])
  run('/usr/bin/codesign', ['--force', '--sign', '-', '--identifier', bundle, app])
  const before = operation(0)
  assert(['notFound', 'notRegistered'].includes(before.system?.status))
  attempted = true
  const enabled = operation(1),
    reopened = operation(0)
  const disabled = operation(2),
    final = operation(0)
  clean = final.system?.status === 'notRegistered'
  assert(clean, 'temporary login registration not removed')
  const proof = {
    bundle,
    bundlePath: app,
    fixture: temp,
    before,
    enabled,
    reopened,
    disabled,
    final,
    ownRegistrationRemoved: true,
    temporaryBundleRemoved: true,
    adHocSigned: true,
    systemLoginNotPerformed: true,
    productionUiVerified: false,
  }
  mkdirSync(join(root, '.cache'), { recursive: true })
  writeFileSync(
    join(root, '.cache/main-app-login-native.json'),
    JSON.stringify(proof, null, 2) + '\n',
  )
  assert.equal(enabled.accepted, true, JSON.stringify(enabled))
  assert(['enabled', 'requiresApproval'].includes(reopened.system?.status))
  assert.equal(disabled.accepted, true, JSON.stringify(disabled))
  console.log(
    JSON.stringify({
      before: before.system.status,
      enabled: reopened.system.status,
      final: final.system.status,
      ownRegistrationRemoved: clean,
    }),
  )
} finally {
  if (inFlight) throw Error(`Owned probe still needs process reconciliation: ${app}`)
  if (attempted && !clean) {
    const observed = operation(0)
    if (['enabled', 'requiresApproval'].includes(observed.system?.status)) operation(2)
    const verified = operation(0)
    clean = verified.system?.status === 'notRegistered'
    if (!clean)
      throw Error(`Owned login sample requires reconciliation: ${app}; ${JSON.stringify(verified)}`)
  }
  if (!attempted || clean) rmSync(temp, { recursive: true, force: true })
}
