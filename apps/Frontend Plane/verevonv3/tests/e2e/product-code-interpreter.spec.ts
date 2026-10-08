import { expect, test, type Page, type TestInfo } from '@playwright/test'
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { composer, openFresh, transcript } from './product-acceptance-helpers'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'
import {
  PROOF_CASES, TERRA_MODEL, appendEvidence, armStreamCapture, judgeCase, mockCodeInterpreterTurn, proofDir, readDelivery,
  readStreamCapture, runOracle, summariseTurn, type DeliveryDom, type MockFile, type OracleVerdict,
  type ProofCase, type TurnSummary,
} from './code-interpreter-proof'

/**
 * Chat gap-closure plan, workstream H (Office, data and code outputs), task H5:
 * live proof that `code_interpreter` works on the ChatGPT Terra subscription route
 * (gpt-5.6-terra through openai-codex-subscription): five fresh threads, one
 * output each, from the composer, the way a user would ask.
 *
 *   xlsx  formulas and a native chart     docx  a table        pdf  æøå
 *   pptx  a matplotlib picture on a slide                      png  a stand-alone chart
 *
 * Per turn it records and checks: the tool decision called code_interpreter at all;
 * the sandbox time against its 30 s limit (tool_call frame to tool_result frame);
 * each file arriving as a download card, the PNG as an inline preview; each card
 * clicked and the real download saved; and each saved file read back by
 * tests/e2e/fixtures/code-interpreter/h5_oracle.py with the library for its type,
 * run in the stack's own execution-core image. The judging is code-interpreter-proof.ts
 * `judgeCase`, all problems at once. The runbook (plans/system-audits/
 * H5_CODE_INTERPRETER_TERRA_RUNBOOK_2026-10-01.md) says how to run it and what to
 * do with each failure.
 *
 * Two kinds of test, cheapest first, in one serial describe so that a failure stops
 * everything after it (a broken harness must not spend Terra turns, and the first
 * failed live case says whether the next four are worth running):
 *
 *   dry run   the proof's own plumbing on the real page with a mocked stream: no model
 *             turn and no Terra quota. The five reference programs are run by the
 *             oracle in the stack's image, their files are sent as a mocked
 *             code_interpreter turn, and the same observation and judging code reads
 *             it back. Needs docker and the model-plane-execution-core image, and a
 *             signed-in localhost session; nothing else.
 *   live x5   ONE Terra turn each, through requireProductSubscription (it blocks any
 *             request that is not the Terra subscription and never falls back to
 *             Balance or Claude). They only run with H5_LIVE=1, so a bare run of this
 *             file cannot spend quota. Never retried. A subscription usage limit is
 *             recorded as such and stops the series.
 *
 * Run from apps/Frontend Plane/verevonv3 (--no-deps keeps an already signed-in
 * localhost session instead of running the sign-in setup again):
 *
 *   npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "dry run"
 *   H5_LIVE=1 H5_PROOF_DIR=C:/private/verevon-recordings/h5-proof npx playwright test --project=product-readiness --no-deps tests/e2e/product-code-interpreter.spec.ts --grep "live xlsx"
 *
 * Evidence: one JSON line per live case in $H5_PROOF_DIR/evidence.jsonl (default
 * test-results/code-interpreter-proof, which Playwright clears at the start of every run,
 * so set H5_PROOF_DIR to keep the series): ids, counts, timings, file names and hashes. No
 * prompt, answer or token is written; every string passes through scrub(). The
 * downloaded files (invented content only) and the oracle's full verdict are saved
 * beside it in $H5_PROOF_DIR/<kind>/.
 */

const LIVE = process.env.H5_LIVE === '1'
if (TERRA_MODEL !== PRODUCT_MODEL) throw new Error('code-interpreter-proof.ts TERRA_MODEL has drifted from PRODUCT_MODEL')
const TURN_BODY_BUDGET_MS = 200_000

type Observed = {
  summary: TurnSummary
  dom: DeliveryDom
  saved: string[]
  verdicts: OracleVerdict[]
  answer: string
  elapsedMs: number
  requestFeatures: string[]
}

/** The files that one turn left in the message, saved by clicking their own download controls. */
async function downloadDelivered(page: Page, dir: string): Promise<string[]> {
  rmSync(dir, { recursive: true, force: true })
  mkdirSync(dir, { recursive: true })
  const message = page.locator('.verevon-chat-message--assistant').last()
  const controls = message.locator('.verevon-chat-generated-file a[download], .verevon-chat-image-preview__actions a[download]')
  const saved: string[] = []
  const count = await controls.count()
  for (let index = 0; index < count; index++) {
    const [download] = await Promise.all([page.waitForEvent('download', { timeout: 15_000 }), controls.nth(index).click()])
    const target = join(dir, `${String(index + 1).padStart(2, '0')}-${download.suggestedFilename()}`)
    await download.saveAs(target)
    saved.push(target)
  }
  return saved
}

