#!/usr/bin/env node
/**
 * Privacy and smoke probe of the built interactive demo (stage 04).
 *
 *   node scripts/probe-product-demo.mjs [--dir <published folder>] [--manifest <file>]
 *                                       [--out <folder>] [--timeout-ms <ms>]
 *
 * The probe serves the published folder on a free port between 5301 and 5399, with the hosting
 * headers recorded in the manifest. It embeds the demo in a host page with
 * <iframe sandbox="allow-scripts">, the same as the site, and drives headless Chromium through it.
 * Each visit runs in a fresh browser context. Inside the frame the probe reads and acts through the
 * page's own JavaScript (frame.evaluate), so the demo's guard is the context under test.
 *
 * Visit "dashboard": the ready message; the dashboard greeting within the cold budget (requests and
 * brotli bytes); the guard's replacements in the frame's own JavaScript world; the persistent label;
 * no forbidden text in the frame; a product API path that no responder answers gets a 404 with the
 * demo_unavailable body; a free-text prompt is answered with the honest demo reply.
 *
 * Visit "links": a link that leaves the demo, and a product page the demo does not offer, is refused
 * with the notice; an offered link is routed inside the demo; the browser address field is inert (absent
 * or disabled), and an enabled one must leave nothing blocked when an address is typed into it; the host
 * protocol (pause, resume, deactivate, activate, reset) and Escape work.
 *
 * Both visits assert: no request outside the static origin; no request from the demo's frame outside
 * its own folder; no /api request on the wire; no console error (except the known zod CSP report, at
 * most three); no uncaught page error; no failed request and no 4xx or 5xx response from the static
 * origin; no guard line in the console (a blocked or unanswered request).
 * Evidence goes to <out>/probe.json, with screenshots in <out>.
 */
import { chromium } from '@playwright/test'
import { createServer } from 'node:http'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, extname, join, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { brotliCompressSync, constants as zlibConstants } from 'node:zlib'

const SPA_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const DEFAULT_WEB_ROOT = join(SPA_ROOT, 'apps', 'verevon-web')
const HOST_PATH = '/__probe__/host.html'
const DEMO_PREFIX = '/product-demo/'
const GREETING_SOURCE = '/God (morgen|ettermiddag|kveld)\\b/i'
const PROMPT = 'Hva er været i Bergen i morgen?'
const DEMO_REPLY_SOURCE = '/Dette er en demo/'
const LABEL = 'Demo · fiktive data'
const GUARD_PREFIX = '[demo-guard]'
/** Chunks warmed in the background by the product, outside the critical path (see runDashboardVisit). */
const BACKGROUND_PREFETCH = /\/assets\/ChatPage-[^/]+\.js$/
/** The zod feature probe (new Function('')) is refused by the demo's CSP. It is benign, and only this many reports are tolerated. */
const BENIGN_EVAL_REPORT = /Refused to evaluate a string as JavaScript because 'unsafe-eval'/
const MAX_BENIGN_EVAL_REPORTS = 3
const FORBIDDEN_IN_FRAME = [
  ['Aquatiq', /aquatiq/i],
  ['Velion', /velion/i],
  ['Ima', /\bIma\b/],
  ['CoresSystem', /coresystem/i],
  ['localhost:5173', /localhost:5173/],
]
/** A product API path that no responder answers: the demo must answer it with a 404 and the Norwegian body. */
const UNMATCHED_API_PATH = '/api/v1/demo-probe/unmatched'
const UNAVAILABLE_CODE = 'demo_unavailable'
/** The notice the link policy shows (DemoApp.tsx). */
const LINK_NOTICE = 'Denne lenken er ikke med i demoen.'
/** Links the demo must refuse: one that leaves the demo, and a product page the demo does not offer. */
const REFUSED_LINKS = ['https://external.example/side', '/chat/findings', 'mailto:ida@fjordform.example']
/** The browser pane's address field (BrowserChrome.tsx) and an address typed into it. */
const ADDRESS_FIELD = 'input[aria-label="Nettleseradresse"]'
const ADDRESS_QUERY = 'Solhaug Hotell tilbud'
/** The browser asks for its icon on its own: that request is not part of the demo. */
const BROWSER_ICON_PATH = '/favicon.ico'
const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.json': 'application/json; charset=utf-8',
  '.woff2': 'font/woff2',
  '.webm': 'video/webm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.txt': 'text/plain; charset=utf-8',
}
const HOST_HTML = `<!doctype html>
<html lang="nb">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Probe</title>
<style>body{margin:0;background:#fff}iframe{display:block;width:1024px;height:640px;border:0}</style>
</head>
<body>
<iframe id="demo" title="Interaktiv demo" sandbox="allow-scripts" src="/product-demo/index.html" allow=""></iframe>
<script>
window.__demoMessages = [];
window.__foreignMessages = 0;
window.addEventListener('message', function (event) {
  var frame = document.getElementById('demo');
  if (event.source !== frame.contentWindow) { window.__foreignMessages += 1; return; }
  var data = event.data;
  if (data && data.source === 'verevon-demo' && data.v === 1) window.__demoMessages.push(data);
});
</script>
</body>
</html>
`

