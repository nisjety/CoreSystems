import { expect, test } from '@playwright/test'

/**
 * Z-60 pass 1: a chat turn from a Zero Data Retention organisation fails closed while no model
 * deployment is attested.
 *
 * With ZDR on and nothing attested, inference-core refuses the request (`zdr_unavailable`; a
 * ChatGPT-account connection that cannot give ZDR is `zdr_connection_not_supported`) before any
 * model call. This spec sends ONE real turn and proves what the browser sees: a typed refusal,
 * and no streamed content at all. The zero row delta behind it (threads, messages, runs, cache,
 * feedback, audit outbox) is read by `apps/Model Plane/scripts/tests/zdr-live-proof.sh`, which
 * must bracket this run (`--phase before`, this spec, `--phase after`); the order is in
 * `apps/Model Plane/docs/runbooks/zdr-attestation-and-live-proof.md`.
 *
 * OPT-IN: skipped unless `ZDR_LIVE_TURN=1`. It sends a turn through the live stack, so it runs
 * only for the dedicated test org, never for a real one: the spec reads the signed-in session's
 * org and refuses to send unless its name starts with `ZDR-TEST` and its ZDR switch is on. The
 * test org is seeded with `apps/gateway/scripts/zdr-seed-test-org.mjs`, never through here.
 *
 * Pass 2 (after a real attestation, task Z-40) expects the opposite, a streamed answer, and is
 * selected with `ZDR_LIVE_TURN_EXPECT=answer`; it is not the proof of this pass.
 */

const CHAT = '/chat'
const COMPOSER = 'form textarea'
const REFUSAL_CODES = ['zdr_unavailable', 'zdr_connection_not_supported']
/** Frames that carry model text. A refusal stream may carry none of them. */
const CONTENT_FRAMES = ['TEXT_MESSAGE_CONTENT', 'TEXT_MESSAGE_CHUNK', 'REASONING_MESSAGE_CONTENT', 'REASONING_MESSAGE_CHUNK']
const CHAT_STORAGE_KEYS = [
  'verevon.chat.threadId',
  'verevon.chat.threadHistory.v1',
  'verevon.chat.threadTranscripts.v1',
  'verevon.chat.pendingLaunch',
]

type Frame = { event?: string; type?: string; code?: string; delta?: string; content?: string }

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

test.describe('ZDR live turn (Z-60 pass 1)', () => {
  test('a ZDR-TEST org turn is refused with a typed code and streams no content', async ({ page }) => {
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

    // The model catalogue must agree that nothing is attested: if a chat model carries the zdr
    // feature this turn would be answered, and that is pass 2.
    const models = await page.request.get('/api/v1/models')
    expect(models.status()).toBe(200)
    const catalogue = ((await models.json()) as { models?: { modality?: string; features?: string[] }[] }).models ?? []
    expect(
      catalogue.some((model) => model.modality === 'chat' && (model.features ?? []).includes('zdr')),
      'a chat model is attested for ZDR: this is not the fail-closed state (set ZDR_LIVE_TURN_EXPECT=answer for pass 2)',
    ).toBe(false)

    await page.addInitScript(
      ([keys]) => {
        window.localStorage.setItem('verevon.locale', 'en')
        for (const key of keys as string[]) window.localStorage.removeItem(key)
      },
      [CHAT_STORAGE_KEYS] as const,
    )
    await page.goto(CHAT)
    const composer = page.locator(COMPOSER)
    await expect(composer).toBeVisible()

    const streamResponse = page.waitForResponse(
      (response) => response.url().includes('/api/v1/chat/stream') && response.request().method() === 'POST',
    )
    await composer.click()
    await composer.fill('Say hello.')
    await page.locator('form button[type="submit"]').click()
    const response = await streamResponse
    const text = await response.text()

    // The refusal arrives either as the HTTP error body or as a RUN_ERROR / error frame.
    let code: string | undefined
    let contentFrames = 0
    if (response.status() >= 400) {
      try {
        code = ((JSON.parse(text) as { error?: { code?: string } }).error ?? {}).code
      } catch {
        code = undefined
      }
    } else {
      const frames = sseFrames(text)
      code = frames.find((frame) => frame.code && (frame.type === 'RUN_ERROR' || frame.event === 'error'))?.code
      contentFrames = frames.filter(
        (frame) => CONTENT_FRAMES.includes(frame.type ?? '') || frame.event === 'chunk',
      ).length
    }
    expect(code, 'the turn must end in a typed ZDR refusal').toBeDefined()
    expect(REFUSAL_CODES, `refusal code ${code}`).toContain(code)
    expect(contentFrames, 'a refused turn streams no model content').toBe(0)

    // What the reader sees: the refusal says the request was not sent to a model.
    await expect(page.getByText(/not sent to (a|any) model/i).first()).toBeVisible()
  })
})
