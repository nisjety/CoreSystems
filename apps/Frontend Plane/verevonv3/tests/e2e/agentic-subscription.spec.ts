import { expect, test, type Page, type Request as PwRequest, type TestInfo } from '@playwright/test'
import { appendFileSync, mkdirSync } from 'node:fs'
import { dirname, isAbsolute, relative, resolve } from 'node:path'
import { PRODUCT_MODEL, PRODUCT_PROVIDER, requireProductSubscription } from './product-subscription-route'

/**
 * A6: live proof of Do mode (an agentic run on execution-core's governed loop)
 * on the user's ChatGPT Terra subscription route.
 *
 * Run ONE probe at a time and stop at the first failure (each title starts
 * with its probe id; the describe is serial and has retries off, so a failure
 * also skips the probes after it):
 *
 *   cd "apps/Frontend Plane/verevonv3"
 *   npx playwright test --project=product-readiness tests/e2e/agentic-subscription.spec.ts --grep P1
 *
 * P0, P1 and P2 are free: they never reach a Codex turn (P0 sends nothing and
 * opens no page). PK, P3, P4 and P5 each make exactly ONE chat request on Terra
 * through the composer, behind requireProductSubscription, and never retry. A
 * subscription usage limit stops the probe with an explicit message.
 *
 * Playwright's --grep is a case-insensitive regex over the whole title path, so
 * `--grep P3` selects P3 only because no other title contains "p3": PK is named
 * so that it cannot be picked up by P1 to P5 (and the reverse). Keep that true
 * for any probe added later; `playwright test --list --grep <id>` shows it.
 *
 * A refused tool is not tool evidence. A call that capability policy refuses is
 * still recorded as a durable run step, so steps_completed >= 1 does not show
 * that a tool ran (the first live run of P3 passed that way while the model's
 * own answer said, in Norwegian, that both searches were refused by the access
 * policy). A probe that claims tool use (PK, P3, P5) therefore also classifies
 * the last assistant message of the thread, read from GET
 * /api/v1/chat/threads/<thread_id>/messages (the streamed chunks if that cannot
 * be read), into three values and keeps nothing else: refusal_hint (the answer
 * contains a refusal word such as avvist, policy, denied or utilgjengelig),
 * url_count (https links) and the answer length. The text is never stored,
 * printed or put in an assertion message. P3 needs refusal_hint false AND
 * url_count >= 1 (a web result cites a source URL). This is a word list, not a
 * proof: an honest answer that happens to say "policy" fails closed, and an
 * answer can cite a URL after a refusal, so the ids in the evidence line still
 * go to the tool audit. P4 is exempt: its proof is the plan_proposed frame, and
 * a plan for a customer follow-up can legitimately say "avvist".
 *
 * PK (knowledge search only) is the narrow half of P3: one short prompt that
 * asks the assistant to search the user's own knowledge base for a topic and
 * say in one sentence how many sources it found. It passes when the stream
 * finished with done, carried no error frame, refusal_hint is false and the
 * durable run recorded at least one step (a model that answers without calling
 * the tool has nothing to refuse, so a clean answer alone would pass a probe
 * that never searched). It CANNOT prove that retrieval returned results, only
 * that a call was attempted and the answer does not report it refused. On the
 * first live run of PK (and of P3) also confirm, from the run
 * id and request id in the evidence line, that session-core's tool audit shows
 * knowledge_search (and web_search for P3) as allowed and executed, and that
 * the execution-core log shows no capability-policy refusal for them.
 *
 * Evidence: one JSON line per probe is appended to $AGENTIC_PROOF_EVIDENCE
 * (default test-results/agentic-subscription-evidence.jsonl). It holds ids,
 * counts, timings and error codes only. No prompt, answer, message, token,
 * cookie or connection id is ever written or printed: every string passes
 * through scrub(), which drops anything that is not an identifier-shaped token.
 * That covers the evidence file and the console only: when a probe FAILS,
 * Playwright itself saves a screenshot and an error-context.md page snapshot of
 * the rendered chat (prompt and answer) under test-results, and no per-spec
 * setting turns the snapshot off. Delete those folders before sharing a run.
 * Playwright clears test-results at the start of every run unless it is given
 * --preserve-output, so point AGENTIC_PROOF_EVIDENCE somewhere else (or pass
 * that flag) when the probes are run one by one.
 *
 * Wire facts this spec relies on, read from the code on 2026-10-01 (the first
 * live run is what confirms them):
 *
 * - The agentic refusal for a disabled switch is decided in
 *   model-gateway normalize_stream, but invoke_stream_sse turns it into a
 *   streamed `error` frame (error_stream), so the HTTP status is 200
 *   text/event-stream, not a 400. It is the only frame, and it comes before
 *   StartRun, so there is no `connected` frame and no run.
 * - A Do run's chat stream carries `connected` (run_id, thread_id,
 *   request_id), `step_update`, `chunk`, `usage`, `plan_proposed`, `done` or
 *   `error`/`awaiting_approval`. `done` spells the model `modelUsed`.
 * - The run's orchestration tail reaches the chat stream only as
 *   `step_update` frames (orchestration_event_to_step_update): a subagent
 *   attach is `Subagent · <role>` / status running, a stop is `Subagent` /
 *   status done. The raw `subagent_attached` / `subagent_stopped` names exist
 *   only on GET /api/v1/runs/:id/events, which the live run panel opens, so
 *   that stream is captured as secondary evidence.
 * - Tool calls (knowledge_search, web_search) are recorded as durable run
 *   steps, not as stream frames, so tool use is corroborated from the durable
 *   run's steps_completed instead of a tool step_update. A refused call is
 *   recorded as a step too, so steps_completed only says that a call was
 *   attempted; it is necessary for P3 and never sufficient (see the refusal
 *   classifier above). A Plan step_update
 *   exists only when the model calls plan.propose (session-core broadcasts plan
 *   transitions on propose/transition, not on entering plan mode), so a run
 *   that researches and answers without proposing a plan has none.
 * - `done.modelUsed` and every chunk's `model_used` are the REQUESTED model,
 *   echoed by agentic_run_stream. They show the request was not rewritten on
 *   the way to the gateway; they say nothing about what served the rounds.
 *   Which provider and model served them is proved only by the inference-core
 *   log (provider=openai-codex-subscription for every round of the request id)
 *   and the cost-core ledger, which the runbook checks from the ids recorded
 *   here.
 * - A stopped subagent's step_update always has status `done`; its detail is
 *   `completed` or `failed` (execution-core's announce_delegated_child_stopped).
 */

const DISABLED_CODE = 'agentic_subscription_disabled'
const USAGE_LIMIT_CODE = 'subscription_usage_limit'
const RATE_LIMITED_CODE = 'rate_limited'
// A first round that cannot start: the route is pinned but the connection does
// not resolve, so no Codex turn runs. inference-core types a failed round of a
// pinned run as one of these.
const P2_TYPED_CODES = ['subscription_route_unavailable', 'subscription_reauth_required', 'subscription_bad_request']
// Never a real connection. P1 and P2 use it so that a switch left in the wrong
// state can start a run that dies before Codex, never one that spends quota.
const PLACEHOLDER_CONNECTION = 'a6-probe-no-such-connection'
const DEFAULT_EVIDENCE = 'test-results/agentic-subscription-evidence.jsonl'

