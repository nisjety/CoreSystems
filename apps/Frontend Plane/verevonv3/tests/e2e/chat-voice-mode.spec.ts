import { expect, test, type Page, type Request, type Route } from '@playwright/test'
import { composer, openFresh } from './product-acceptance-helpers'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'
import { collectOffOriginRequests, FAKE_TAKE_TEXT, installVoiceFakes, voiceFakeReport } from './voice-fakes'

/**
 * Voice mode and "Les høyt" on Verevon's governed speech path, end to end in
 * the browser, with every model and speech call mocked:
 * - `/api/v1/ai/dictate`, `/api/v1/ai/speech` and `/api/v1/chat/stream` are
 *   answered by `page.route` (the transport-fixture pattern in
 *   product-experience.spec.ts), and `openFresh(..., { mockedInference })`
 *   aborts any chat stream a fixture does not answer.
 * - The microphone, recorder and audio playback are fakes (voice-fakes.ts),
 *   and the Web Speech globals are counters that must stay at zero.
 *
 * What it proves: one dictate call per take with the "chat message" context
 * and nothing else client-controllable; the spoken turn leaves through the
 * composer's own send with the same model, subscription route and retention
 * flag as a typed one; replies are read through Azure Speech from `blob:`
 * audio; and no request leaves the origin.
 */

