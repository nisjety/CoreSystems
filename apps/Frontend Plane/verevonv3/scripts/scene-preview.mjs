#!/usr/bin/env node
/**
 * Real-time preview driver for a product scene (plans/product-scenes/CONTRACT.md).
 *
 * Opens /scene/<id> on a running scene-mode dev server in headless Chromium,
 * waits for `window.__verevonScene.ready`, calls `start()` and then drives the
 * pointer on a WALL clock exactly as the capture harness does on virtual time:
 * every tick it evaluates `pointerAt(t)`, moves the real mouse (so `:hover` is
 * genuine), presses/releases on the pressed edges, and calls `setCursor`.
 *
 * It screenshots at fixed checkpoints, records console errors, page errors,
 * failed requests, every `unmatched-api` log entry, the request count per API
 * path (navbar polling check) and scans `document.body.innerText` at every
 * checkpoint for forbidden real-data strings.
 *
 *   node scripts/scene-preview.mjs --scene 02-plan [--url http://127.0.0.1:5211]
 *        [--out <dir>] [--viewport 1120x700] [--scale 1] [--checkpoints 0,1,1.8,...]
 *
 * Start the server first:
 *   VITE_SCENE_MODE=1 pnpm exec vite --host 127.0.0.1 --port 5211 --strictPort
 */
import { chromium } from '@playwright/test'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const spaRoot = path.resolve(here, '..')

const args = parseArgs(process.argv.slice(2))
const sceneId = args.scene ?? '02-plan'
const baseUrl = (args.url ?? 'http://127.0.0.1:5211').replace(/\/+$/, '')
const outDir = path.resolve(args.out ?? path.join(spaRoot, 'test-results', 'scene-preview', sceneId))
const scale = Number(args.scale ?? '1')
const checkpoints = (args.checkpoints ?? '0,1.0,1.8,2.6,3.5,4.4,5.5,6.3,7.0,7.8')
  .split(',')
  .map((value) => Number(value.trim()))
  .filter((value) => Number.isFinite(value))
const fixedNow = new Date('2026-10-09T12:00:00+02:00')

// GDPR guard (CONTRACT.md section 0): none of these may appear in any frame.
const FORBIDDEN_PATTERNS = [
  /\bIma\b/,
  /Aquatiq/i,
  /Velion/i,
  /[A-Z0-9._%+-]+@(?!fjordform\.example\b)[A-Z0-9.-]+\.[A-Z]{2,}/i,
]

const viewport = await resolveViewport(args.viewport)

await mkdir(outDir, { recursive: true })

const browser = await chromium.launch({ headless: true })
const context = await browser.newContext({
  viewport,
  deviceScaleFactor: scale,
  locale: 'nb-NO',
  timezoneId: 'Europe/Oslo',
  reducedMotion: 'no-preference',
  colorScheme: 'light',
})
const page = await context.newPage()
await page.clock.setFixedTime(fixedNow)

const consoleErrors = []
const consoleWarnings = []
const pageErrors = []
const failedRequests = []
page.on('console', (message) => {
  if (message.type() === 'error') consoleErrors.push(message.text())
  else if (message.type() === 'warning') consoleWarnings.push(message.text())
})
page.on('pageerror', (error) => pageErrors.push(String(error)))
page.on('requestfailed', (request) => failedRequests.push(`${request.method()} ${request.url()} :: ${request.failure()?.errorText ?? ''}`))
page.on('response', (response) => {
  if (response.status() >= 400) failedRequests.push(`${response.request().method()} ${response.url()} :: HTTP ${response.status()}`)
})

const url = `${baseUrl}/scene/${sceneId}`
console.log(`[scene-preview] opening ${url} at ${viewport.width}x${viewport.height} @${scale}x`)
await page.goto(url, { waitUntil: 'load' })
await page.waitForFunction(() => Boolean(window.__verevonScene), null, { timeout: 30_000 })
const readyStartedAt = performance.now()
await page.evaluate(() => window.__verevonScene.ready)
console.log(`[scene-preview] ready after ${Math.round(performance.now() - readyStartedAt)} ms`)

const durationMs = await page.evaluate(() => window.__verevonScene.durationMs)

// Harness protocol on a wall clock.
const report = { sceneId, viewport, scale, url, checkpoints: [], forbiddenHits: [], beats: [] }
let pressed = false
let nextCheckpoint = 0
const pointerTrace = []
await page.evaluate(() => window.__verevonScene.start())
const t0 = performance.now()
let lastCursor = { x: -1, y: -1, visible: false, pressed: false }