function parseArgs(argv) {
  const args = { dir: undefined, manifest: undefined, out: undefined, timeoutMs: 60000 }
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i]
    if (arg === '--dir') args.dir = argv[++i]
    else if (arg === '--manifest') args.manifest = argv[++i]
    else if (arg === '--out') args.out = argv[++i]
    else if (arg === '--timeout-ms') args.timeoutMs = Number(argv[++i])
    else throw new Error(`unknown option ${arg}`)
  }
  return args
}

/** Header rules whose source covers the path. `:path*` also matches the folder itself, as Next does. */
function headersFor(pathname, rules) {
  const headers = {}
  for (const rule of rules) {
    const base = rule.source.replace(/\/?:path\*$/, '')
    if (pathname === base || pathname.startsWith(`${base}/`)) {
      for (const { key, value } of rule.headers) headers[key.toLowerCase()] = value
    }
  }
  return headers
}

/** The static server: the production rewrite (/product-demo serves the entry), the hosting headers, and the host page. */
function makeHandler(outDir, rules) {
  const root = resolve(outDir)
  return async (req, res) => {
    const pathname = decodeURIComponent(new URL(req.url ?? '/', 'http://localhost').pathname)
    if (pathname === HOST_PATH) {
      res.writeHead(200, { 'content-type': TYPES['.html'], 'cache-control': 'no-store' })
      res.end(HOST_HTML)
      return
    }
    let rel = null
    if (pathname === '/product-demo' || pathname === '/product-demo/') rel = 'index.html'
    else if (pathname.startsWith(DEMO_PREFIX)) rel = pathname.slice(DEMO_PREFIX.length)
    const file = rel === null ? null : resolve(root, rel)
    if (file === null || (file !== root && !file.startsWith(root + sep))) {
      res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' })
      res.end('not found')
      return
    }
    try {
      const body = await readFile(file)
      const headers = { 'content-type': TYPES[extname(file).toLowerCase()] ?? 'application/octet-stream' }
      res.writeHead(200, { ...headers, ...headersFor(pathname, rules) })
      res.end(body)
    } catch {
      res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' })
      res.end('not found')
    }
  }
}

async function listenOnFreePort(handler) {
  for (let port = 5301; port <= 5399; port++) {
    const server = createServer(handler)
    try {
      await new Promise((resolvePromise, rejectPromise) => {
        server.once('error', rejectPromise)
        server.listen(port, '127.0.0.1', () => resolvePromise())
      })
      return { server, port }
    } catch (error) {
      server.close()
      if (error.code !== 'EADDRINUSE') throw error
    }
  }
  throw new Error('no free port between 5301 and 5399')
}

