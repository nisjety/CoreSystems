#!/usr/bin/env node
/**
 * Browser check of the live demo (stage 04, src/demo/live). It drives the real dashboard and chat controls in
 * headless Chromium, runs three customer cases to their decision, and fails on any leak. Each case is a
 * different customer issue (apps/verevon-web/plans/product-demo/CASES_2026-10-12.md):
 *   a) the dashboard card «Solhaug Hotell ber om tilbud» (the card chooses «Utfør») → Godkjenn
 *   b) free text «Hva er været i Bergen i morgen?» → the honest demo reply → its link
 *      «Reklamasjon fra Havnebrygga Kafé» → Avvis
 *   c) Ny samtale → the start-screen card for Nordhavn kommune → Godkjenn
 *
 * Each step scans the visible text for forbidden strings. Before each decision the run opens the technical activity
 * disclosure («Teknisk aktivitet») of the Work pane, so its rows are rendered, and scans the Work panes for English
 * product words (code, run ids and tool names are technical and are left out). On a phone the gate must be in view
 * as the visitor sees it, read from the page without any scroll: inside the transcript's visible area above the Work
 * pane, not covered by another element, and with the transcript at its end (the follow in suggestions-live.ts brings
 * it there; the run waits, bounded, for that). On a wider screen the gate's place is recorded as an observation only.
 * Then the gate is pressed (a trial press first, then the real one).
 *
 * The run fails on any request that leaves the page's origin, on any /api request that reaches the network (the
 * engine answers those in memory), on an uncaught page error or an unexpected console error, and on a page that stops
 * answering. A watchdog asks the page for a trivial value every few seconds: when it does not answer within five
 * seconds it records page-unresponsive and exits non-zero, rather than hang. Every wait has a timeout.
 *
 *   node scripts/demo-live-check.mjs [--url http://127.0.0.1:5301/product-demo/] [--out <dir>] [--viewport 1024x640]
 *
 * At 760 px wide or less the run uses phone emulation (isMobile, touch taps). Exit code 0 means every case
 * reached its decision and nothing leaked.
 */
import { chromium } from '@playwright/test'
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const spaRoot = path.resolve(here, '..')
const args = parseArgs(process.argv.slice(2))
const demoUrl = args.url ?? 'http://127.0.0.1:5301/product-demo/'
const outDir = path.resolve(args.out ?? path.join(spaRoot, 'test-results', 'demo-live-check'))
const [width, height] = (args.viewport ?? '1024x640').split('x').map(Number)
if (!(width > 0 && height > 0)) throw new Error(`--viewport needs WxH, got ${args.viewport}`)
const phone = width <= 760
const origin = new URL(demoUrl).origin

/** Every wait in this file is bounded by one of these, in ms. */
const TIMEOUT = { load: 30_000, short: 10_000, reply: 25_000, decision: 40_000, watchdog: 5_000, settle: 4_000 }
const WATCHDOG_EVERY_MS = 3_000

/** The visitor's words, as the demo shows them. */
const CASE_A_CARD = 'Solhaug Hotell ber om tilbud'
const FREE_TEXT = 'Hva er været i Bergen i morgen?'
const HONEST_REPLY = 'Dette er en demo med forhåndsdefinerte svar'
const CASE_B_HREF = '#demo:reklamasjon-bordplate'
const CASE_B_LINK = 'Reklamasjon fra Havnebrygga Kafé'
const CASE_C_CARD = 'Nordhavn kommune'
const APPROVED = 'Godkjent.'
const REJECTED = 'Avvist.'
const DEMO_LABEL = 'Demo · fiktive data'
const COMPOSER = 'textarea#verevon-dashboard-input'
const SEND = 'button[aria-label="Send melding"]'
const NEW_CONVERSATION = 'button[aria-label="Ny samtale"]'
const APPROVE = '.verevon-chat-approval__approve'
const REJECT = '.verevon-chat-approval__reject'
/** The transcript (the element that scrolls), the Work panes, and the technical activity disclosure of a run. */
const TRANSCRIPT = '.verevon-chat-message-list'
const WORK_PANES = '.verevon-chat-workspace-canvas, .verevon-chat-run-panel'
const ACTIVITY_DISCLOSURE = '.verevon-chat-run-activity-disclosure'