while (true) {
  const t = performance.now() - t0
  if (t > durationMs + 50) break
  const pointer = await page.evaluate((tMs) => window.__verevonScene.pointerAt(tMs), t)
  if (pointer.visible || lastCursor.visible) {
    if (pointer.x !== lastCursor.x || pointer.y !== lastCursor.y) await page.mouse.move(pointer.x, pointer.y)
  }
  if (pointer.pressed && !pressed) {
    await page.mouse.down({ button: 'left' })
    pressed = true
    pointerTrace.push({ t: Math.round(t), action: 'down', x: pointer.x, y: pointer.y })
  } else if (!pointer.pressed && pressed) {
    await page.mouse.up({ button: 'left' })
    pressed = false
    pointerTrace.push({ t: Math.round(t), action: 'up', x: pointer.x, y: pointer.y })
  }
  if (pointer.visible !== lastCursor.visible) pointerTrace.push({ t: Math.round(t), action: pointer.visible ? 'show' : 'hide', x: pointer.x, y: pointer.y })
  await page.evaluate((state) => window.__verevonScene.setCursor(state.x, state.y, state.visible, state.pressed), pointer)
  lastCursor = pointer

  if (nextCheckpoint < checkpoints.length && t >= checkpoints[nextCheckpoint] * 1000) {
    const wanted = checkpoints[nextCheckpoint]
    nextCheckpoint += 1
    const snapshot = await page.evaluate(() => ({
      text: document.body.innerText,
      scrollbars: Array.from(document.querySelectorAll('*')).filter((element) => {
        const style = getComputedStyle(element)
        const scrollable = (style.overflowY === 'auto' || style.overflowY === 'scroll') && element.scrollHeight > element.clientHeight + 1
        return scrollable && element.clientWidth > 0 && element.offsetWidth - element.clientWidth > 0
      }).map((element) => `${element.tagName.toLowerCase()}.${String(element.className).split(' ')[0]}`),
      hovered: document.querySelectorAll(':hover').length,
      approveHovered: Boolean(document.querySelector('.verevon-chat-approval__approve:hover')),
      spinners: document.querySelectorAll('.session-loading, [aria-busy="true"]').length,
      cursor: document.querySelector('.verevon-scene-cursor')?.getAttribute('data-visible'),
    }))
    const file = path.join(outDir, `t-${wanted.toFixed(1).replace('.', '_')}s.png`)
    await page.screenshot({ path: file, fullPage: false })
    const tAfter = Math.round(performance.now() - t0)
    const hits = FORBIDDEN_PATTERNS.map((pattern) => snapshot.text.match(pattern)?.[0]).filter(Boolean)
    if (hits.length > 0) report.forbiddenHits.push({ checkpoint: wanted, hits })
    report.checkpoints.push({
      wanted,
      actualMs: Math.round(t),
      screenshotDoneMs: tAfter,
      file,
      cursorVisible: snapshot.cursor,
      approveHovered: snapshot.approveHovered,
      visibleScrollbars: snapshot.scrollbars,
      busy: snapshot.spinners,
      textLength: snapshot.text.length,
    })
    await writeFile(path.join(outDir, `text-${wanted.toFixed(1).replace('.', '_')}s.txt`), snapshot.text, 'utf8')
    console.log(`[scene-preview] checkpoint ${wanted}s captured at ${Math.round(t)} ms (shot done ${tAfter} ms) hovered=${snapshot.approveHovered} scrollbars=${snapshot.scrollbars.length}`)
  }
  await new Promise((resolve) => setTimeout(resolve, 4))
}

const sceneLog = await page.evaluate(() => window.__verevonScene.log.slice())
const requestCounts = await page.evaluate(() => {
  // Exposed by the runtime for the navbar-polling check.
  const counts = window.__verevonSceneRequestCounts
  return counts ? Object.fromEntries(counts) : null
})
const unmatched = sceneLog.filter((entry) => entry.kind === 'unmatched-api')

report.beats = sceneLog
report.pointerTrace = pointerTrace
report.consoleErrors = consoleErrors
report.consoleWarnings = consoleWarnings
report.pageErrors = pageErrors
report.failedRequests = failedRequests
report.unmatchedApi = unmatched
report.requestCounts = requestCounts
await writeFile(path.join(outDir, 'preview-report.json'), JSON.stringify(report, null, 2), 'utf8')

console.log('\n[scene-preview] scene log:')
for (const entry of sceneLog) console.log(`  ${String(entry.atMs).padStart(5)} ms  ${entry.kind.padEnd(14)} ${entry.note}`)
console.log('\n[scene-preview] pointer trace:', JSON.stringify(pointerTrace))
console.log(`\n[scene-preview] console errors (${consoleErrors.length}):`)
for (const line of consoleErrors) console.log(`  ${line}`)
console.log(`[scene-preview] console warnings (${consoleWarnings.length}):`)
for (const line of consoleWarnings) console.log(`  ${line}`)
console.log(`[scene-preview] page errors (${pageErrors.length}):`)
for (const line of pageErrors) console.log(`  ${line}`)
console.log(`[scene-preview] failed requests (${failedRequests.length}):`)
for (const line of failedRequests) console.log(`  ${line}`)
console.log(`[scene-preview] unmatched /api requests (${unmatched.length}):`)
for (const entry of unmatched) console.log(`  ${entry.atMs} ms ${entry.note}`)
console.log('[scene-preview] request counts per path:', JSON.stringify(requestCounts))
console.log(`[scene-preview] forbidden strings: ${report.forbiddenHits.length === 0 ? 'none' : JSON.stringify(report.forbiddenHits)}`)
console.log(`[scene-preview] report: ${path.join(outDir, 'preview-report.json')}`)

await browser.close()

const ok = consoleErrors.length === 0 && pageErrors.length === 0 && report.forbiddenHits.length === 0
process.exit(ok ? 0 : 1)

function parseArgs(argv) {
  const out = {}
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (!arg.startsWith('--')) continue
    const key = arg.slice(2)
    const next = argv[index + 1]
    if (next && !next.startsWith('--')) {
      out[key] = next
      index += 1
    } else {
      out[key] = 'true'
    }
  }
  return out
}

async function resolveViewport(spec) {
  if (spec) {
    const [width, height] = spec.split('x').map((value) => Number(value))
    if (width && height) return { width, height }
  }
  const raw = await readFile(path.join(spaRoot, 'src', 'demo', 'scene-config.json'), 'utf8')
  return JSON.parse(raw).viewport
}