/**
 * Submit `prompt` from the composer and read the turn back: the stream (with
 * timestamps), what the message offers, the downloads, the oracle's verdicts.
 * What it saw is attached to the test before anything is judged, so a failed case keeps it.
 */
async function observeTurn(page: Page, info: TestInfo, kind: string, prompt: string): Promise<Observed> {
  const startedAt = Date.now()
  await composer(page).fill(prompt)
  await armStreamCapture(page)
  const responsePending = page.waitForResponse(r => r.url().endsWith('/api/v1/chat/stream') && r.request().method() === 'POST', { timeout: 60_000 })
  await page.locator('form button[type="submit"]').first().click()
  const response = await responsePending
  const request = response.request().postDataJSON() as { model?: string; provider?: string; subscription_connection_id?: string; features?: string[] }
  expect(request.model).toBe(PRODUCT_MODEL)
  expect(request.provider).toBe(PRODUCT_PROVIDER)
  expect(Boolean(request.subscription_connection_id)).toBe(true)
  expect(response.status()).toBe(200)

  const capture = await readStreamCapture(page)
  const summary = summariseTurn(capture.chunks, capture.startedAt)
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.status, { timeout: 20_000 }).not.toBe('waiting')

  // The cards and the preview render a moment after the stream's last frame.
  await expect.poll(async () => { const delivery = await readDelivery(page); return delivery.cards.length + delivery.previews.length }, { timeout: 10_000 })
    .toBeGreaterThan(0).catch(() => undefined)
  await page.locator('.verevon-chat-image-preview img').last().scrollIntoViewIfNeeded({ timeout: 2_000 }).catch(() => undefined)
  await expect.poll(async () => (await readDelivery(page)).previews.every(preview => preview.loaded), { timeout: 10_000 }).toBe(true).catch(() => undefined)
  const dom = await readDelivery(page)
  await info.attach(`${kind}-delivery`, { body: JSON.stringify({ cards: dom.cards.map(({ name, label }) => ({ name, label })), previews: dom.previews.map(({ loaded, width, download }) => ({ loaded, width, download })) }), contentType: 'application/json' })

  const dir = join(proofDir(), kind)
  const saved = await downloadDelivered(page, dir)
  const oracle = saved.length > 0 ? runOracle('check', dir) : { results: [] as OracleVerdict[] }
  await info.attach(`${kind}-oracle`, { body: JSON.stringify(oracle), contentType: 'application/json' })

  const answer = (await transcript(page))?.turns.filter(turn => turn.role === 'assistant').at(-1)?.content ?? ''
  return { summary, dom, saved, verdicts: oracle.results, answer, elapsedMs: Date.now() - startedAt, requestFeatures: request.features ?? [] }
}

function evidenceFor(proofCase: ProofCase, observed: Observed | null, problems: string[], failure: string | null) {
  const summary = observed?.summary
  const calls = summary?.tools.filter(tool => tool.name === 'code_interpreter') ?? []
  const cap = summary?.errorCodes.some(code => code === 'subscription_usage_limit' || code === 'rate_limited') ?? false
  return {
    mode: 'live',
    case: proofCase.kind,
    at: new Date().toISOString(),
    outcome: cap ? 'cap_hit' : problems.length === 0 && !failure ? 'pass' : 'fail',
    failure,
    problems: problems.length,
    request_id: summary?.requestId ?? null,
    thread_id: summary?.threadId ?? null,
    elapsed_ms: observed?.elapsedMs ?? null,
    request_features: observed?.requestFeatures ?? [],
    frames: summary?.frameCounts ?? {},
    done_model: summary?.doneModel ?? null,
    error_codes: summary?.errorCodes ?? [],
    tools: summary?.tools.map(tool => tool.name) ?? [],
    code_interpreter_calls: calls.length,
    // The tool_call frame to its tool_result frame: the sandbox run plus the dispatch around it.
    code_interpreter: calls.map(call => ({ exit_code: call.exitCode, result_after_ms: call.resultAfterMs, files: call.filesDelivered, traceback: call.traceback, error: call.error })),
    tool_errors: summary?.tools.filter(tool => tool.error).length ?? 0,
    attachments: summary?.attachments.map(item => ({ name: item.name, mime: item.mime, declared_bytes: item.declaredBytes, decoded_bytes: item.decodedBytes, sha256: item.sha256?.slice(0, 16) ?? null })) ?? [],
    cards: observed?.dom.cards.map(card => card.name) ?? [],
    previews: observed?.dom.previews.length ?? 0,
    previews_loaded: observed?.dom.previews.filter(preview => preview.loaded).length ?? 0,
    downloads: observed?.saved.length ?? 0,
    oracle: observed?.verdicts.map(verdict => ({ file: verdict.file, ok: verdict.ok, problems: verdict.problems.length })) ?? [],
    answer_chars: summary?.answerChars ?? null,
  }
}