/** The owner's forbidden list. An e-mail address must end in @fjordform.example. */
const FORBIDDEN = [
  ['Nordlys', /nordlys/i],
  ['Bring', /\bbring\b/i],
  ['4821', /4821/],
  ['20471', /20471/],
  ['faktura', /faktura/i],
  ['forsinkelse', /forsinkelse/i],
  ['Aquatiq', /aquatiq/i],
  ['Velion', /velion/i],
  ['Ima', /\bIma\b/],
  ['e-mail outside @fjordform.example', /[A-Z0-9._%+-]+@(?!fjordform\.example\b)[A-Z0-9.-]+\.[A-Z]{2,}/i],
  ['real company', /\b(Visma|Tripletex|DHL|UPS|FedEx|Google|Microsoft|Amazon)\b/],
]
/**
 * English product words that must not show in the Work panes or in the technical activity disclosure (Norwegian
 * bokmål only). Words that also exist in Norwegian are left out («to», «is», «for»). Technical identifiers such as
 * SearchWeb, web_search or run ids do not match: a word boundary does not sit inside them.
 */
const ENGLISH_IN_WORK = /\b(work|workspace|output|sources?|trace|approval|approve|reject|rejected|running|pending|completed|cancelled|canceled|failed|waiting|steps?|draft|done|ready|retry|submit|cancel|save|tools?|connect|connected|stream|streaming|search|found|captured|verification|answer|compose|response|model|selected|delta|artifact|updated|created|message|call|text|the|and|with|from|your|this|that)\b|live agent|browser run/gi
/** The zod CSP probe the demo's policy refuses. It is benign, and only this many reports are tolerated (as in probe-product-demo.mjs). */
const BENIGN_EVAL_REPORT = /Refused to evaluate a string as JavaScript because 'unsafe-eval'/
const MAX_BENIGN_EVAL_REPORTS = 3

const failures = []
const checks = []
const checkpoints = []
/** Where each gate sat on a wider screen, recorded and not judged (see checkGateInView). */
const observations = []
const thirdPartyRequests = []
const apiRequests = []
const pageErrors = []
const consoleErrors = []
let browser = null
let finished = false

await mkdir(outDir, { recursive: true })

function check(name, passed, detail = '') {
  checks.push({ name, passed: Boolean(passed), detail })
  if (!passed) failures.push(detail ? `${name}: ${detail}` : name)
}

function withTimeout(promise, ms, label) {
  let timer
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`timeout ${ms} ms: ${label}`)), ms)
    }),
  ]).finally(() => clearTimeout(timer))
}

/** Writes the report, closes the browser (bounded) and exits. Only the first call has an effect. */
async function finish(code) {
  if (finished) return
  finished = true
  if (watchdogTimer) clearInterval(watchdogTimer)
  const report = { demoUrl, viewport: `${width}x${height}`, phone, checks, checkpoints, observations, failures, pageErrors, consoleErrors, thirdPartyRequests, apiRequests }
  await writeFile(path.join(outDir, 'report.json'), JSON.stringify(report, null, 2)).catch(() => undefined)
  console.log(JSON.stringify({ outDir, failures, checkpoints: checkpoints.map((item) => item.name), observations: observations.map((item) => ({ label: item.label, facts: item.facts })) }, null, 2))
  if (browser) await withTimeout(browser.close(), 5_000, 'browser close').catch(() => undefined)
  process.exit(code === 0 && failures.length === 0 ? 0 : 1)
}

/** Records the stop and exits. A page that does not answer must fail the run, not hang it. */
async function unresponsive(label) {
  failures.push(`page-unresponsive: no answer to a trivial evaluate within ${TIMEOUT.watchdog} ms (${label})`)
  await finish(1)
}

/** True when the page answers a trivial evaluate within the watchdog budget. */
async function answers() {
  try {
    await withTimeout(page.evaluate(() => 1), TIMEOUT.watchdog, 'page answer')
    return true
  } catch {
    return false
  }
}

async function ensureAlive(label) {
  if (!(await answers())) {
    await unresponsive(label)
    throw new Error(`page-unresponsive at ${label}`)
  }
}

/** A visitor's press: a tap on a phone, a click elsewhere. Each press has its own bounded timeout. */
function press(locator, options = {}) {
  const { timeout = TIMEOUT.short, trial = false } = options
  return phone ? locator.tap({ timeout, trial }) : locator.click({ timeout, trial })
}