function brotliBytes(bytes) {
  return brotliCompressSync(bytes, {
    params: {
      [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
      [zlibConstants.BROTLI_PARAM_SIZE_HINT]: bytes.length,
    },
  }).length
}

async function measure(request) {
  try {
    const response = await request.response()
    const body = response ? await response.body() : Buffer.alloc(0)
    return { url: request.url(), bytes: body.length, brotli: brotliBytes(body) }
  } catch {
    return { url: request.url(), bytes: 0, brotli: 0 }
  }
}

/** The demo's frame, found by its URL. Polls until the iframe has loaded its document. */
async function findDemoFrame(page, timeoutMs) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    const frame = page.frames().find((candidate) => candidate.url().includes(DEMO_PREFIX))
    if (frame) return frame
    await new Promise((done) => setTimeout(done, 100))
  }
  return null
}

/** One visit to the demo in a fresh browser context. Everything the probe watches is recorded from the start. */
async function openVisit(browser, origin, timeoutMs) {
  const context = await browser.newContext({
    locale: 'nb-NO',
    timezoneId: 'Europe/Oslo',
    viewport: { width: 1280, height: 800 },
    serviceWorkers: 'block',
  })
  const page = await context.newPage()
  const watch = { requests: [], finished: [], consoleMessages: [], pageErrors: [], failedRequests: [], badResponses: [] }

  page.on('request', (request) =>
    watch.requests.push({ url: request.url(), method: request.method(), type: request.resourceType(), frameUrl: request.frame()?.url() ?? '' }),
  )
  page.on('requestfailed', (request) => watch.failedRequests.push({ url: request.url(), error: request.failure()?.errorText ?? 'unknown' }))
  page.on('requestfinished', (request) => {
    if (request.url().startsWith(origin) && request.url() !== `${origin}${HOST_PATH}`) watch.finished.push(measure(request))
  })
  page.on('response', (response) => {
    const url = response.url()
    if (url.startsWith(origin) && response.status() >= 400 && !url.endsWith(BROWSER_ICON_PATH)) {
      watch.badResponses.push({ url, status: response.status() })
    }
  })
  page.on('console', (message) => watch.consoleMessages.push({ type: message.type(), text: message.text() }))
  page.on('pageerror', (error) => watch.pageErrors.push(String(error.stack ?? error.message ?? error)))

  await page.goto(`${origin}${HOST_PATH}`, { waitUntil: 'load', timeout: timeoutMs })
  const ready = await page
    .waitForFunction(() => (window.__demoMessages ?? []).some((message) => message.type === 'ready'), undefined, { timeout: timeoutMs })
    .then(() => true, () => false)
  const demo = ready ? await findDemoFrame(page, timeoutMs) : null
  return { context, page, demo, ready, watch }
}

function waitForGreeting(frame, timeoutMs) {
  return frame
    .waitForFunction(
      (source) => new RegExp(source.slice(1, source.lastIndexOf('/')), source.slice(source.lastIndexOf('/') + 1)).test(document.body.innerText),
      GREETING_SOURCE,
      { timeout: timeoutMs },
    )
    .then(() => true, () => false)
}

/** Clicks an anchor the way a visitor does: a click event on an anchor in the demo's own document. */
async function clickAnchor(frame, href) {
  await frame.evaluate((target) => {
    const anchor = document.createElement('a')
    anchor.setAttribute('href', target)
    anchor.setAttribute('data-probe', 'link')
    anchor.textContent = 'probe'
    document.body.append(anchor)
    anchor.click()
    anchor.remove()
  }, href)
}

function waitForNotice(frame, text, timeoutMs) {
  return frame
    .waitForFunction(
      (expected) => {
        const notice = document.querySelector('.demo-notice')
        return notice?.getAttribute('data-visible') === 'true' && (notice.textContent ?? '').includes(expected)
      },
      text,
      { timeout: timeoutMs },
    )
    .then(() => true, () => false)
}

function waitForNoticeHidden(frame, timeoutMs) {
  return frame
    .waitForFunction(() => document.querySelector('.demo-notice')?.getAttribute('data-visible') !== 'true', undefined, { timeout: timeoutMs })
    .then(() => true, () => false)
}

