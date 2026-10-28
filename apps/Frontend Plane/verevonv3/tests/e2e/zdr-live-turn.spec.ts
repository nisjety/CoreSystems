import { expect, test, type Page } from '@playwright/test'

/**
 * Z-60: a chat turn from a Zero Data Retention organisation, live. Two tests, one per pass.
 *
 * Pass 1 is the fail-closed proof. With ZDR on and nothing attested, inference-core refuses the
 * request (`zdr_unavailable`; a ChatGPT-account connection that cannot give ZDR is
 * `zdr_connection_not_supported`) before any model call. Its test sends ONE real turn and proves
 * what the browser sees: a typed refusal, and no streamed content at all. The zero row delta behind
 * it (threads, messages, runs, cache, feedback, audit outbox) is read by
 * `apps/Model Plane/scripts/tests/zdr-live-proof.sh`, which must bracket this run (`--phase before`,
 * this spec, `--phase after`); the order is in `apps/Model Plane/docs/runbooks/zdr-attestation-and-live-proof.md`.
 *
 * Pass 2 (after a real attestation, task Z-40) is the opposite: a chat model must list the `zdr`
 * feature in `/api/v1/models`, and one turn must stream an answer with no error frame. It is selected
 * with `ZDR_LIVE_TURN_EXPECT=answer`, which skips pass 1. Its zero-delta proof is the same
 * `zdr-live-proof.sh` bracket (runbook sections 3 to 5).
 *
 * OPT-IN: both tests are skipped unless `ZDR_LIVE_TURN=1`. They send a turn through the live stack,
 * so they run only for the dedicated test org, never for a real one: each reads the signed-in
 * session's org and refuses to send unless its name starts with `ZDR-TEST` and its ZDR switch is on.
 * The test org is seeded with `apps/gateway/scripts/zdr-seed-test-org.mjs`, never through here.
 *
 * Web search is switched off before the turn. ZDR refuses it on its own (`zdr_feature_not_supported`,
 * feature `web_search`) in model-gateway, before the request reaches inference-core, and the composer
 * sends it by default: with it on, pass 1 never reaches the attestation check and pass 2 can never be
 * answered (first live run, 2026-10-28). `ZDR_LIVE_TURN_MODEL` (optional) pins the composer's model for
 * the test org, so a stack that may only use one model is not sent the composer's default mode.
 */

const CHAT = '/chat'
const COMPOSER = 'form textarea'
/** The composer's model choice, with the selectors model-entitlement.spec.ts drives. */
const MODEL_BUTTON = 'button[aria-label="Choose AI model"], button[aria-label="Velg AI-modell"]'
const MODEL_MENU = '.dashboard-composer-model-menu'
const REFUSAL_CODES = ['zdr_unavailable', 'zdr_connection_not_supported']
/** Frames that carry model text. A refusal stream may carry none of them. */
const CONTENT_FRAMES = ['TEXT_MESSAGE_CONTENT', 'TEXT_MESSAGE_CHUNK', 'REASONING_MESSAGE_CONTENT', 'REASONING_MESSAGE_CHUNK']
const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
]
/** The composer's web-search preference (`CHAT_BROWSE_WEB_KEY`); '0' is the explicit opt-out. */
const BROWSE_WEB_KEY = 'verevon.chat.browseWeb.v1'
/** The composer's stored model choice per org (`src/shared/ai/model-selection.ts`). */
const MODEL_SELECTION_PREFIX = 'verevon.ai-model-selection.v1:'

type Frame = {
  event?: string
  type?: string
  code?: string
  delta?: string
  content?: string
  /** The nested envelope the BFF sends on an `error` event (`{"error":{"code":...}}`). */
  error?: { code?: string }
}
/** The fields of one `/api/v1/models` entry that this spec reads. */
type CatalogueModel = { id?: string; name?: string; modality?: string; features?: string[] }

/** Parse an SSE body into its JSON data frames. A line that is not JSON is ignored. */
function sseFrames(body: string): Frame[] {
  const frames: Frame[] = []
  for (const block of body.split(/\r?\n\r?\n/)) {
    let event: string | undefined
    const data: string[] = []
    for (const line of block.split(/\r?\n/)) {
      if (line.startsWith('event:')) event = line.slice(6).trim()
      else if (line.startsWith('data:')) data.push(line.slice(5).trim())
    }
    if (data.length === 0) continue
    try {
      frames.push({ event, ...(JSON.parse(data.join('\n')) as Frame) })
    } catch {
      // A keep-alive or a non-JSON frame carries no refusal and no content.
    }
  }
  return frames
}