/** How many times a text occurs in the visible page. */
function occurrences(needle) {
  return withTimeout(
    page.evaluate((text) => document.body.innerText.split(text).length - 1, needle),
    TIMEOUT.watchdog,
    'count text',
  )
}

/**
 * The Work panes as the visitor sees them: the rendered text of every visible pane. A pane without height is hidden,
 * so its text (for example a collapsed disclosure) is not in the scan until the visitor has opened it.
 */
function workRowsText() {
  return Array.from(document.querySelectorAll('.verevon-chat-workspace-canvas, .verevon-chat-run-panel'))
    .filter((pane) => pane instanceof HTMLElement && pane.getBoundingClientRect().height > 0)
    .map((pane) => pane.innerText)
    .join('\n')
}

/**
 * Where the gate sits, as the visitor sees it. It only reads: no scroll, no focus, no click. The gate must sit in the
 * transcript's visible area. On a phone the Work panes are a pane at the foot of the screen, so the transcript's
 * visible area ends where the highest of them starts.
 */
function gateFacts({ selector, transcriptSelector, paneSelector, phone: isPhone }) {
  const button = document.querySelector(selector)
  const transcript = document.querySelector(transcriptSelector)
  if (!(button instanceof HTMLElement) || !(transcript instanceof HTMLElement)) return { found: false }
  const rect = button.getBoundingClientRect()
  const area = transcript.getBoundingClientRect()
  const paneTops = Array.from(document.querySelectorAll(paneSelector))
    .filter((pane) => pane instanceof HTMLElement && pane.getBoundingClientRect().height > 0)
    .map((pane) => pane.getBoundingClientRect().top)
  const paneTop = isPhone && paneTops.length > 0 ? Math.min(...paneTops) : null
  const bottomLimit = paneTop === null ? area.bottom : Math.min(area.bottom, paneTop)
  const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2)
  return {
    found: true,
    inTranscript: rect.height > 0 && rect.top >= area.top - 1 && rect.bottom <= bottomLimit + 1,
    inWindow: rect.top >= 0 && rect.bottom <= window.innerHeight,
    uncovered: hit instanceof Element && (hit === button || button.contains(hit)),
    atEnd: transcript.scrollHeight - transcript.scrollTop - transcript.clientHeight <= 4,
    paneTop: paneTop === null ? null : Math.round(paneTop),
    gate: { top: Math.round(rect.top), bottom: Math.round(rect.bottom) },
    transcript: { top: Math.round(area.top), bottom: Math.round(area.bottom) },
  }
}

/**
 * The checks of one step: the visible text against the forbidden list, the Work panes against the English list,
 * no horizontal scroll (a phone has a 16px gutter and no sideways page scroll). Then a screenshot and the text.
 */
async function checkpoint(name) {
  await ensureAlive(`checkpoint ${name}`)
  const text = await withTimeout(page.evaluate(() => document.body.innerText), TIMEOUT.short, `text ${name}`)
  const hits = FORBIDDEN.filter(([, pattern]) => pattern.test(text)).map(([label]) => label)
  const work = await withTimeout(page.evaluate(workRowsText), TIMEOUT.short, `work rows ${name}`)
  const englishHits = [...new Set(work.match(ENGLISH_IN_WORK) ?? [])]
  // The text around each hit, so a failure shows where the word sits.
  const englishContext = [...work.matchAll(ENGLISH_IN_WORK)].slice(0, 4).map((match) => work.slice(Math.max(0, match.index - 40), match.index + match[0].length + 40).replace(/\s+/g, ' '))
  const overflow = await withTimeout(
    page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth),
    TIMEOUT.short,
    `overflow ${name}`,
  )
  checkpoints.push({ name, forbidden: hits, englishInWork: englishHits, englishContext, overflow })
  check(`${name}: no forbidden text`, hits.length === 0, hits.join(', '))
  check(`${name}: no English product words in the Work panes`, englishHits.length === 0, `${englishHits.join(', ')} | ${englishContext.join(' / ')}`)
  check(`${name}: no horizontal page scroll`, overflow <= 0, `${overflow}px`)
  await page.screenshot({ path: path.join(outDir, `${name}.png`), timeout: TIMEOUT.short })
  await writeFile(path.join(outDir, `${name}.txt`), text)
  return text
}