const frame = (event: string, data: object, id: number) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`
const assistant = (page: Page) => page.locator('.verevon-chat-message--assistant').last()

const TYPED = 'Hva er status for Team Nord?'
const TYPED_ANSWER = 'Team Nord har seks medarbeidere.'
const SPOKEN = 'Og hvem eier planen?'
const SPOKEN_ANSWER = 'Eva eier **planen** [1].'
const SPOKEN_ANSWER_HEARD = 'Eva eier planen.'
const FAKE_MP3 = Buffer.from('fake mp3').toString('base64')

/** The parts of a chat request that decide where and how it runs, as opposed to what it says. */
const ROUTE_FIELDS = ['model', 'provider', 'subscription_connection_id', 'zdr', 'profile', 'browse_web', 'generate_image', 'plan_mode', 'min_privacy_tier']
function route(body: Record<string, unknown>) {
  const picked = Object.fromEntries(ROUTE_FIELDS.map((field) => [field, body[field]]))
  const features = Array.isArray(body.features) ? [...(body.features as string[])].sort() : body.features
  return { ...picked, features }
}

type Captured = { dictate: Record<string, unknown>[]; speech: Record<string, unknown>[]; stream: Record<string, unknown>[] }

async function mockSpeechAndChat(page: Page, answers: string[]): Promise<Captured> {
  const captured: Captured = { dictate: [], speech: [], stream: [] }
  await page.route('**/api/v1/ai/dictate', (routed: Route) => {
    captured.dictate.push(routed.request().postDataJSON() as Record<string, unknown>)
    return routed.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ text: SPOKEN, raw_text: 'eh og hvem eier planen', cleaned: true, detected_language: 'nb' }),
    })
  })
  await page.route('**/api/v1/ai/speech', (routed: Route) => {
    captured.speech.push(routed.request().postDataJSON() as Record<string, unknown>)
    return routed.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ audio_base64: FAKE_MP3, format: 'mp3', provider_used: 'azure-speech' }),
    })
  })
  // Registered after openFresh's subscription guard, so this runs first.
  await page.route('**/api/v1/chat/stream', (routed: Route) => {
    const body = routed.request().postDataJSON() as Record<string, unknown>
    expect(body).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    captured.stream.push(body)
    const turn = captured.stream.length
    return routed.fulfill({
      status: 200,
      contentType: 'text/event-stream',
      body:
        frame('connected', { request_id: `voice-e2e-${turn}`, ok: true }, 1)
        + frame('chunk', { content: answers[turn - 1] ?? 'Ekstra svar.' }, 2)
        + frame('done', { request_id: `voice-e2e-${turn}`, done: true, model_used: PRODUCT_MODEL }, 3),
    })
  })
  return captured
}

async function sendTyped(page: Page, text: string, answer: string) {
  await composer(page).fill(text)
  await page.locator('form button[type="submit"]').first().click()
  await expect(assistant(page)).toContainText(answer)
}

/**
 * Whether every `/api/v1/models` request the page has made so far has
 * finished. The composer takes a turn's privacy-tier floor
 * (`min_privacy_tier`) from that catalogue, so a turn sent before it arrives
 * goes without one, and a turn sent after carries the model's tier.
 */
function trackModelCatalogue(page: Page): () => boolean {
  const pending = new Set<Request>()
  let finished = 0
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/api/v1/models') pending.add(request)
  })
  const settle = (request: Request) => {
    if (pending.delete(request)) finished += 1
  }
  page.on('requestfinished', settle)
  page.on('requestfailed', settle)
  return () => finished > 0 && pending.size === 0
}

test('a spoken turn is dictated once, sent like a typed one, and read aloud through Azure Speech', async ({ page, baseURL }) => {
  const offOrigin = collectOffOriginRequests(page, new URL(baseURL!).origin)
  const catalogueLoaded = trackModelCatalogue(page)
  await installVoiceFakes(page)
  await openFresh(page, '/chat', { mockedInference: true })
  const captured = await mockSpeechAndChat(page, [TYPED_ANSWER, SPOKEN_ANSWER])

  // A typed turn first: the baseline the spoken one has to match. It waits
  // for the model catalogue, as the spoken turn always does by the time it is
  // sent; otherwise the comparison below measures how fast the catalogue
  // loaded (`min_privacy_tier` absent on one turn, 1 on the other) rather
  // than the voice path.
  await expect.poll(catalogueLoaded).toBe(true)
  await sendTyped(page, TYPED, TYPED_ANSWER)

  await page.getByRole('button', { name: 'Stemmemodus', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Stemmemodus' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('status')).toHaveText('Trykk for å snakke')
  const readReplies = dialog.getByRole('checkbox', { name: 'Les opp svar' })
  await expect(readReplies).not.toBeChecked()
  await readReplies.check()

  // Tap to talk, tap to stop: one take, one dictate call.
  const mic = dialog.getByRole('button', { name: /^(Start opptak|Stopp opptak)$/ })
  await mic.click()
  await expect(mic).toHaveAttribute('aria-pressed', 'true')
  await mic.click()
  await expect(dialog.getByRole('textbox', { name: 'Transkripsjon' })).toHaveValue(SPOKEN)
  await dialog.getByRole('button', { name: 'Send', exact: true }).click()

  await expect.poll(() => captured.stream.length).toBe(2)
  await expect(assistant(page)).toContainText('Eva eier')
  await expect.poll(() => captured.speech.length).toBe(1)
  // Read to the end and back to ready: no failure alert, and the microphone
  // was not re-opened on its own.
  await expect(dialog.getByRole('status')).toHaveText('Trykk for å snakke')
  await expect(dialog.getByRole('alert')).toHaveCount(0)
  await expect(mic).toHaveAttribute('aria-pressed', 'false')

  // The dictate call: the take, its format and language, and only the
  // "chat message" context. No clean-up model override, no retention flag in
  // an ordinary chat.
  expect(captured.dictate).toHaveLength(1)
  expect(captured.dictate[0]).toEqual({
    audio_base64: Buffer.from(FAKE_TAKE_TEXT).toString('base64'),
    format: 'webm',
    language: 'nb-NO',
    context: 'chat message',
  })

  // The spoken turn went through the composer's own send: same model,
  // subscription route, retention flag and features as the typed one.
  const [typed, spoken] = captured.stream
  expect(spoken!.content).toBe(SPOKEN)
  expect(route(spoken!)).toEqual(route(typed!))
  expect(spoken).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })

  // The reply was read through Azure Speech, in the Voice mode language.
  expect(captured.speech[0]).toEqual({
    operation: 'tts',
    text: SPOKEN_ANSWER_HEARD,
    language: 'nb-NO',
    voice: 'nb-NO-PernilleNeural',
    format: 'mp3',
    provider: 'azure-speech',
  })

  const report = await voiceFakeReport(page)
  expect(report.getUserMedia).toBe(1)
  expect(report.recorders).toEqual([{ mimeType: 'audio/webm;codecs=opus', audioBitsPerSecond: 32_000 }])
  expect(report.played.length).toBeGreaterThan(0)
  expect(report.played.every((source) => source.startsWith('blob:'))).toBe(true)
  expect(report.webSpeech).toEqual({ SpeechRecognition: 0, webkitSpeechRecognition: 0, speechSynthesis: 0, SpeechSynthesisUtterance: 0 })
  expect(offOrigin).toEqual([])
})

test('"Les høyt" reads a reply through Azure Speech and can be stopped', async ({ page, baseURL }) => {
  const offOrigin = collectOffOriginRequests(page, new URL(baseURL!).origin)
  // Long clips, so the reading is still going when "Stopp opplesing" is pressed.
  await installVoiceFakes(page, { clipMs: 30_000 })
  await openFresh(page, '/chat', { mockedInference: true })
  const captured = await mockSpeechAndChat(page, [SPOKEN_ANSWER])
  await sendTyped(page, TYPED, 'Eva eier')

  const reply = assistant(page)
  await reply.getByRole('button', { name: 'Flere handlinger' }).click()
  await reply.getByRole('button', { name: 'Les høyt', exact: true }).click()
  await expect.poll(() => captured.speech.length).toBe(1)
  // The UI language's voice, and the words rather than the markdown.
  expect(captured.speech[0]).toEqual({
    operation: 'tts',
    text: SPOKEN_ANSWER_HEARD,
    language: 'nb-NO',
    voice: 'nb-NO-PernilleNeural',
    format: 'mp3',
    provider: 'azure-speech',
  })

  await reply.getByRole('button', { name: 'Flere handlinger' }).click()
  await reply.getByRole('button', { name: 'Stopp opplesing', exact: true }).click()
  await reply.getByRole('button', { name: 'Flere handlinger' }).click()
  await expect(reply.getByRole('button', { name: 'Les høyt', exact: true })).toBeEnabled()
  // Stopped, not failed: no "Opplesing er utilgjengelig" notice.
  await expect(reply.getByRole('alert')).toHaveCount(0)

  const report = await voiceFakeReport(page)
  expect(report.played.length).toBeGreaterThan(0)
  expect(report.played.every((source) => source.startsWith('blob:'))).toBe(true)
  expect(report.getUserMedia).toBe(0)
  expect(report.webSpeech).toEqual({ SpeechRecognition: 0, webkitSpeechRecognition: 0, speechSynthesis: 0, SpeechSynthesisUtterance: 0 })
  expect(offOrigin).toEqual([])
})
