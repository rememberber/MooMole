// Read-only, bounded process observation for an explicitly running local App.
// This never starts/stops a collector or inspects any metric payload or files.
import { execFileSync } from 'node:child_process'
import { writeFile } from 'node:fs/promises'
import path from 'node:path'
import { parseArgs } from 'node:util'
import { setTimeout as sleep } from 'node:timers/promises'

const { values } = parseArgs({
  options: {
    pid: { type: 'string' },
    seconds: { type: 'string', default: '180' },
    output: { type: 'string' },
  },
})
const pid = Number(values.pid)
const seconds = Number(values.seconds)
if (
  process.platform !== 'darwin' ||
  !Number.isSafeInteger(pid) ||
  pid < 1 ||
  !Number.isInteger(seconds) ||
  seconds < 5 ||
  seconds > 3600 ||
  !values.output
) {
  throw new Error(
    'Usage (macOS): node scripts/observe-status-resources.mjs --pid PID --seconds 180 --output NEW_REPORT.json; 5–3600 seconds',
  )
}
function ps(args) {
  return execFileSync('/bin/ps', args, {
    encoding: 'utf8',
    timeout: 2000,
    maxBuffer: 2 * 1024 * 1024,
    env: { ...process.env, LC_ALL: 'C' },
  }).trim()
}
function readProcess(id) {
  const line = ps(['-p', String(id), '-o', 'pid=,ppid=,pcpu=,rss=,lstart=,comm='])
  const fields = line.match(
    /^\s*(\d+)\s+(\d+)\s+([\d.]+)\s+(\d+)\s+([A-Za-z]{3}\s+[A-Za-z]{3}\s+\d+\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s+(.+)$/,
  )
  if (!fields) throw new Error('Process observation is unavailable or unrecognized')
  return {
    pid: Number(fields[1]),
    parent: Number(fields[2]),
    psCpu: Number(fields[3]),
    rssKiB: Number(fields[4]),
    started: fields[5],
    executable: fields[6],
  }
}
const owner = readProcess(pid)
if (!owner.executable.endsWith('/MooMole.app/Contents/MacOS/moomole'))
  throw new Error('PID must identify the explicit MooMole App executable')
const collectorPath = path.resolve(
  path.dirname(owner.executable),
  '../Resources/mole/bin/status-go',
)
const started = performance.now()
const report = {
  version: 1,
  startedAt: new Date().toISOString(),
  requestedSeconds: seconds,
  scope:
    'App main process and its single direct status collector only; excludes WebView, helpers, GPU, power and whole-system cost',
  measurement:
    'ps %CPU report and resident KiB; sampled every ~5s, not a CPU-time integral or physical footprint',
  outcome: 'observing',
  error: null,
  collectorCommand: null,
  samples: [],
  summary: null,
}
let collector
try {
  while (true) {
    const app = readProcess(pid)
    if (app.started !== owner.started || app.executable !== owner.executable)
      throw new Error('App identity changed')
    const children = ps(['-axo', 'pid=,ppid=,comm='])
      .split('\n')
      .flatMap((line) => {
        const row = line.match(/^\s*(\d+)\s+(\d+)\s+(.+)$/)
        return row && Number(row[2]) === pid && row[3] === collectorPath ? [Number(row[1])] : []
      })
    if (children.length !== 1)
      throw new Error(`Expected one collector, observed ${children.length}`)
    const child = readProcess(children[0])
    if (child.parent !== pid || child.executable !== collectorPath)
      throw new Error('Collector identity changed')
    if (collector && (collector.pid !== child.pid || collector.started !== child.started))
      throw new Error('Collector restarted during observation')
    if (!collector) {
      collector = child
      const command = ps(['-p', String(child.pid), '-o', 'args='])
      if (![1, 2, 5].some((n) => command === `${collectorPath} --watch --interval ${n}s`))
        throw new Error('Collector command is outside the supported observation scope')
      report.collectorCommand = command
    }
    const elapsedMs = Math.round(performance.now() - started)
    report.samples.push({ elapsedMs, app, collector: child })
    if (elapsedMs >= seconds * 1000) break
    await sleep(Math.min(5000, seconds * 1000 - elapsedMs))
  }
  report.outcome = 'completed'
} catch (error) {
  report.outcome = 'interrupted'
  report.error = String(error)
  process.exitCode = 1
}
function summary(kind) {
  const samples = report.samples.map((s) => s[kind])
  if (!samples.length) return null
  const cpu = samples.map((s) => s.psCpu)
  const rss = samples.map((s) => s.rssKiB / 1024)
  return {
    psCpuMin: Math.min(...cpu),
    psCpuMax: Math.max(...cpu),
    psCpuSampleMean: cpu.reduce((a, b) => a + b, 0) / cpu.length,
    rssMiBMin: Math.min(...rss),
    rssMiBMax: Math.max(...rss),
    rssMiBFirst: rss[0],
    rssMiBLast: rss.at(-1),
  }
}
report.finishedAt = new Date().toISOString()
report.summary = {
  count: report.samples.length,
  app: summary('app'),
  collector: summary('collector'),
}
await writeFile(values.output, JSON.stringify(report, null, 2) + '\n', { flag: 'wx', mode: 0o600 })
console.log(
  JSON.stringify(
    {
      outcome: report.outcome,
      error: report.error,
      output: path.resolve(values.output),
      ...report.summary,
    },
    null,
    2,
  ),
)