function noticeVisible(frame) {
  return frame.evaluate(() => document.querySelector('.demo-notice')?.getAttribute('data-visible') === 'true')
}

/** Sends a host message to the demo's frame, as the site does. */
async function postToDemo(page, message) {
  await page.evaluate((payload) => {
    const frame = document.getElementById('demo')
    frame?.contentWindow?.postMessage(payload, '*')
  }, message)
}

/**
 * The request, response and console checks of one visit. The summary goes into result.visits, so the
 * evidence keeps what the checks read.
 */
function assessNetwork(label, watch, origin, check, result) {
  const outside = watch.requests.filter((request) => !request.url.startsWith(origin) && !request.url.startsWith('data:') && !request.url.startsWith('blob:'))
  check(`${label}: no request outside the static origin`, outside.length === 0, outside.slice(0, 5).map((request) => request.url).join(' '))
  // The demo's own requests stay in its folder. A root-relative asset path (for example /verevon-mark.svg)
  // stays on the same origin, so the origin check alone would miss it.
  const leftFolder = watch.requests.filter((request) => request.frameUrl.includes(DEMO_PREFIX) && !request.url.startsWith(`${origin}${DEMO_PREFIX}`))
  check(`${label}: the demo requests only its own folder`, leftFolder.length === 0, leftFolder.slice(0, 5).map((request) => request.url).join(' '))
  const apiCalls = watch.requests.filter((request) => {
    try {
      return new URL(request.url).pathname.startsWith('/api')
    } catch {
      return false
    }
  })
  check(`${label}: no /api request on the wire`, apiCalls.length === 0, apiCalls.slice(0, 5).map((request) => request.url).join(' '))

  const errors = watch.consoleMessages.filter((message) => message.type === 'error')
  const benign = errors.filter((message) => BENIGN_EVAL_REPORT.test(message.text))
  const realErrors = errors.filter((message) => !BENIGN_EVAL_REPORT.test(message.text))
  check(`${label}: no console error`, realErrors.length === 0, realErrors.slice(0, 3).map((message) => message.text).join(' | '))
  check(`${label}: at most ${MAX_BENIGN_EVAL_REPORTS} benign CSP eval reports`, benign.length <= MAX_BENIGN_EVAL_REPORTS, `${benign.length} reports`)

  // Playwright reads navigator.serviceWorker in its own utility script when it first touches a sandboxed
  // frame. That read throws in the utility world only, where the guard does not apply (guardState shows the
  // page's own world is replaced). It is recorded, and it has no URL frame. Any other uncaught error fails.
  const instrumentationError = (text) => /Service worker is disabled because the context is sandboxed/.test(text) && !/https?:\/\//.test(text)
  const demoPageErrors = watch.pageErrors.filter((text) => !instrumentationError(text))
  check(`${label}: no uncaught page error`, demoPageErrors.length === 0, demoPageErrors.slice(0, 2).join(' | '))
  const sameOriginFailures = watch.failedRequests.filter((request) => request.url.startsWith(origin))
  check(`${label}: no failed request to the static origin`, sameOriginFailures.length === 0, sameOriginFailures.slice(0, 3).map((request) => `${request.url} ${request.error}`).join(' | '))
  // A 404 is not a failed request: a root-relative path that misses the folder answers with one.
  check(`${label}: no 4xx or 5xx from the static origin`, watch.badResponses.length === 0, watch.badResponses.slice(0, 3).map((item) => `${item.status} ${item.url}`).join(' | '))

  // Every block and every unanswered product call of the guard is a console line with its prefix.
  const guardLines = watch.consoleMessages.filter((message) => message.text.startsWith(GUARD_PREFIX))
  // The probe asks for UNMATCHED_API_PATH on purpose, to prove the 404; that one miss is expected.
  const unanswered = guardLines
    .filter((message) => message.text.includes('no responder') && !message.text.includes(UNMATCHED_API_PATH))
    .map((message) => message.text)
  check(`${label}: no unanswered product API call`, unanswered.length === 0, unanswered.slice(0, 3).join(' | '))
  const unexpectedGuardLines = guardLines.filter((message) => !message.text.includes(UNMATCHED_API_PATH))
  check(`${label}: no guard line (a blocked request)`, unexpectedGuardLines.length === 0, unexpectedGuardLines.slice(0, 3).map((message) => message.text).join(' | '))

  result.visits[label] = {
    requests: watch.requests.length,
    outside: outside.map((request) => request.url),
    api: apiCalls.map((request) => request.url),
    list: watch.requests.map((request) => `${request.method} ${request.url}`),
    consoleMessages: watch.consoleMessages,
    pageErrors: watch.pageErrors,
    failedRequests: watch.failedRequests,
    badResponses: watch.badResponses,
    instrumentationErrors: watch.pageErrors.filter(instrumentationError).length,
  }
}