const FREE_TEST_TIMEOUT_MS = 90_000
const P2_TEST_TIMEOUT_MS = 150_000
const TERRA_TEST_TIMEOUT_MS = 270_000
// Everything a Terra probe does, from the start of its body, must be over by
// here so that evidence is written before Playwright's own timeout fires.
const TERRA_BODY_BUDGET_MS = 258_000
const DURABLE_READ_TIMEOUT_MS = 8_000
const DURABLE_READ_MIN_REMAINING_MS = 20_000
// The persisted assistant message can land a moment after `done`.
const ANSWER_READ_ATTEMPTS = 3
const ANSWER_READ_RETRY_MS = 1_500

// Any of these in the final answer means the model is telling the user that a
// tool was refused or is unavailable. A word list: it fails closed on an honest
// answer that happens to say "policy", and it proves nothing when it stays
// quiet. Spaces are \s+ so that a line break or a non-breaking space still hits.
const REFUSAL_HINT = /tilgangspolic|avvist|ikke\s+tilgang|nektet|policy|denied|refused|not\s+permitted|not\s+allowed|unavailable|utilgjengelig|blokkert|blocked|ikke\s+tillatt|ikke\s+tilgjengelig|not\s+available|forbudt|forbidden|avslått|avslo|rejected|unauthori[sz]ed|uautorisert/i
// A link needs a dotted host with a letters-only top-level domain, so a bare
// scheme and the "https://..." placeholder that the P3 prompt spells out (which
// a refusing model may echo back) are not counted as a cited source.
const URL_PATTERN = /https?:\/\/(?:[\p{L}\p{N}-]+\.)+\p{L}{2,}(?::\d+)?(?:[/?#][^\s<>()[\]"'`]*)?/giu

// Words only: no personal data, nothing the account could regret sending.
const P3_PROMPT = 'Bruk kunnskapsbasen min og søk på nett. Finn to korte fakta om norsk lakseoppdrett, for eksempel eksportverdi og antall sysselsatte, og svar med to punkter og én kilde per punkt, med full nettadresse (https://...) til kilden.'
// Knowledge search only: no web search is asked for. One short sentence back.
const PK_PROMPT = 'Søk i min egen kunnskapsbase etter temaet lakseoppdrett. Ikke søk på nett. Si i én setning hvor mange kilder du fant.'
const P4_PROMPT = 'Lag en plan i tre steg for å forberede en kundeoppfølging etter et salgsmøte. Foreslå planen formelt med plan-verktøyet, og ikke utfør noe av den.'
const P5_ITEMS = ['Oslo', 'Bergen', 'Trondheim']
const P5_PROMPT = 'Undersøk tre byer hver for seg og parallelt med research_fan_out: Oslo, Bergen og Trondheim. For hver by: folketall og én kjent severdighet. Slå resultatene sammen til en kort oppsummering med én linje per by.'

type Probe = 'P1' | 'P2' | 'PK' | 'P3' | 'P4' | 'P5'
type Scalar = string | number | boolean | null
type EvidenceValue = Scalar | Scalar[] | Record<string, Scalar>
type Evidence = {
  probe: Probe
  ok: boolean
  at: string
  run_id: string | null
  thread_id: string | null
  request_id: string | null
  http_status: number | null
  frames: Record<string, number>
  elapsed_ms: number | null
  error_code: string | null
  failed_stage: string | null
  detail: Record<string, EvidenceValue>
}

type Frame = { event: string; id: string | null; data: Record<string, unknown> | null; endOffset: number }
type StreamRecord = { status: number; startedAt: number; headersAt: number; endAt: number | null; failed: boolean; chunks: Array<{ at: number; text: string }> }
type CaptureSnapshot = { clickAt: number | null; chat: StreamRecord[]; runs: StreamRecord[] }
type CaptureWindow = typeof window & { a6Capture?: CaptureSnapshot & { restore: () => void } }

const isRecord = (value: unknown): value is Record<string, unknown> => Boolean(value) && typeof value === 'object' && !Array.isArray(value)
const str = (value: unknown): string | undefined => (typeof value === 'string' ? value : undefined)
const num = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0)

function countBy<T>(items: readonly T[], key: (item: T) => string): Record<string, number> {
  const out: Record<string, number> = {}
  for (const item of items) {
    const name = key(item)
    out[name] = (out[name] ?? 0) + 1
  }
  return out
}

// ---------------------------------------------------------------------------
// Evidence. Identifier-shaped strings, numbers and booleans only.
// ---------------------------------------------------------------------------

const SAFE_TOKEN = /^[A-Za-z0-9_.:+\-/]{0,100}$/

function scrub(value: unknown): unknown {
  if (typeof value === 'string') return SAFE_TOKEN.test(value) ? value : '[omitted]'
  if (Array.isArray(value)) return value.map(scrub)
  if (isRecord(value)) {
    return Object.fromEntries(Object.entries(value).filter(([key]) => SAFE_TOKEN.test(key)).map(([key, entry]) => [key, scrub(entry)]))
  }
  return value
}

function writeEvidence(info: TestInfo, evidence: Evidence) {
  const path = resolve(process.env.AGENTIC_PROOF_EVIDENCE?.trim() || DEFAULT_EVIDENCE)
  const line = JSON.stringify(scrub(evidence))
  try {
    mkdirSync(dirname(path), { recursive: true })
    appendFileSync(path, `${line}\n`)
  } catch (error) {
    console.log(`[A6 ${evidence.probe}] evidence file not written (${error instanceof Error ? error.name : 'error'})`)
  }
  const fromOutputDir = relative(resolve(info.project.outputDir), path)
  if (!fromOutputDir.startsWith('..') && !isAbsolute(fromOutputDir)) {
    console.log(`[A6 ${evidence.probe}] the evidence file is inside Playwright's output directory, which is cleared on every run unless --preserve-output is given; set AGENTIC_PROOF_EVIDENCE to keep it`)
  }
  console.log(`[A6 ${evidence.probe}] ${line}`)
}

function newEvidence(probe: Probe): Evidence {
  return {
    probe, ok: false, at: new Date().toISOString(), run_id: null, thread_id: null, request_id: null,
    http_status: null, frames: {}, elapsed_ms: null, error_code: null, failed_stage: null, detail: {},
  }
}

function failureClass(error: unknown): string {
  const message = error instanceof Error ? error.message : ''
  if (message.startsWith('STOP')) return 'stop'
  if (/timeout/i.test(message)) return 'timeout'
  return 'assertion_or_error'
}

/** Runs a probe body; the evidence line is written whether it passes or not. */
async function runProbe(info: TestInfo, probe: Probe, body: (evidence: Evidence, stage: (name: string) => void) => Promise<void>) {
  const evidence = newEvidence(probe)
  const startedAt = Date.now()
  let current = 'start'
  try {
    await body(evidence, name => { current = name })
    evidence.ok = true
  } catch (error) {
    evidence.failed_stage = current
    evidence.detail.failure = failureClass(error)
    throw error
  } finally {
    evidence.detail.total_ms = Date.now() - startedAt
    writeEvidence(info, evidence)
  }
}

// ---------------------------------------------------------------------------
// SSE parsing. Frame content is read for decisions and never printed.
// ---------------------------------------------------------------------------

function parseSse(text: string): Frame[] {
  const frames: Frame[] = []
  const push = (block: string, endOffset: number) => {
    let event: string | null = null
    let id: string | null = null
    const data: string[] = []
    for (const raw of block.split('\n')) {
      const line = raw.endsWith('\r') ? raw.slice(0, -1) : raw
      if (!line || line.startsWith(':')) continue
      const colon = line.indexOf(':')
      const field = colon < 0 ? line : line.slice(0, colon)
      const value = colon < 0 ? '' : line.slice(colon + 1).replace(/^ /, '')
      if (field === 'event') event = value
      else if (field === 'id') id = value
      else if (field === 'data') data.push(value)
    }
    if (event === null && data.length === 0) return
    let parsed: unknown = null
    try { parsed = JSON.parse(data.join('\n')) } catch { parsed = null }
    frames.push({ event: event ?? 'message', id, data: isRecord(parsed) ? parsed : null, endOffset })
  }
  const boundary = /\r?\n\r?\n/g
  let start = 0
  for (let match = boundary.exec(text); match; match = boundary.exec(text)) {
    push(text.slice(start, match.index), match.index)
    start = match.index + match[0].length
  }
  if (start < text.length) push(text.slice(start), text.length)
  return frames
}

/** When each frame reached the page, from the chunk its last byte arrived in. */
function frameArrivals(chunks: StreamRecord['chunks'], frames: Frame[]): number[] {
  const bounds: number[] = []
  let total = 0
  for (const chunk of chunks) { total += chunk.text.length; bounds.push(total) }
  return frames.map(frame => {
    const index = bounds.findIndex(bound => bound >= frame.endOffset)
    return chunks[index < 0 ? chunks.length - 1 : index]?.at ?? Number.NaN
  })
}

/** The class of an orchestration step_update, from the server's fixed titles. */
function stepClass(frame: Frame): string {
  const title = str(frame.data?.title) ?? ''
  const status = str(frame.data?.status) ?? ''
  if (title === 'Subagent') return status === 'done' ? 'subagent_stopped' : 'subagent_other'
  if (title.startsWith('Subagent')) return status === 'running' ? 'subagent_attached' : 'subagent_other'
  const known = ['Plan', 'Step', 'Approval', 'Paused', 'Resumed', 'Browser', 'Approval required', 'Approval decided', 'Verified']
  return known.includes(title) ? title.toLowerCase().replace(/ /g, '_') : 'other'
}

const errorFrames = (frames: Frame[]) => frames.filter(frame => frame.event === 'error')

/** An error frame's code: top level on the gateway's frames, nested under `error` on the BFF's transport-loss frame. */
function errorCode(frame: Frame | undefined): string | undefined {
  const data = frame?.data
  if (!data) return undefined
  return str(data.code) ?? (isRecord(data.error) ? str(data.error.code) : undefined)
}

// ---------------------------------------------------------------------------
// Direct requests (P1, P2) and read-only lookups.
// ---------------------------------------------------------------------------

/** The body the composer sends for Do on the subscription route, as buildChatWireBody builds it. */
function doBody(content: string, connectionId: string) {
  return {
    content,
    model: PRODUCT_MODEL,
    provider: PRODUCT_PROVIDER,
    subscription_connection_id: connectionId,
    profile: 'chat',
    browse_web: false,
    generate_image: false,
    plan_mode: true,
    deep_research: false,
    attachments: [],
    features: ['usage', 'citations', 'reasoning', 'steps', 'artifacts', 'memory', 'tools', 'agentic'],
    tools: [],
    zdr: false,
    regenerated: false,
    edited_resubmit: false,
  }
}

/**
 * The direct POST of P1 and P2. These go through page.request, which the
 * page.route guard of requireProductSubscription never sees, so what they may
 * send is fixed here instead: Terra on the subscription route, the placeholder
 * connection (never the account's real one), and a fixed prompt with no
 * content. A switch left in the wrong state can then start a run that dies
 * before Codex, never one that spends quota or reaches another provider.
 */
async function postDirect(page: Page, timeoutMs: number) {
  const startedAt = Date.now()
  const response = await page.request.post('/api/v1/chat/stream', {
    data: doBody('A6 probe. No answer is needed.', PLACEHOLDER_CONNECTION),
    headers: { Accept: 'text/event-stream' },
    timeout: timeoutMs,
    failOnStatusCode: false,
  })
  const text = await response.text()
  return { status: response.status(), contentType: response.headers()['content-type'] ?? '', frames: parseSse(text), elapsedMs: Date.now() - startedAt }
}

async function readCatalogue(page: Page) {
  const response = await page.request.get('/api/v1/models', { timeout: 30_000 })
  expect(response.ok(), `GET /api/v1/models answered ${response.status()}`).toBe(true)
  const body: unknown = await response.json()
  const payload = isRecord(body) && 'data' in body ? body.data : body
  const list = Array.isArray(payload) ? payload : isRecord(payload) && Array.isArray(payload.models) ? payload.models : []
  const models = list.filter(isRecord).map(item => {
    const features = Array.isArray(item.features) ? item.features : Array.isArray(item.capabilities) ? item.capabilities : []
    return { id: str(item.id) ?? '', provider: str(item.provider) ?? '', features: features.filter((feature): feature is string => typeof feature === 'string') }
  })
  const subscription = models.filter(model => model.provider === PRODUCT_PROVIDER)
  return { subscription, terra: subscription.find(model => model.id === PRODUCT_MODEL) ?? null }
}

async function readJson(page: Page, path: string, timeout: number): Promise<Record<string, unknown> | null> {
  try {
    const response = await page.request.get(path, { timeout })
    if (!response.ok()) return null
    const body: unknown = await response.json()
    const payload = isRecord(body) && isRecord(body.data) ? body.data : body
    return isRecord(payload) ? payload : null
  } catch {
    return null
  }
}

/** The durable run record: a corroborating read, never the only evidence. */
async function readRun(page: Page, runId: string) {
  const payload = await readJson(page, `/api/v1/agents/runs/${encodeURIComponent(runId)}`, DURABLE_READ_TIMEOUT_MS)
  const run = payload && isRecord(payload.run) ? payload.run : null
  if (!run) return null
  return { status: str(run.status) ?? 'unknown', mode: str(run.mode) ?? 'unknown', stepsCompleted: num(run.steps_completed ?? run.stepsCompleted) }
}

async function readChildRunCount(page: Page, threadId: string, runId: string): Promise<number | null> {
  const payload = await readJson(page, `/api/v1/agents/runs?thread_id=${encodeURIComponent(threadId)}&limit=100`, DURABLE_READ_TIMEOUT_MS)
  const runs = payload && Array.isArray(payload.runs) ? payload.runs : null
  if (!runs) return null
  return runs.filter(run => isRecord(run) && (str(run.parent_run_id) ?? str(run.parentRunId)) === runId).length
}

// ---------------------------------------------------------------------------
// A Do turn through the composer: one request, captured frame by frame.
// ---------------------------------------------------------------------------

type TurnCapture = {
  frames: Frame[]
  status: number | null
  finished: boolean
  postCount: number
  resumeCount: number
  post: { model: string | null; provider: string | null; features: string[]; planMode: boolean; hasConnection: boolean } | null
  elapsedMs: number | null
  headersMs: number | null
  firstStepMs: number | null
  doneMs: number | null
  doneAt: string | null
  runStreamFrames: Record<string, number>
}

async function installCapture(page: Page) {
  await page.evaluate(() => {
    const target = window as CaptureWindow
    target.a6Capture?.restore()
    const original = window.fetch.bind(window)
    const now = () => performance.timeOrigin + performance.now()
    const onClick = (event: Event) => {
      if (capture.clickAt === null && event.target instanceof Element && event.target.closest('form button[type="submit"]')) capture.clickAt = now()
    }
    const capture: NonNullable<CaptureWindow['a6Capture']> = {
      clickAt: null,
      chat: [],
      runs: [],
      restore: () => { window.fetch = original; document.removeEventListener('click', onClick, true) },
    }
    // Capture phase: the click is stamped before the application handles it.
    document.addEventListener('click', onClick, true)
    window.fetch = async (...args: Parameters<typeof fetch>) => {
      const input = args[0]
      const url = input instanceof Request ? input.url : String(input)
      const method = (args[1]?.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase()
      const path = new URL(url, location.href).pathname
      const startedAt = now()
      const response = await original(...args)
      const kind = method === 'POST' && path === '/api/v1/chat/stream'
        ? 'chat'
        : method === 'GET' && /^\/api\/v1\/runs\/[^/]+\/events$/.test(path) ? 'runs' : null
      if (kind && response.body) {
        const record: StreamRecord = { status: response.status, startedAt, headersAt: now(), endAt: null, failed: false, chunks: [] }
        capture[kind].push(record)
        const reader = response.clone().body?.getReader()
        void (async () => {
          const decoder = new TextDecoder('utf-8')
          try {
            while (reader) {
              const { done, value } = await reader.read()
              if (done) break
              const text = decoder.decode(value, { stream: true })
              if (text) record.chunks.push({ at: now(), text })
            }
            const tail = decoder.decode()
            if (tail) record.chunks.push({ at: now(), text: tail })
          } catch {
            record.failed = true
          } finally {
            record.endAt = now()
          }
        })()
      }
      return response
    }
    target.a6Capture = capture
  })
}

async function snapshotCapture(page: Page): Promise<CaptureSnapshot | null> {
  return page.evaluate((): CaptureSnapshot | null => {
    const capture = (window as CaptureWindow).a6Capture
    return capture ? JSON.parse(JSON.stringify({ clickAt: capture.clickAt, chat: capture.chat, runs: capture.runs })) as CaptureSnapshot : null
  })
}

/**
 * Opens the chat on Terra, switches Do on, sends `prompt` once and waits for
 * the one stream to end or for the probe's time budget to run out.
 */
async function driveDoTurn(page: Page, prompt: string, stage: (name: string) => void, deadlineAt: number): Promise<TurnCapture> {
  stage('guard')
  // Blocks any /api/v1/chat/stream request that is not Terra on its connection.
  await requireProductSubscription(page)
  // One chat request per probe, enforced and not only counted. Playwright runs
  // the most recently registered route first, so this gate sees every request
  // before the guard above and lets exactly one POST through to it. A second
  // one (a retry, a loop, a regenerate) is blocked client side and never
  // reaches the network, so it cannot spend Terra quota; assertDoTurn still
  // reports it through the request count.
  let admittedPosts = 0
  await page.route('**/api/v1/chat/stream', route => {
    if (route.request().method() !== 'POST') return route.fallback()
    admittedPosts += 1
    return admittedPosts === 1 ? route.fallback() : route.abort('blockedbyclient')
  })
  await page.addInitScript(() => {
    if (sessionStorage.getItem('a6.fixture.initialized')) return
    sessionStorage.setItem('a6.fixture.initialized', 'yes')
    localStorage.setItem('verevon.locale', 'no')
    for (const key of ['verevon.chat.threadId', 'verevon.chat.threadHistory.v1', 'verevon.chat.threadTranscripts.v1', 'verevon.chat.pendingLaunch']) {
      localStorage.removeItem(key)
      sessionStorage.removeItem(key)
    }
  })

  stage('open_chat')
  await page.goto('/chat')
  expect(new URL(page.url()).pathname).toBe('/chat')
  await expect(page).toHaveTitle(/verevon/i)
  const composer = page.locator('form textarea').first()
  await expect(composer).toBeVisible({ timeout: 30_000 })
  await expect(page.locator('vite-error-overlay')).toHaveCount(0)

  stage('select_do')
  const workMode = page.getByRole('group', { name: /^(Arbeidsmodus|Work mode)$/ })
  const doButton = workMode.getByRole('button', { name: /^(Utfør|Do)$/ })
  await expect(workMode, 'The composer shows no Ask/Do control').toBeVisible({ timeout: 30_000 })
  await expect(doButton, "Do (Utfør) is disabled: the catalogue does not advertise 'agentic' for the selected Terra model, so AGENTIC_SUBSCRIPTION_ENABLED is not on for this stack").toBeEnabled({ timeout: 30_000 })
  await doButton.click()
  await expect(doButton).toHaveAttribute('aria-pressed', 'true')

  stage('submit')
  let postCount = 0
  let resumeCount = 0
  let firstPost: PwRequest | null = null
  page.on('request', request => {
    const { pathname } = new URL(request.url())
    if (request.method() === 'POST' && pathname === '/api/v1/chat/stream') {
      postCount += 1
      firstPost ??= request
    } else if (request.method() === 'GET' && pathname.startsWith('/api/v1/chat/stream/resume/')) {
      resumeCount += 1
    }
  })
  await installCapture(page)
  await composer.fill(prompt)
  const submit = page.locator('form button[type="submit"]').first()
  await expect(submit).toBeEnabled()
  const clickedAt = Date.now()
  await submit.click()
  await expect.poll(() => postCount, { timeout: 30_000, message: 'The composer did not send a chat request' }).toBeGreaterThan(0)

  stage('await_stream')
  let finished = true
  try {
    await page.waitForFunction(() => {
      const first = (window as CaptureWindow).a6Capture?.chat[0]
      return Boolean(first && first.endAt !== null)
    }, undefined, { timeout: Math.max(5_000, deadlineAt - Date.now()), polling: 500 })
  } catch {
    finished = false
  }
  // A second POST would be a retry or a loop; give one a moment to show itself.
  await page.waitForTimeout(1_000)

  stage('parse')
  const snapshot = await snapshotCapture(page)
  const chat = snapshot?.chat[0] ?? null
  const frames = chat ? parseSse(chat.chunks.map(chunk => chunk.text).join('')) : []
  const arrivals = chat ? frameArrivals(chat.chunks, frames) : []
  const clickAt = snapshot?.clickAt ?? clickedAt
  const firstStep = frames.findIndex(frame => frame.event === 'step_update')
  const done = frames.findIndex(frame => frame.event === 'done')
  const doneArrival = done >= 0 ? arrivals[done] : undefined
  let post: TurnCapture['post'] = null
  try {
    const body: unknown = (firstPost as PwRequest | null)?.postDataJSON()
    if (isRecord(body)) {
      post = {
        model: str(body.model) ?? null,
        provider: str(body.provider) ?? null,
        features: Array.isArray(body.features) ? body.features.filter((feature): feature is string => typeof feature === 'string') : [],
        planMode: body.plan_mode === true,
        hasConnection: Boolean(str(body.subscription_connection_id)),
      }
    }
  } catch {
    post = null
  }
  const runFrames = countBy((snapshot?.runs ?? []).flatMap(record => parseSse(record.chunks.map(chunk => chunk.text).join(''))), frame => frame.event)
  return {
    frames,
    status: chat?.status ?? null,
    finished: finished && chat?.endAt != null,
    postCount,
    resumeCount,
    post,
    elapsedMs: chat?.endAt != null ? Math.round(chat.endAt - clickAt) : null,
    headersMs: chat ? Math.round(chat.headersAt - clickAt) : null,
    firstStepMs: firstStep >= 0 && Number.isFinite(arrivals[firstStep]) ? Math.round((arrivals[firstStep] ?? 0) - clickAt) : null,
    doneMs: doneArrival !== undefined && Number.isFinite(doneArrival) ? Math.round(doneArrival - clickAt) : null,
    doneAt: doneArrival !== undefined && Number.isFinite(doneArrival) ? new Date(doneArrival).toISOString() : null,
    runStreamFrames: runFrames,
  }
}

/** Ids, counts and timings of a turn, written before any assertion can fail. */
function recordTurn(evidence: Evidence, turn: TurnCapture) {
  const connected = turn.frames.find(frame => frame.event === 'connected')?.data
  const errors = errorFrames(turn.frames)
  const chunks = turn.frames.filter(frame => frame.event === 'chunk')
  const steps = turn.frames.filter(frame => frame.event === 'step_update')
  evidence.run_id = str(connected?.run_id) ?? null
  evidence.thread_id = str(connected?.thread_id) ?? null
  evidence.request_id = str(connected?.request_id) ?? str(errors[0]?.data?.request_id) ?? null
  evidence.http_status = turn.status
  evidence.frames = countBy(turn.frames, frame => frame.event)
  evidence.elapsed_ms = turn.elapsedMs
  evidence.error_code = errorCode(errors[0]) ?? null
  evidence.detail.finished = turn.finished
  evidence.detail.chat_requests = turn.postCount
  evidence.detail.resume_requests = turn.resumeCount
  evidence.detail.request_model = turn.post?.model ?? null
  evidence.detail.request_provider = turn.post?.provider ?? null
  evidence.detail.request_features = turn.post?.features ?? []
  evidence.detail.request_plan_mode = turn.post?.planMode ?? null
  evidence.detail.headers_ms = turn.headersMs
  evidence.detail.first_step_update_ms = turn.firstStepMs
  evidence.detail.done_ms = turn.doneMs
  evidence.detail.done_at = turn.doneAt
  evidence.detail.answer_chunks = chunks.filter(frame => (str(frame.data?.delta) ?? '') !== '').length
  evidence.detail.answer_chars = chunks.reduce((total, frame) => total + (str(frame.data?.delta)?.length ?? 0), 0)
  evidence.detail.step_updates = countBy(steps, stepClass)
  evidence.detail.tool_named_step_updates = steps.filter(frame => /tool|verktøy|search|søk|knowledge/i.test(`${str(frame.data?.title) ?? ''} ${str(frame.data?.detail) ?? ''}`)).length
  evidence.detail.run_stream_frames = turn.runStreamFrames
}

/** The checks every Terra probe shares, in the order they explain a failure best. */
function assertDoTurn(turn: TurnCapture, label: string) {
  expect(turn.postCount, `${label}: exactly one chat request may be made (saw ${turn.postCount})`).toBe(1)
  expect(turn.post?.model, `${label}: the request must be Terra`).toBe(PRODUCT_MODEL)
  expect(turn.post?.provider, `${label}: the request must be on the subscription route`).toBe(PRODUCT_PROVIDER)
  expect(turn.post?.hasConnection, `${label}: the request must carry the subscription connection`).toBe(true)
  expect(turn.post?.features.includes('agentic') && turn.post.planMode, `${label}: the request must be a Do run (agentic feature with plan_mode)`).toBe(true)
  expect(turn.status, `${label}: the chat stream answered with HTTP ${turn.status}`).toBe(200)

  const errors = errorFrames(turn.frames)
  const code = errorCode(errors[0])
  if (code === USAGE_LIMIT_CODE) {
    const when = str(errors[0]?.data?.retry_at) ?? str(errors[0]?.data?.resets_at) ?? 'unknown'
    throw new Error(`STOP: the ChatGPT plan's Codex usage limit is exhausted (reset ${when}). Not retrying. Do not run another Terra probe until it resets.`)
  }
  if (code === RATE_LIMITED_CODE) {
    throw new Error('STOP: the ChatGPT subscription is rate limited right now. Not retrying. Wait, then run this one probe again by hand.')
  }
  expect(errors.length, `${label}: the stream carried a typed error frame (${code ?? 'none'}), after ${turn.frames.length} frames`).toBe(0)
  expect(turn.finished, `${label}: the stream did not end within the probe's time budget (${turn.frames.length} frames so far)`).toBe(true)
  expect(turn.frames.filter(frame => frame.event === 'awaiting_approval').length, `${label}: the run paused for approval, which a read-only probe should never need`).toBe(0)

  const connected = turn.frames.find(frame => frame.event === 'connected')?.data
  expect(Boolean(str(connected?.run_id)), `${label}: no connected frame carried a run id`).toBe(true)
  const done = turn.frames.filter(frame => frame.event === 'done')
  expect(done.length, `${label}: expected exactly one done frame`).toBe(1)
  // The echoed request model (see the header): proof that nothing rewrote the
  // request, not proof of what served it. The inference-core log is that proof.
  expect(str(done[0]?.data?.modelUsed) ?? str(done[0]?.data?.model_used), `${label}: the done frame must echo Terra (this does not show which provider served the rounds)`).toBe(PRODUCT_MODEL)
  const chunks = turn.frames.filter(frame => frame.event === 'chunk' && (str(frame.data?.delta) ?? '') !== '')
  expect(chunks.length, `${label}: the stream carried no answer chunks`).toBeGreaterThanOrEqual(1)
}

const answerOf = (turn: TurnCapture) => turn.frames.filter(frame => frame.event === 'chunk').map(frame => str(frame.data?.delta) ?? '').join('')

/** The durable run record, when the probe has time left to read it. */
async function readDurableRun(page: Page, evidence: Evidence, deadlineAt: number) {
  if (!evidence.run_id || deadlineAt - Date.now() < DURABLE_READ_MIN_REMAINING_MS) {
    evidence.detail.durable_run = 'skipped'
    return null
  }
  const run = await readRun(page, evidence.run_id)
  evidence.detail.durable_run = run ? 'read' : 'unreadable'
  if (run) {
    evidence.detail.durable_status = run.status
    evidence.detail.durable_mode = run.mode
    evidence.detail.durable_steps_completed = run.stepsCompleted
  }
  return run
}

// ---------------------------------------------------------------------------
// Refusal classifier. The answer text lives in a local variable for the length
// of one call; only booleans and counts leave it. Never log, store or assert
// with the text.
// ---------------------------------------------------------------------------

type AnswerClass = { refusalHint: boolean; urlCount: number; answerChars: number }
type FinalAnswer = { source: 'thread_messages' | 'stream_chunks' | 'none'; refusalHint: boolean | null; urlCount: number; answerChars: number }

function classifyAnswer(text: string): AnswerClass {
  // The refusal words are looked for outside the links: a cited address such as
  // .../policy/... is a source, not a refusal. A refusal is said in prose.
  return { refusalHint: REFUSAL_HINT.test(text.replace(URL_PATTERN, ' ')), urlCount: (text.match(URL_PATTERN) ?? []).length, answerChars: text.length }
}

/** The messages array of GET .../messages, whichever envelope the gateway used: [..], {messages}, {data:[..]} or {data:{messages}}. */
function messageList(body: unknown): unknown[] {
  if (Array.isArray(body)) return body
  if (!isRecord(body)) return []
  if (Array.isArray(body.messages)) return body.messages
  return messageList(body.data)
}

/** The content of the last assistant message that has any. An empty one is a placeholder, not an answer. */
function lastAssistantContent(body: unknown): string | null {
  const answers = messageList(body)
    .filter(isRecord)
    .map(message => (message.role === 'assistant' ? str(message.content) ?? '' : ''))
    .filter(content => content.trim() !== '')
  return answers.at(-1) ?? null
}

async function readLastAssistantText(page: Page, threadId: string): Promise<string | null> {
  try {
    const response = await page.request.get(`/api/v1/chat/threads/${encodeURIComponent(threadId)}/messages`, { timeout: DURABLE_READ_TIMEOUT_MS })
    return response.ok() ? lastAssistantContent(await response.json()) : null
  } catch {
    return null
  }
}

/**
 * Classifies the final answer of a turn and records refusal_hint, url_count and
 * the length in the evidence. The persisted last assistant message is the
 * source; the streamed chunks (the same answer, already in memory) stand in when
 * it cannot be read, and `answer_source` says which one was used. With neither,
 * refusal_hint is null, which no probe accepts.
 */
async function classifyFinalAnswer(page: Page, evidence: Evidence, turn: TurnCapture, deadlineAt: number): Promise<FinalAnswer> {
  let text: string | null = null
  let attempts = 0
  while (text === null && evidence.thread_id && attempts < ANSWER_READ_ATTEMPTS && deadlineAt - Date.now() >= DURABLE_READ_MIN_REMAINING_MS) {
    if (attempts > 0) await page.waitForTimeout(ANSWER_READ_RETRY_MS)
    attempts += 1
    text = await readLastAssistantText(page, evidence.thread_id)
  }
  let source: FinalAnswer['source'] = 'thread_messages'
  if (text === null) {
    const streamed = answerOf(turn)
    text = streamed.trim() === '' ? null : streamed
    source = text === null ? 'none' : 'stream_chunks'
  }
  const answer: FinalAnswer = text === null
    ? { source, refusalHint: null, urlCount: 0, answerChars: 0 }
    : { source, ...classifyAnswer(text) }
  evidence.detail.answer_source = answer.source
  evidence.detail.answer_read_attempts = attempts
  evidence.detail.refusal_hint = answer.refusalHint
  evidence.detail.url_count = answer.urlCount
  evidence.detail.final_answer_chars = answer.answerChars
  return answer
}

// Fixed text, never the answer. Also what a probe fails with when no answer could be read (refusal_hint null).
const REFUSED_TOOL_MESSAGE = 'the final answer reports a refused or unavailable tool, or no answer could be read (answer_source in the evidence line says which); a refused call still counts as a durable run step, so the run proves nothing about tool use. Check capability policy for the tools, then the session-core tool audit and the execution-core log for the run id in the evidence line'

test.describe('A6 agentic subscription live proof', () => {
  // Serial, no retries: a failure stops the probes after it, and no probe may
  // ever make a second chat request on its own.
  test.describe.configure({ mode: 'serial', retries: 0 })

  // Free and offline: no page, no request. It pins the classifier that PK, P3
  // and P5 rely on, so that a refused tool cannot pass as tool evidence again.
  // Run it first (`--grep P0`); a failure here means the verdicts below are void.
  test('P0 answer classifier self-check: a refused tool is not tool evidence', () => {
    test.setTimeout(FREE_TEST_TIMEOUT_MS)
    // The shape of the first live run's answer: both searches refused by policy.
    const refusedBoth = 'Både kunnskapssøket og nettsøket ble avvist av tilgangspolicyen, så jeg kan ikke oppgi noen kilder.'
    expect(classifyAnswer(refusedBoth), 'a refusal sentence must raise refusal_hint and cite no URL').toMatchObject({ refusalHint: true, urlCount: 0 })

    // Every word of the list on its own, in the case a model might write it.
    const hinted = [
      'Tilgangspolicyen stoppet søket.', 'Søket ble avvist.', 'Jeg har IKKE TILGANG til nettsøk.', 'Tilgangen ble nektet.',
      'Blocked by policy.', 'The call was denied.', 'The call was refused.', 'That is not\npermitted.', 'That is Not Allowed.',
      'The tool is unavailable.', 'Verktøyet er utilgjengelig.',
      'Verktøyet var blokkert.', 'The tool was Blocked.', 'Nettsøk er ikke tillatt.', 'Nettsøk er ikke\ntilgjengelig.', 'Web search is not available.',
      'Det er forbudt.', 'Access is forbidden.', 'Forespørselen ble avslått.', 'Systemet avslo kallet.', 'The call was rejected.',
      'The call was unauthorized.', 'The call was unauthorised.', 'Kallet var uautorisert.',
    ]
    hinted.forEach((text, index) => expect(classifyAnswer(text).refusalHint, `refusal word fixture ${index} must be recognised`).toBe(true))

    // A cited answer with no refusal word passes, and every https link is counted once.
    const cited = 'Norsk lakseoppdrett eksporterte for rundt 100 milliarder kroner (kilde: https://www.ssb.no/utenriksokonomi). Omtrent 8000 er sysselsatt ([Fiskeridirektoratet](https://www.fiskeridir.no/statistikk)).'
    expect(classifyAnswer(cited), 'a cited answer with no refusal word must be clean').toMatchObject({ refusalHint: false, urlCount: 2, answerChars: cited.length })
    expect(classifyAnswer('http://a.example/x, https://b.example/y and https://c.example.').urlCount, 'http and https links both count').toBe(3)
    expect(classifyAnswer('Jeg fant tre kilder om lakseoppdrett i kunnskapsbasen din.'), 'a plain answer has no refusal word and no URL').toMatchObject({ refusalHint: false, urlCount: 0 })
    expect(classifyAnswer('Se https:// og www.example.no'), 'a bare scheme is not a link').toMatchObject({ urlCount: 0 })
    // The P3 prompt spells out "(https://...)"; a refusing model that echoes it has cited nothing.
    expect(classifyAnswer('Jeg fant ingen full nettadresse (https://...) til kilden.'), 'the prompt placeholder is not a link').toMatchObject({ urlCount: 0 })
    expect(classifyAnswer('Kilde: https://example/ og http://localhost:3000/x'), 'a host without a dotted domain is not a link').toMatchObject({ urlCount: 0 })
    expect(classifyAnswer('Kilde: https://www.regjeringen.no:443/no/policy-forbidden/?a=1#b.').urlCount, 'a port, path, query and fragment stay one link').toBe(1)
    // A refusal word inside a cited address is a source, not a refusal; the same word in prose still is.
    expect(classifyAnswer('Eksport: 100 mrd (https://www.example.no/policy/blocked-denied).'), 'a refusal word in a link is not a refusal').toMatchObject({ refusalHint: false, urlCount: 1 })
    expect(classifyAnswer('Avvist av policy, se https://www.example.no/policy.'), 'a refusal in prose is still a refusal when a link follows').toMatchObject({ refusalHint: true, urlCount: 1 })

    // The last assistant message, whichever envelope the gateway answers with.
    const messages = [
      { role: 'user', content: 'a question', message_id: 'm1' },
      { role: 'assistant', content: 'the first answer', message_id: 'm2' },
      { role: 'assistant', content: 'the last answer', message_id: 'm3' },
      { role: 'user', content: 'a later question', message_id: 'm4' },
    ]
    for (const [shape, body] of Object.entries({ array: messages, messages: { messages }, data_array: { data: messages }, data_messages: { data: { messages } } })) {
      expect(lastAssistantContent(body), `the last assistant message must be found in the ${shape} envelope`).toBe('the last answer')
    }
    expect(lastAssistantContent({ messages: [{ role: 'user', content: 'only a question' }, { role: 'assistant', content: '  ' }] }), 'an empty assistant message is not an answer').toBeNull()
    expect([null, {}, 'text', { data: null }, { messages: 'no' }].map(lastAssistantContent), 'an unreadable body has no answer').toEqual([null, null, null, null, null])

    // The text itself can never reach the evidence line.
    expect(scrub({ detail: { answer: 'Both searches were refused by the access policy.' } }), 'free text must be omitted from the evidence').toEqual({ detail: { answer: '[omitted]' } })
  })

  test('P1 switch off: typed refusal, no run, agentic not advertised', async ({ page }, info) => {
    test.setTimeout(FREE_TEST_TIMEOUT_MS)
    await runProbe(info, 'P1', async (evidence, stage) => {
      stage('catalogue')
      const catalogue = await readCatalogue(page)
      const advertising = catalogue.subscription.filter(model => model.features.includes('agentic')).length
      evidence.detail.subscription_models = catalogue.subscription.length
      evidence.detail.terra_listed = catalogue.terra !== null
      evidence.detail.models_advertising_agentic = advertising
      expect(catalogue.terra !== null, `The catalogue does not list ${PRODUCT_MODEL} on ${PRODUCT_PROVIDER}`).toBe(true)
      // Checked before anything is sent: with the switch on the request below
      // would be admitted and start a run.
      expect(advertising, "A subscription model advertises 'agentic', so the switch is on and this probe must not be sent").toBe(0)

      stage('refusal_request')
      const response = await postDirect(page, 60_000)
      const errors = errorFrames(response.frames)
      evidence.http_status = response.status
      evidence.frames = countBy(response.frames, frame => frame.event)
      evidence.elapsed_ms = response.elapsedMs
      evidence.error_code = errorCode(errors[0]) ?? null
      evidence.request_id = str(errors[0]?.data?.request_id) ?? null
      evidence.detail.content_type = response.contentType.split(';')[0] ?? ''
      evidence.detail.shape = 'sse_error_frame'

      stage('assert_refusal')
      expect(response.status, `Expected the refusal as a streamed frame under HTTP 200, got HTTP ${response.status} (${response.contentType || 'no content type'}) with ${response.frames.length} frames`).toBe(200)
      expect(response.contentType, 'The refusal must be an event stream').toContain('text/event-stream')
      expect(response.frames.map(frame => frame.event), 'The refusal must be the only frame').toEqual(['error'])
      expect(evidence.error_code, 'The refusal must carry the typed code').toBe(DISABLED_CODE)
      expect(errors[0]?.data?.retryable, 'The refusal is not retryable').toBe(false)
      expect(Boolean(evidence.request_id), 'The refusal must carry its request id').toBe(true)
      // StartRun comes after normalization, and `connected` is its first frame.
      const mentionsRun = response.frames.some(frame => frame.event === 'connected' || (frame.data !== null && 'run_id' in frame.data))
      expect(mentionsRun, 'A run was created: a frame carried a run id').toBe(false)
      evidence.detail.run_created = false
    })
  })

  test('P2 switch on, bogus connection: run created, first round fails typed', async ({ page }, info) => {
    test.setTimeout(P2_TEST_TIMEOUT_MS)
    await runProbe(info, 'P2', async (evidence, stage) => {
      stage('request')
      const response = await postDirect(page, 120_000)
      const connected = response.frames.find(frame => frame.event === 'connected')?.data
      const errors = errorFrames(response.frames)
      const code = errorCode(errors[0])
      evidence.http_status = response.status
      evidence.frames = countBy(response.frames, frame => frame.event)
      evidence.elapsed_ms = response.elapsedMs
      evidence.run_id = str(connected?.run_id) ?? null
      evidence.thread_id = str(connected?.thread_id) ?? null
      evidence.request_id = str(connected?.request_id) ?? str(errors[0]?.data?.request_id) ?? null
      evidence.error_code = code ?? null
      evidence.detail.error_retryable = typeof errors[0]?.data?.retryable === 'boolean' ? errors[0].data.retryable : null
      evidence.detail.step_updates = countBy(response.frames.filter(frame => frame.event === 'step_update'), stepClass)

      stage('assert_failure')
      if (code === DISABLED_CODE) throw new Error('P2 needs AGENTIC_SUBSCRIPTION_ENABLED on for model-gateway: it answered with the disabled refusal')
      expect(response.status, `The stream answered with HTTP ${response.status}`).toBe(200)
      expect(Boolean(evidence.run_id), 'No run was created: no connected frame carried a run id').toBe(true)
      expect(errors.length, `Expected exactly one error frame, saw ${errors.length}`).toBe(1)
      expect(response.frames.at(-1)?.event, 'The stream must end on the error frame').toBe('error')
      if (code === USAGE_LIMIT_CODE || code === RATE_LIMITED_CODE) {
        throw new Error(`A real Codex turn seems to have run (${code}); a bogus connection must fail before one`)
      }
      expect(P2_TYPED_CODES, `Expected a typed subscription error, got ${code ?? 'none'}`).toContain(code)
      expect(response.frames.filter(frame => frame.event === 'chunk' || frame.event === 'done').length, 'A failed first round produces no answer').toBe(0)

      stage('durable_read')
      await readDurableRun(page, evidence, Date.now() + P2_TEST_TIMEOUT_MS)
    })
  })

  // Knowledge search only, one Terra request. A pass shows that a tool call was
  // attempted and the answer does not report it refused; it cannot show that
  // retrieval returned results (see the header), so the first live run is
  // confirmed in the tool audit and the execution-core log.
  test('PK knowledge search only: Do on Terra, the tool is not refused', async ({ page }, info) => {
    test.setTimeout(TERRA_TEST_TIMEOUT_MS)
    const deadlineAt = Date.now() + TERRA_BODY_BUDGET_MS
    await runProbe(info, 'PK', async (evidence, stage) => {
      const turn = await driveDoTurn(page, PK_PROMPT, stage, deadlineAt)
      recordTurn(evidence, turn)
      // The shared checks: one request, Terra on the subscription route, no
      // error frame (a usage limit stops here), a finished stream and a done frame.
      stage('assert_turn')
      assertDoTurn(turn, 'PK')

      // A model that answers without calling the tool has nothing to refuse, so
      // a clean answer alone would pass a probe that never searched. The durable
      // run records one step per tool call (a refused call too): necessary, never
      // sufficient, as in P3.
      stage('durable_read')
      const run = await readDurableRun(page, evidence, deadlineAt)
      evidence.detail.tool_evidence = run ? (run.stepsCompleted >= 1 ? 'durable_steps_recorded' : 'no_durable_steps') : 'run_not_read'

      stage('classify_answer')
      const answer = await classifyFinalAnswer(page, evidence, turn, deadlineAt)

      stage('assert_not_refused')
      expect(answer.refusalHint, `PK: ${REFUSED_TOOL_MESSAGE}`).toBe(false)
      stage('assert_tool_called')
      expect(run?.stepsCompleted ?? 0, 'PK: no durable step corroborates a knowledge_search call (tool_evidence and durable_run in the evidence line say whether the run recorded none or could not be read); check the session-core tool audit and the execution-core log for the run id before counting this as a pass').toBeGreaterThanOrEqual(1)
    })
  })

  test('P3 Do on Terra: knowledge and web search run to a done frame', async ({ page }, info) => {
    test.setTimeout(TERRA_TEST_TIMEOUT_MS)
    const deadlineAt = Date.now() + TERRA_BODY_BUDGET_MS
    await runProbe(info, 'P3', async (evidence, stage) => {
      const turn = await driveDoTurn(page, P3_PROMPT, stage, deadlineAt)
      recordTurn(evidence, turn)
      stage('assert_turn')
      assertDoTurn(turn, 'P3')
      const stepFrames = turn.frames.filter(frame => frame.event === 'step_update').length

      stage('durable_read')
      const run = await readDurableRun(page, evidence, deadlineAt)
      // The stream cannot show a tool call, and it carries a step_update only
      // when the model proposes a plan or delegates (see the header), so a
      // correct run can have none. The durable run is the corroboration: a run
      // that recorded no step at all ran no tool. It is necessary and never
      // sufficient, because a call that policy refused is a durable step too.
      evidence.detail.tool_evidence = run
        ? (run.stepsCompleted >= 1 ? 'durable_steps_recorded' : 'no_durable_steps')
        : stepFrames >= 1 ? 'stream_step_updates_only' : 'none'

      stage('classify_answer')
      const answer = await classifyFinalAnswer(page, evidence, turn, deadlineAt)
      evidence.detail.tool_use_proved = answer.refusalHint === false && answer.urlCount >= 1
        && (run ? run.stepsCompleted >= 1 : stepFrames >= 1)

      stage('assert_tool_use')
      // The answer first: it is what shows that a durable step was a refusal.
      expect(answer.refusalHint, `P3: ${REFUSED_TOOL_MESSAGE}`).toBe(false)
      expect(answer.urlCount, 'P3: the final answer cites no source URL, so no web_search result reached it (url_count is in the evidence line)').toBeGreaterThanOrEqual(1)
      if (run) {
        expect(run.stepsCompleted, 'P3: the durable run recorded no step, so no knowledge_search or web_search ran').toBeGreaterThanOrEqual(1)
      } else {
        // Fail closed: an answer with no corroboration of tool use must not pass
        // as the proof that tools ran. The ids are in the evidence line, so the
        // logs can settle it without a second Terra turn.
        expect(stepFrames, 'P3: nothing corroborates tool use (the durable run could not be read and the stream carried no step_update); check the execution-core and inference-core logs for the run id in the evidence line').toBeGreaterThanOrEqual(1)
        info.annotations.push({ type: 'tool-evidence', description: 'The durable run could not be read; tool use rests on the stream step_updates alone. Check the execution-core and inference-core logs.' })
      }
    })
  })

  test('P4 Do on Terra: a multi-step plan arrives as plan_proposed', async ({ page }, info) => {
    test.setTimeout(TERRA_TEST_TIMEOUT_MS)
    const deadlineAt = Date.now() + TERRA_BODY_BUDGET_MS
    await runProbe(info, 'P4', async (evidence, stage) => {
      const turn = await driveDoTurn(page, P4_PROMPT, stage, deadlineAt)
      recordTurn(evidence, turn)
      const plans = turn.frames.filter(frame => frame.event === 'plan_proposed')
      const steps = Array.isArray(plans[0]?.data?.steps) ? plans[0].data.steps : []
      evidence.detail.plan_proposed_frames = plans.length
      evidence.detail.plan_steps = steps.length
      evidence.detail.plan_steps_titled = steps.filter(step => isRecord(step) && (str(step.title)?.trim() ?? '') !== '').length
      evidence.detail.plan_has_id = Boolean(str(plans[0]?.data?.plan_id))

      stage('assert_turn')
      // The point of P4 first: plan.propose reached the stream through the envelope.
      expect(plans.length, 'P4: no plan_proposed frame arrived').toBeGreaterThanOrEqual(1)
      expect(steps.length, 'P4: the plan_proposed frame carried no steps').toBeGreaterThanOrEqual(1)
      expect(evidence.detail.plan_steps_titled, 'P4: every plan step must have a title').toBe(steps.length)
      expect(evidence.detail.plan_has_id, 'P4: the plan must carry its plan_id').toBe(true)
      assertDoTurn(turn, 'P4')
    })
  })

  test('P5 Do on Terra: research_fan_out of 3 items attaches and stops 3 subagents', async ({ page }, info) => {
    test.setTimeout(TERRA_TEST_TIMEOUT_MS)
    const deadlineAt = Date.now() + TERRA_BODY_BUDGET_MS
    await runProbe(info, 'P5', async (evidence, stage) => {
      const turn = await driveDoTurn(page, P5_PROMPT, stage, deadlineAt)
      recordTurn(evidence, turn)
      const attached = new Set<string>()
      const stopped = new Set<string>()
      // A stop's status is always `done`; whether the child finished is its detail.
      const failedStops = new Set<string>()
      for (const frame of turn.frames.filter(candidate => candidate.event === 'step_update')) {
        const id = str(frame.data?.id) ?? ''
        const kind = stepClass(frame)
        if (kind === 'subagent_attached') attached.add(id)
        else if (kind === 'subagent_stopped') {
          stopped.add(id)
          if (str(frame.data?.detail) === 'failed') failedStops.add(id)
        }
      }
      const answer = answerOf(turn).toLocaleLowerCase('nb')
      const mentioned = Object.fromEntries(P5_ITEMS.map(item => [item, answer.includes(item.toLocaleLowerCase('nb'))]))
      evidence.detail.subagents_attached = attached.size
      evidence.detail.subagents_stopped = stopped.size
      evidence.detail.subagents_failed = failedStops.size
      evidence.detail.subagents_unmatched = [...attached].filter(id => !stopped.has(id)).length
      evidence.detail.subagent_child_ids = [...attached]
      evidence.detail.digest_items_mentioned = Object.values(mentioned).filter(Boolean).length
      evidence.detail.raw_subagent_attached = turn.runStreamFrames.subagent_attached ?? 0
      evidence.detail.raw_subagent_stopped = turn.runStreamFrames.subagent_stopped ?? 0

      stage('durable_read')
      const run = await readDurableRun(page, evidence, deadlineAt)
      const children = evidence.run_id && evidence.thread_id && deadlineAt - Date.now() >= DURABLE_READ_MIN_REMAINING_MS
        ? await readChildRunCount(page, evidence.thread_id, evidence.run_id)
        : null
      evidence.detail.durable_child_runs = children
      evidence.detail.durable_run_read = run !== null

      stage('classify_answer')
      const classified = await classifyFinalAnswer(page, evidence, turn, deadlineAt)

      stage('assert_turn')
      assertDoTurn(turn, 'P5')
      expect(attached.size, `P5: expected 3 subagent attachments on the chat stream, saw ${attached.size} (durable child runs: ${children ?? 'unread'})`).toBe(3)
      expect(stopped.size, `P5: expected 3 subagent stops on the chat stream, saw ${stopped.size}`).toBe(3)
      expect([...attached].filter(id => !stopped.has(id)).length, 'P5: a subagent attached and never stopped').toBe(0)
      // Without this, three children that all failed still pass, and the parent's
      // own knowledge of three cities can fill the digest.
      expect(failedStops.size, 'P5: a subagent stopped as failed; the fan-out did not complete').toBe(0)
      // A child can stop as completed while its own research tools were refused.
      expect(classified.refusalHint, `P5: ${REFUSED_TOOL_MESSAGE}`).toBe(false)
      expect(evidence.detail.digest_items_mentioned, 'P5: the merged answer must cover all three items').toBe(P5_ITEMS.length)
    })
  })
})