/** How many frames in a parsed stream carry model text. */
function contentFrameCount(frames: Frame[]): number {
  return frames.filter((frame) => CONTENT_FRAMES.includes(frame.type ?? '') || frame.event === 'chunk').length
}

/** A chat model that lists the `zdr` feature: the attestation the live catalogue carries. */
function isAttestedChatModel(model: CatalogueModel): boolean {
  return model.modality === 'chat' && (model.features ?? []).includes('zdr')
}

/**
 * The signed-in session's org must be the dedicated ZDR-TEST org with ZDR on. Both tests run it first.
 * Returns the org id.
 */
async function expectZdrTestOrg(page: Page): Promise<string> {
  const current = await page.request.get('/api/v1/session/current')
  expect(current.status(), 'a signed-in session is needed').toBe(200)
  const orgId = ((await current.json()) as { data?: { org?: { id?: string | null } | null } }).data?.org?.id
  expect(orgId, 'an active org is needed').toBeTruthy()
  const org = await page.request.get(`/api/v1/orgs/${orgId}`)
  expect(org.status()).toBe(200)
  const orgBody = (await org.json()) as {
    name?: string
    metadata?: { interactiveRetention?: { zdr?: boolean } }
    data?: { name?: string; metadata?: { interactiveRetention?: { zdr?: boolean } } }
  }
  const name = orgBody.data?.name ?? orgBody.name ?? ''
  const zdr = (orgBody.data?.metadata ?? orgBody.metadata)?.interactiveRetention?.zdr === true
  expect(name, 'this spec sends a turn only for a ZDR-TEST org').toMatch(/^\s*ZDR-TEST/i)
  expect(zdr, 'the ZDR-TEST org must have ZDR on (zdr-seed-test-org.mjs --apply)').toBe(true)
  return orgId as string
}

/** The live model catalogue as the signed-in session sees it. */
async function loadCatalogue(page: Page): Promise<CatalogueModel[]> {
  const models = await page.request.get('/api/v1/models')
  expect(models.status()).toBe(200)
  return ((await models.json()) as { models?: CatalogueModel[] }).models ?? []
}

/**
 * Open a fresh chat with the chat storage cleared and web search off, and wait for the composer.
 * With `ZDR_LIVE_TURN_MODEL` set, the composer's model for this org is that model.
 */
async function openFreshChat(page: Page, orgId: string): Promise<void> {
  const model = process.env.ZDR_LIVE_TURN_MODEL?.trim() ?? ''
  await page.addInitScript(
    ([keys, browseWebKey, selectionKey, pinned]) => {
      window.localStorage.setItem('verevon.locale', 'en')
      for (const key of keys as string[]) window.localStorage.removeItem(key)
      window.localStorage.setItem(browseWebKey as string, '0')
      if (pinned) window.localStorage.setItem(selectionKey as string, JSON.stringify({ model: pinned, label: pinned }))
    },
    [CHAT_STORAGE_KEYS, BROWSE_WEB_KEY, `${MODEL_SELECTION_PREFIX}${orgId}`, model] as const,
  )
  await page.goto(CHAT)
  await expect(page.locator(COMPOSER)).toBeVisible()
}

/** Send one turn from the open chat, and return the stream's HTTP status and its whole body. */
async function sendTurn(page: Page, text: string): Promise<{ status: number; body: string }> {
  const composer = page.locator(COMPOSER)
  const streamResponse = page.waitForResponse(
    (response) => response.url().includes('/api/v1/chat/stream') && response.request().method() === 'POST',
  )
  await composer.click()
  await composer.fill(text)
  await page.locator('form button[type="submit"]').click()
  const response = await streamResponse
  return { status: response.status(), body: await response.text() }
}

/** True when the composer shows its model choice. A composer without one sends with its default model. */
async function hasModelChoice(page: Page): Promise<boolean> {
  try {
    await page.locator(MODEL_BUTTON).first().waitFor({ state: 'visible', timeout: 10_000 })
    return true
  } catch {
    return false
  }
}

/** Pick a model from the composer's model menu, as model-entitlement.spec.ts does. */
async function pickModel(page: Page, name: string): Promise<void> {
  await page.locator(MODEL_BUTTON).first().click()
  const option = page.locator(MODEL_MENU).getByText(name, { exact: true })
  await expect(option, `the composer's model menu lists ${name}`).toBeVisible()
  await option.click()
}