/**
 * Opens the «Teknisk aktivitet» disclosure of the run, so its rows are rendered and the English scan reads them. The
 * disclosure must be on the page: a run without it would leave those rows unchecked, which the check records.
 */
async function openActivityDisclosure(label) {
  // A turn shows its Resultat tab at the gate, in the run panel or the canvas. The disclosure lives under the Arbeid tab, so show that first.
  await press(page.locator('.verevon-chat-run-panel [role="tab"]:has-text("Arbeid"), .verevon-chat-workspace-canvas [role="tab"]:has-text("Arbeid")').first(), { timeout: TIMEOUT.short }).catch(() => undefined)
  const state = await evaluateBounded((selector) => {
    const details = document.querySelector(selector)
    return details instanceof HTMLDetailsElement ? { open: details.open } : null
  }, ACTIVITY_DISCLOSURE)
  check(`${label}: the technical activity disclosure is on the page`, state !== null, ACTIVITY_DISCLOSURE)
  if (state === null) return
  if (!state.open) {
    const opened = await press(page.locator(`${ACTIVITY_DISCLOSURE} > summary`).first(), { timeout: TIMEOUT.short }).then(() => true, () => false)
    check(`${label}: the technical activity disclosure opens`, opened, ACTIVITY_DISCLOSURE)
  }
  const rows = await page
    .locator(`${ACTIVITY_DISCLOSURE} .verevon-chat-run-activity`)
    .first()
    .waitFor({ state: 'attached', timeout: TIMEOUT.short })
    .then(() => true, () => false)
  check(`${label}: the technical activity rows are rendered for the scan`, rows, ACTIVITY_DISCLOSURE)
}

/**
 * The gate as the visitor sees it, read before any press. On a phone the Work pane covers the lower part of the screen,
 * so the gate must sit in the transcript's visible area above that pane, must not be covered by another element at its
 * centre, and the transcript must be at its end (the follow in suggestions-live.ts brings it there). The check never
 * scrolls to the gate. It waits, bounded, for the follow to settle, and reads the facts again until they hold.
 * A wider screen has no pane over the transcript (its gate is pressed with a trial press, which is the check there),
 * so its facts are recorded as observations and not judged.
 */
async function checkGateInView(label, selector) {
  const gateArgs = { selector, transcriptSelector: TRANSCRIPT, paneSelector: WORK_PANES, phone }
  let facts = await evaluateBounded(gateFacts, gateArgs)
  if (!phone) {
    observations.push({ label, facts })
    return
  }
  const settleUntil = Date.now() + TIMEOUT.settle
  while (Date.now() < settleUntil && !(facts?.found && facts.inTranscript && facts.inWindow && facts.uncovered && facts.atEnd)) {
    await new Promise((done) => setTimeout(done, 250))
    facts = await evaluateBounded(gateFacts, gateArgs)
  }
  const detail = JSON.stringify(facts)
  check(`${label}: the gate is in the transcript's visible area`, Boolean(facts?.found && facts.inTranscript && facts.inWindow), detail)
  check(`${label}: nothing covers the gate`, Boolean(facts?.found && facts.uncovered), detail)
  check(`${label}: the transcript is at its end`, Boolean(facts?.found && facts.atEnd), detail)
}

/** Presses a gate's decision button after it can be reached, and waits for the decision text it adds. */
async function decide(label, selector, text) {
  await ensureAlive(`${label} gate`)
  const button = page.locator(selector).first()
  await button.waitFor({ state: 'visible', timeout: TIMEOUT.decision })
  await openActivityDisclosure(label)
  await checkpoint(`${label}-gate`)
  await checkGateInView(label, selector)
  const reachable = await press(button, { trial: true, timeout: TIMEOUT.short }).then(() => true, () => false)
  check(`${label}: the gate can be reached (hit test)`, reachable, selector)
  if (!reachable) throw new Error(`${label}: the gate is covered`)
  const before = await occurrences(text)
  await press(button, { timeout: TIMEOUT.short })
  const decided = await page
    .waitForFunction(([needle, count]) => document.body.innerText.split(needle).length - 1 > count, [text, before], { timeout: TIMEOUT.reply })
    .then(() => true, () => false)
  check(`${label}: the decision text appears`, decided, text)
  if (!decided) throw new Error(`${label}: no "${text}" after the press`)
}