// retries: 0 whatever the project says (playwright.config.ts retries once under CI): a failed live case must not
// re-run the file's earlier cases, because each live case is a real Terra turn.
test.describe.configure({ mode: 'serial', retries: 0 })

test('H5 dry run: the proof reads a mocked code_interpreter turn on the real page (no model turn)', async ({ page }, info) => {
  test.setTimeout(180_000)
  // The five reference programs, run by the oracle in the stack's own image, give the files.
  const reference = runOracle<{ files: Array<MockFile & { program: string }>; timings: Array<{ program: string; exit_code: number; seconds: number }> }>('reference')
  expect(reference.timings.map(timing => timing.exit_code), 'every reference program must run clean in the oracle image').toEqual(reference.timings.map(() => 0))
  expect(reference.files.map(file => file.name).sort()).toEqual(['besokstall.png', 'kvartalsrapport.pptx', 'omsetning.png', 'salg-q3.xlsx', 'sammendrag.pdf', 'statusnotat.docx'])
  await info.attach('reference-timings', { body: JSON.stringify(reference.timings), contentType: 'application/json' })

  await openFresh(page, '/chat', { mockedInference: true })
  await page.route('**/api/v1/chat/stream', route => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    return route.fulfill({
      status: 200, contentType: 'text/event-stream',
      body: mockCodeInterpreterTurn(reference.files, 'Filene er laget og ligger som nedlastinger under meldingen.'),
    })
  })
  const observed = await observeTurn(page, info, 'dry-run', 'Transporttest: lever fem filer fra kodeverktøyet uten å bruke en modell.')

  // The page: four cards and two inline previews (the deck's chart and the stand-alone one).
  expect(observed.dom.cards.map(card => card.name).sort()).toEqual(['kvartalsrapport.pptx', 'salg-q3.xlsx', 'sammendrag.pdf', 'statusnotat.docx'])
  expect(observed.dom.cards.every(card => card.href.startsWith('data:'))).toBe(true)
  expect(observed.dom.previews, 'both PNG files are inline previews').toHaveLength(2)
  expect(observed.dom.previews.every(preview => preview.loaded && preview.width >= 300)).toBe(true)
  // The downloads: one per control, the same bytes the stream carried.
  expect(observed.saved).toHaveLength(6)
  const streamed = new Map(observed.summary.attachments.map(item => [item.name, item.sha256]))
  for (const path of observed.saved) {
    const name = path.replace(/^.*[\\/]\d+-/, '')
    const hash = createHash('sha256').update(readFileSync(path)).digest('hex')
    // A preview's own download control names the file after its title, so match those by content.
    expect([...streamed.values()], `${name} must be bytes the stream delivered`).toContain(hash)
  }
  // The stream: one successful code_interpreter call, in time.
  expect(observed.summary.tools.map(tool => tool.name)).toEqual(['code_interpreter'])
  expect(observed.summary.done).toBe(true)
  expect(observed.summary.doneModel).toBe(PRODUCT_MODEL)
  // The oracle on what the browser downloaded, and the judge for every case.
  for (const proofCase of PROOF_CASES) {
    expect(judgeCase(proofCase, observed), `${proofCase.kind}: the dry run must pass every check the live run makes`).toEqual([])
  }
  // And the judge must be able to say no: the same turn judged as if the model had never called the tool.
  const nothing = { ...observed, summary: { ...observed.summary, tools: [] as TurnSummary['tools'] } }
  expect(judgeCase(PROOF_CASES[0]!, nothing).join(' ')).toContain('never called code_interpreter')
})

test.describe('H5 live', () => {
  test.skip(!LIVE, 'H5_LIVE=1 is required: each case makes ONE real turn on the ChatGPT Terra subscription')

  for (const proofCase of PROOF_CASES) {
    test(`H5 live ${proofCase.kind}: a fresh Terra thread produces a ${proofCase.extension} that the library reads back`, async ({ page }, info) => {
      test.setTimeout(240_000)
      let observed: Observed | null = null
      let problems: string[] = []
      let failure: string | null = null
      const bodyStartedAt = Date.now()
      try {
        await openFresh(page)
        observed = await observeTurn(page, info, proofCase.kind, proofCase.prompt)
        problems = judgeCase(proofCase, observed)
        expect(Date.now() - bodyStartedAt, 'the case must finish inside its budget so the evidence is written').toBeLessThan(TURN_BODY_BUDGET_MS)
      } catch (error) {
        failure = error instanceof Error ? error.name : 'error'
        throw error
      } finally {
        const line = appendEvidence(join(proofDir(), 'evidence.jsonl'), evidenceFor(proofCase, observed, problems, failure))
        console.log(`[H5 ${proofCase.kind}] ${line}`)
        await info.attach(`${proofCase.kind}-evidence`, { body: line, contentType: 'application/json' })
      }
      expect(problems, `H5 ${proofCase.kind}: what fell short of the proof`).toEqual([])
    })
  }
})