test.describe('ZDR live turn (Z-60)', () => {
  test('pass 1: a ZDR-TEST org turn is refused with a typed code and streams no content', async ({ page }) => {
    test.skip(
      process.env.ZDR_LIVE_TURN !== '1',
      'opt-in: set ZDR_LIVE_TURN=1 to send one real turn. Only for the ZDR-TEST org, with the proof script around it.',
    )
    test.skip(
      process.env.ZDR_LIVE_TURN_EXPECT === 'answer',
      'ZDR_LIVE_TURN_EXPECT=answer is pass 2 (after a real attestation); this test is the fail-closed proof',
    )
    test.setTimeout(120_000)

    // The org must be the dedicated test org with ZDR on. A turn from any other org is never sent.
    const orgId = await expectZdrTestOrg(page)

    // The model catalogue must agree that nothing is attested: if a chat model carries the zdr
    // feature this turn would be answered, and that is pass 2.
    const catalogue = await loadCatalogue(page)
    expect(
      catalogue.some(isAttestedChatModel),
      'a chat model is attested for ZDR: this is not the fail-closed state (set ZDR_LIVE_TURN_EXPECT=answer for pass 2)',
    ).toBe(false)

    await openFreshChat(page, orgId)
    const { status, body: text } = await sendTurn(page, 'Say hello.')

    // The refusal arrives either as the HTTP error body or as a RUN_ERROR / error frame.
    let code: string | undefined
    let contentFrames = 0
    if (status >= 400) {
      try {
        code = ((JSON.parse(text) as { error?: { code?: string } }).error ?? {}).code
      } catch {
        code = undefined
      }
    } else {
      const frames = sseFrames(text)
      code = frames.find((frame) => frame.code && (frame.type === 'RUN_ERROR' || frame.event === 'error'))?.code
      contentFrames = contentFrameCount(frames)
    }
    expect(code, 'the turn must end in a typed ZDR refusal').toBeDefined()
    expect(REFUSAL_CODES, `refusal code ${code}`).toContain(code)
    expect(contentFrames, 'a refused turn streams no model content').toBe(0)

    // What the reader sees: the refusal says the request was not sent to a model.
    await expect(page.getByText(/not sent to (a|any) model/i).first()).toBeVisible()
  })

  test('pass 2: a ZDR-TEST org turn streams an answer from an attested chat model', async ({ page }) => {
    test.skip(
      process.env.ZDR_LIVE_TURN !== '1',
      'opt-in: set ZDR_LIVE_TURN=1 and ZDR_LIVE_TURN_EXPECT=answer, after a real attestation. Only for the ZDR-TEST org.',
    )
    test.skip(
      process.env.ZDR_LIVE_TURN_EXPECT !== 'answer',
      'pass 2 runs only with ZDR_LIVE_TURN_EXPECT=answer, after a real attestation (task Z-40); pass 1 is the fail-closed proof',
    )
    test.setTimeout(120_000)

    // The same org guard as pass 1: a turn is sent only from the dedicated ZDR-TEST org with ZDR on.
    const orgId = await expectZdrTestOrg(page)

    // Pass 2 starts from an attested catalogue. A catalogue with no zdr chat model is still pass 1 territory.
    const attested = (await loadCatalogue(page)).find(isAttestedChatModel)
    expect(
      attested,
      'no chat model lists zdr: this is pass 1 territory (leave ZDR_LIVE_TURN_EXPECT unset until a deployment is attested)',
    ).toBeDefined()

    await openFreshChat(page, orgId)
    // Pick the attested model when the composer offers that choice; otherwise the default model is sent.
    if (await hasModelChoice(page)) {
      await pickModel(page, attested?.name ?? attested?.id ?? '')
    }

    const { status, body: text } = await sendTurn(page, 'Say hello.')
    expect(status, 'the attested model answers: the stream must not fail').toBeLessThan(400)
    const frames = sseFrames(text)
    // An answered turn streams no error frame. Unlike pass 1 this does not require a code: any RUN_ERROR
    // or error frame is a failure, since the chat client reports both as a failed run.
    const errors = frames.filter((frame) => frame.type === 'RUN_ERROR' || frame.event === 'error')
    expect(
      errors.map((frame) => frame.code ?? frame.error?.code ?? frame.type ?? frame.event),
      'an answered turn streams no error frame',
    ).toEqual([])
    expect(contentFrameCount(frames), 'the attested model streams at least one content frame').toBeGreaterThan(0)
  })
})