/** A page read with the watchdog's budget, so a frozen page fails the step instead of holding it. */
function evaluateBounded(fn, arg) {
  return withTimeout(page.evaluate(fn, arg), TIMEOUT.watchdog, 'page read')
}

/** Routes inside the demo to the chat page, the way a visitor's link does (the demo's own link policy). */
async function openChat(label) {
  await ensureAlive(`${label} open chat`)
  await evaluateBounded((href) => {
    const anchor = document.createElement('a')
    anchor.setAttribute('href', href)
    anchor.textContent = 'chat'
    document.body.append(anchor)
    anchor.click()
    anchor.remove()
  }, '/chat')
  await page.locator('.verevon-chat-page').first().waitFor({ state: 'attached', timeout: TIMEOUT.load })
}

/**
 * Starts a new conversation: the chat page first when the visitor is on the dashboard, then «Ny samtale» when a
 * conversation with messages is open. The start screen (an empty conversation) has no «Ny samtale» to press.
 */
async function startNewConversation(label) {
  const onChatPage = await evaluateBounded(() => Boolean(document.querySelector('.verevon-chat-page')))
  if (!onChatPage) await openChat(label)
  const startShown = await evaluateBounded(() => {
    const heading = document.querySelector('.verevon-chat-empty__heading')
    return Boolean(heading && heading.getBoundingClientRect().height > 0)
  })
  if (!startShown) await press(page.locator(NEW_CONVERSATION).first(), { timeout: TIMEOUT.short })
  await page.locator('.verevon-chat-empty__heading').first().waitFor({ state: 'visible', timeout: TIMEOUT.reply })
}

/** Case a: the dashboard card, Utfør chosen by the card, run to the gate, Godkjenn. */
async function caseA() {
  await ensureAlive('a start')
  await page.waitForSelector(COMPOSER, { timeout: TIMEOUT.load })
  const card = page.locator('.v-suggestion', { hasText: CASE_A_CARD }).first()
  await card.waitFor({ state: 'visible', timeout: TIMEOUT.load })
  const dashboardText = await checkpoint('dashboard')
  check('dashboard: the demo label is visible', dashboardText.includes(DEMO_LABEL), DEMO_LABEL)
  await press(card)
  // Utfør exists only in the chat composer, so the card opens the chat page with the question in it and Utfør chosen.
  await page.locator('.verevon-chat-page').first().waitFor({ state: 'attached', timeout: TIMEOUT.reply })
  const filled = await page
    .waitForFunction((needle) => (document.querySelector('textarea#verevon-dashboard-input')?.value ?? '').includes(needle), CASE_A_CARD, { timeout: TIMEOUT.reply })
    .then(() => true, () => false)
  check('a: the card opens the chat with its question in the composer', filled, CASE_A_CARD)
  const doPressed = await evaluateBounded(() => {
    const segment = Array.from(document.querySelectorAll('.dashboard-composer-intent-segment button'))
    const doButton = segment.find((button) => button.textContent?.trim() === 'Utfør')
    return doButton ? doButton.getAttribute('aria-pressed') : null
  })
  check('a: the card chooses Utfør', doPressed === 'true', `aria-pressed ${doPressed}`)
  await checkpoint('a-card')
  await press(page.locator(SEND).first())
  await decide('a', APPROVE, APPROVED)
  await checkpoint('a-approved')
}

/** Case b: free text gets the honest reply; its link starts the Havnebrygga case; Avvis. */
async function caseB() {
  await startNewConversation('b')
  await page.fill(COMPOSER, FREE_TEXT, { timeout: TIMEOUT.short })
  await press(page.locator(SEND).first())
  const honest = await page
    .waitForFunction((needle) => document.body.innerText.includes(needle), HONEST_REPLY, { timeout: TIMEOUT.reply })
    .then(() => true, () => false)
  check('b: the free text gets the honest demo reply', honest, HONEST_REPLY)
  await checkpoint('b-fallback')
  const link = page.locator(`a[href="${CASE_B_HREF}"]`, { hasText: CASE_B_LINK }).first()
  await link.waitFor({ state: 'visible', timeout: TIMEOUT.reply })
  await press(link)
  await decide('b', REJECT, REJECTED)
  await checkpoint('b-rejected')
}