/** Visit "dashboard": the greeting, the cold budget, the guard in the frame, the API contract, a prompt. */
async function runDashboardVisit(browser, origin, budgets, result, outRoot, timeoutMs, check) {
  const visit = await openVisit(browser, origin, timeoutMs)
  check('dashboard: ready message from the demo', visit.ready)
  try {
    if (!visit.demo) {
      await visit.page.screenshot({ path: join(outRoot, 'probe-not-ready.png') })
    } else {
      await dashboardSteps(visit, budgets, result, outRoot, timeoutMs, check)
    }
  } finally {
    await visit.context.close()
  }
  assessNetwork('dashboard', visit.watch, origin, check, result)
}

/** Everything that happens inside the frame of the dashboard visit. Each step runs after the greeting. */
async function dashboardSteps(visit, budgets, result, outRoot, timeoutMs, check) {
  const { page, demo, watch } = visit
  const greeting = await waitForGreeting(demo, timeoutMs)
  check('dashboard: greeting visible', greeting)
  if (greeting) await page.screenshot({ path: join(outRoot, 'probe-dashboard.png') })

  // Cold path: what the browser fetched before the greeting was on screen. The chat chunk is
  // warmed in the background by DashboardHome (a product choice for the composer-to-chat
  // transition). It does not block the dashboard, so the budget counts the critical path and
  // the evidence reports both totals.
  const coldRows = await Promise.all(watch.finished.slice())
  const critical = coldRows.filter((row) => !BACKGROUND_PREFETCH.test(row.url))
  const sum = (rows) => rows.reduce((total, row) => total + row.brotli, 0)
  const criticalBrotli = sum(critical)
  result.cold = {
    criticalRequests: critical.length,
    criticalBrotliBytes: criticalBrotli,
    totalRequests: coldRows.length,
    totalBrotliBytes: sum(coldRows),
    budgetRequests: budgets.maxColdRequests,
    budgetBrotliBytes: budgets.maxColdBrotliBytes,
    rows: coldRows.map((row) => ({ url: row.url.replace(/^https?:\/\/[^/]+/, ''), bytes: row.bytes, brotli: row.brotli })),
  }
  check('dashboard: cold path within the request budget', critical.length <= budgets.maxColdRequests, `${critical.length} requests`)
  check('dashboard: cold path within the brotli budget', criticalBrotli <= budgets.maxColdBrotliBytes, `${criticalBrotli} bytes brotli (total ${sum(coldRows)})`)

  // Whether the guard's replacements are in effect in the frame's own JavaScript world.
  result.guardState = await demo
    .evaluate(() => {
      let serviceWorker
      try {
        serviceWorker = typeof navigator.serviceWorker
      } catch (error) {
        serviceWorker = `throws ${error.name}`
      }
      return {
        serviceWorkerOwnProperty: Object.getOwnPropertyDescriptor(navigator, 'serviceWorker') !== undefined,
        serviceWorker,
        geolocation: typeof navigator.geolocation?.getCurrentPosition,
        clipboardOwnProperty: Object.getOwnPropertyDescriptor(navigator, 'clipboard') !== undefined,
        storageOwnProperty: Object.getOwnPropertyDescriptor(window, 'localStorage') !== undefined && Object.getOwnPropertyDescriptor(window, 'sessionStorage') !== undefined,
        localStorageIsMemory: typeof window.localStorage?.setItem === 'function',
        blocked: window.__demoBlocked ?? [],
      }
    })
    .catch((error) => ({ error: String(error) }))
  const guard = result.guardState
  check('guard: service worker replaced in the frame', guard.serviceWorkerOwnProperty === true && guard.serviceWorker === 'object', JSON.stringify(guard.serviceWorker ?? guard.error ?? null))
  check('guard: storage replaced by memory', guard.storageOwnProperty === true && guard.localStorageIsMemory === true, guard.error ?? '')
  check('guard: geolocation replaced', guard.geolocation === 'function', String(guard.geolocation ?? guard.error ?? ''))
  check('guard: clipboard replaced', guard.clipboardOwnProperty === true, guard.error ?? '')

  const labelVisible = await demo.evaluate((label) => document.body.innerText.includes(label), LABEL).catch(() => false)
  check('persistent demo label visible', labelVisible)

  const html = await demo.evaluate(() => document.documentElement.outerHTML).catch(() => '')
  const text = await demo.evaluate(() => document.body.innerText).catch(() => '')
  const forbiddenHits = FORBIDDEN_IN_FRAME.filter(([, pattern]) => pattern.test(html) || pattern.test(text)).map(([name]) => name)
  check('no forbidden text in the frame', forbiddenHits.length === 0, forbiddenHits.join(', '))

  // A product API path that no responder answers must not look like an empty answer.
  result.unmatchedApi = await demo
    .evaluate(async (path) => {
      try {
        const response = await fetch(path)
        return { status: response.status, body: (await response.text()).slice(0, 300) }
      } catch (error) {
        return { status: 0, body: String(error) }
      }
    }, UNMATCHED_API_PATH)
    .catch((error) => ({ status: 0, body: String(error) }))
  check(
    'dashboard: an unmatched /api path answers 404 demo_unavailable',
    result.unmatchedApi.status === 404 && result.unmatchedApi.body.includes(UNAVAILABLE_CODE),
    `${result.unmatchedApi.status} ${result.unmatchedApi.body.slice(0, 80)}`,
  )

  // Type into the composer and submit it through the composer's own form, as a visitor does.
  const submitted = await demo.evaluate(async (prompt) => {
    const composer = document.querySelector('textarea')
    if (!composer) return { outcome: 'no composer' }
    // Diagnostics: which forms receive submit events, and whether anything prevented them.
    window.__submitLog = []
    const record = (phase) => (event) => {
      window.__submitLog.push({ phase, prevented: event.defaultPrevented, form: event.target?.className?.toString().slice(0, 80) ?? '' })
    }
    document.addEventListener('submit', record('capture'), true)
    window.addEventListener('submit', record('window-bubble'))
    const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set
    setValue?.call(composer, prompt)
    composer.dispatchEvent(new Event('input', { bubbles: true }))
    const form = composer.closest('form')
    // The reactive runtime applies the input's effects on a later tick, so wait for the send button to enable.
    let button = null
    for (let attempt = 0; attempt < 60 && !button; attempt++) {
      await new Promise((resolve) => setTimeout(resolve, 50))
      button = form?.querySelector('button[type="submit"]:not([disabled])') ?? null
    }
    const buttons = Array.from(form?.querySelectorAll('button') ?? []).map((item) => ({
      type: item.type,
      disabled: item.disabled,
      label: item.getAttribute('aria-label'),
    }))
    if (!(button instanceof HTMLButtonElement)) {
      return { outcome: 'no enabled submit button', value: composer.value, inert: Boolean(form?.closest('[inert]')), buttons }
    }
    button.click()
    await new Promise((resolve) => setTimeout(resolve, 300))
    return { outcome: 'submitted', value: composer.value, buttons, submitLog: window.__submitLog }
  }, PROMPT)
  result.composerSubmit = submitted
  check('composer submitted the prompt', submitted.outcome === 'submitted', submitted.outcome)

  const answer = await demo
    .waitForFunction((source) => new RegExp(source.slice(1, source.lastIndexOf('/'))).test(document.body.innerText), DEMO_REPLY_SOURCE, { timeout: timeoutMs })
    .then(() => true, () => false)
  check('free-text prompt answered with the honest demo reply', answer)
  await page.screenshot({ path: join(outRoot, 'probe-desktop.png') })
  if (!answer) {
    result.lastFrameText = (await demo.evaluate(() => document.body.innerText).catch(() => '')).slice(0, 1200)
  }
}