/** Case c: a new conversation, the start-screen card for Nordhavn kommune, Godkjenn. */
async function caseC() {
  await startNewConversation('c')
  const card = page.locator('.v-suggestion', { hasText: CASE_C_CARD }).first()
  await card.waitFor({ state: 'visible', timeout: TIMEOUT.reply })
  await press(card)
  const composer = await withTimeout(page.inputValue(COMPOSER, { timeout: TIMEOUT.short }), TIMEOUT.short, 'composer')
  check('c: the start-screen card fills the composer', composer.includes(CASE_C_CARD), composer.slice(0, 60))
  await press(page.locator(SEND).first())
  await decide('c', APPROVE, APPROVED)
  await checkpoint('c-approved')
}

/** Runs one case. A stopped case is recorded and the next case still runs (the watchdog ends the run on its own). */
async function runCase(label, body) {
  try {
    await body()
  } catch (error) {
    const message = String(error?.message ?? error).split('\n')[0]
    failures.push(`${label}: stopped: ${message}`)
    await checkpoint(`${label}-stopped`).catch(() => undefined)
  }
}

let page = null
let watchdogTimer = null
try {
  browser = await withTimeout(chromium.launch({ headless: true }), TIMEOUT.load, 'browser launch')
  const context = await withTimeout(
    browser.newContext({
      viewport: { width, height },
      isMobile: phone,
      hasTouch: true,
      locale: 'nb-NO',
      timezoneId: 'Europe/Oslo',
      reducedMotion: 'reduce',
      colorScheme: 'light',
    }),
    TIMEOUT.load,
    'new context',
  )
  page = await withTimeout(context.newPage(), TIMEOUT.load, 'new page')
  // The watchdog starts once there is a page to ask. It runs for the rest of the run and ends with it.
  watchdogTimer = setInterval(() => {
    answers().then((alive) => {
      if (!alive && !finished) void unresponsive('watchdog')
    })
  }, WATCHDOG_EVERY_MS)
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (url.protocol === 'data:' || url.protocol === 'blob:') return
    if (url.origin !== origin) thirdPartyRequests.push(request.url())
    if (url.pathname.startsWith('/api')) apiRequests.push(request.url())
  })
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  page.on('pageerror', (error) => pageErrors.push(String(error)))
  page.on('crash', () => {
    failures.push('page-crashed')
    void finish(1)
  })

  await page.goto(demoUrl, { waitUntil: 'domcontentloaded', timeout: TIMEOUT.load })
  check('the live engine is installed in the page', await evaluateBounded(() => document.documentElement.classList.contains('verevon-demo-live')), 'verevon-demo-live')

  await runCase('a', caseA)
  await runCase('b', caseB)
  await runCase('c', caseC)
} catch (error) {
  failures.push(`flow stopped: ${String(error?.message ?? error).split('\n')[0]}`)
} finally {
  // A failed run keeps the page as it was when the run stopped, for the evidence.
  if (page && !finished && failures.length > 0) await checkpoint('failure').catch(() => undefined)
  const benign = consoleErrors.filter((text) => BENIGN_EVAL_REPORT.test(text))
  const realConsoleErrors = consoleErrors.filter((text) => !BENIGN_EVAL_REPORT.test(text))
  if (thirdPartyRequests.length > 0) failures.push(`third-party requests: ${thirdPartyRequests.slice(0, 5).join(', ')}`)
  if (apiRequests.length > 0) failures.push(`api requests reached the network: ${apiRequests.slice(0, 5).join(', ')}`)
  if (pageErrors.length > 0) failures.push(`uncaught page errors: ${pageErrors.slice(0, 2).join(' | ')}`)
  if (realConsoleErrors.length > 0) failures.push(`console errors: ${realConsoleErrors.slice(0, 3).join(' | ')}`)
  if (benign.length > MAX_BENIGN_EVAL_REPORTS) failures.push(`${benign.length} benign CSP eval reports (at most ${MAX_BENIGN_EVAL_REPORTS})`)
  await finish(0)
}

function parseArgs(argv) {
  const parsed = {}
  for (let index = 0; index < argv.length; index += 1) {
    const key = argv[index]
    if (key?.startsWith('--')) {
      parsed[key.slice(2)] = argv[index + 1]
      index += 1
    }
  }
  return parsed
}