/** Visit "links": refused and offered links, the browser address field, and the host protocol. */
async function runLinkVisit(browser, origin, result, timeoutMs, check) {
  const visit = await openVisit(browser, origin, timeoutMs)
  check('links: ready message from the demo', visit.ready)
  try {
    if (!visit.demo) {
      check('links: demo frame found', false)
    } else {
      const greeting = await waitForGreeting(visit.demo, timeoutMs)
      check('links: dashboard greeting visible', greeting)
      if (greeting) await linkSteps(visit, result, timeoutMs, check)
    }
  } finally {
    await visit.context.close()
  }
  assessNetwork('links', visit.watch, origin, check, result)
}

async function linkSteps(visit, result, timeoutMs, check) {
  const { page, demo } = visit
  const steps = { refused: [], routedChat: false, routedNoticeShown: true, omnibox: null, bridge: [], escape: false, reset: false }
  result.links = steps

  // Links the demo refuses: one that leaves the demo, and a product page it does not offer.
  for (const href of REFUSED_LINKS) {
    await waitForNoticeHidden(demo, 3500)
    await clickAnchor(demo, href)
    const shown = await waitForNotice(demo, LINK_NOTICE, 2000)
    steps.refused.push({ href, shown })
    check(`links: ${href} is refused with the notice`, shown)
  }

  // A link the demo offers is routed inside the demo: the chat opens, and no notice is shown.
  await waitForNoticeHidden(demo, 3500)
  await clickAnchor(demo, '/chat')
  steps.routedChat = await demo
    .waitForFunction(() => document.querySelector('textarea') !== null, undefined, { timeout: timeoutMs })
    .then(() => true, () => false)
  check('links: /chat opens inside the demo', steps.routedChat)
  steps.routedNoticeShown = await noticeVisible(demo)
  check('links: an offered link shows no notice', steps.routedNoticeShown === false)

  // Back to the dashboard, then to the dashboard's browser pane, where the address field is.
  await clickAnchor(demo, '/dashboard')
  check('links: /dashboard opens again', await waitForGreeting(demo, timeoutMs))
  await clickAnchor(demo, '/dashboard?tab=web')
  // The browser pane's toolbar shows once the web tab is open. The address field is inert in the demo: the
  // product enables it only with a live browser session, and the demo never starts one.
  const toolbar = await demo
    .waitForFunction(() => document.querySelector('[aria-label="Nettleserkontroller"]') !== null, undefined, { timeout: 10000 })
    .then(() => true, () => false)
  const fieldState = await demo.evaluate((selector) => {
    const element = document.querySelector(selector)
    if (!(element instanceof HTMLInputElement)) return 'absent'
    return element.disabled ? 'disabled' : 'enabled'
  }, ADDRESS_FIELD)
  steps.omnibox = { pane: toolbar, field: fieldState, blockedBefore: null, blockedAfter: null }
  check('omnibox: the browser address field is inert in the demo (absent or disabled)', fieldState !== 'enabled', fieldState)
  // Should the field ever be enabled, a typed address must still leave nothing blocked.
  if (fieldState === 'enabled') {
    steps.omnibox.blockedBefore = await demo.evaluate(() => (window.__demoBlocked ?? []).length)
    // Typed the way a visitor types it: the value, the input event, then Enter in the field.
    await demo.evaluate(
      ({ selector, query }) => {
        const element = document.querySelector(selector)
        if (!(element instanceof HTMLInputElement)) return
        const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set
        setValue?.call(element, query)
        element.dispatchEvent(new Event('input', { bubbles: true }))
        element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }))
      },
      { selector: ADDRESS_FIELD, query: ADDRESS_QUERY },
    )
    await new Promise((done) => setTimeout(done, 800))
    steps.omnibox.blockedAfter = await demo.evaluate(() => (window.__demoBlocked ?? []).length)
    check(
      'omnibox: a typed address leaves nothing blocked',
      steps.omnibox.blockedAfter === steps.omnibox.blockedBefore,
      `${steps.omnibox.blockedBefore} blocked before, ${steps.omnibox.blockedAfter} after`,
    )
  }

  // The host protocol: each message moves the demo to the state the host asked for.
  const transitions = [
    ['deactivate', 'inactive'],
    ['resume', 'inactive'],
    ['activate', 'active'],
    ['pause', 'paused'],
    ['resume', 'active'],
  ]
  for (const [type, expected] of transitions) {
    await postToDemo(page, { source: 'verevon-demo', v: 1, type })
    const reached = await demo
      .waitForFunction((value) => document.documentElement.dataset.demoState === value, expected, { timeout: 3000 })
      .then(() => true, () => false)
    steps.bridge.push({ type, expected, reached })
    check(`bridge: ${type} sets the demo state to ${expected}`, reached)
  }

  // Escape in the demo reaches the host.
  await demo.evaluate(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })))
  steps.escape = await page
    .waitForFunction(() => (window.__demoMessages ?? []).some((message) => message.type === 'escape'), undefined, { timeout: 3000 })
    .then(() => true, () => false)
  check('bridge: Escape in the demo reaches the host', steps.escape)

  // Reset: the demo loads again and announces ready a second time.
  await postToDemo(page, { source: 'verevon-demo', v: 1, type: 'reset' })
  steps.reset = await page
    .waitForFunction(() => (window.__demoMessages ?? []).filter((message) => message.type === 'ready').length >= 2, undefined, { timeout: timeoutMs })
    .then(() => true, () => false)
  check('bridge: reset reloads the demo, which announces ready again', steps.reset)
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  const outDir = resolve(args.dir ?? join(DEFAULT_WEB_ROOT, 'public', 'product-demo'))
  const manifestPath = resolve(args.manifest ?? join(DEFAULT_WEB_ROOT, 'plans', 'product-demo', 'manifest.json'))
  const outRoot = resolve(args.out ?? join(SPA_ROOT, 'node_modules', '.cache', 'product-demo', 'probe'))
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
  await mkdir(outRoot, { recursive: true })

  const { server, port } = await listenOnFreePort(makeHandler(outDir, manifest.hosting.headerRules))
  const origin = `http://127.0.0.1:${port}`
  const result = { ok: false, origin, outDir, checks: [], cold: null, guardState: null, unmatchedApi: null, composerSubmit: null, links: null, visits: {} }
  const check = (name, passed, detail = '') => result.checks.push({ name, passed: Boolean(passed), detail })
  const browser = await chromium.launch({ headless: true })
  try {
    await runDashboardVisit(browser, origin, manifest.budgets, result, outRoot, args.timeoutMs, check)
    await runLinkVisit(browser, origin, result, args.timeoutMs, check)
  } finally {
    await browser.close()
    await new Promise((done) => server.close(done))
  }
  result.ok = result.checks.length > 0 && result.checks.every((item) => item.passed)
  await writeFile(join(outRoot, 'probe.json'), `${JSON.stringify(result, null, 2)}\n`)
  for (const item of result.checks) {
    console.log(`${item.passed ? 'PASS' : 'FAIL'} ${item.name}${item.detail ? ` (${item.detail})` : ''}`)
  }
  console.log(`demo:probe: ${result.ok ? 'all checks passed' : 'failed'}; evidence in ${outRoot}`)
  process.exitCode = result.ok ? 0 : 1
}

main().catch((error) => {
  console.error(`demo:probe: ${error instanceof Error ? error.stack : String(error)}`)
  process.exitCode = 1
})
