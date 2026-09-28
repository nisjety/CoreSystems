import { expect, test } from '@playwright/test'
import { PRODUCT_MODEL, PRODUCT_PROVIDER } from './product-subscription-route'
import { composer, openFresh, transcript } from './product-acceptance-helpers'
import { writeGateEvidence } from './gate-evidence'

/**
 * Recovery gate (see evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md §7
 * and RECORDING-WORKFLOW.md step 3): unreadable input, provider timeout, an
 * unavailable connector or expired authorization must all produce an
 * ACTIONABLE state — never a silently-lost turn, never a silently-invented
 * answer, never a silent switch to another provider — and a retry must
 * actually work.
 *
 * Everything below is mocked transport (`page.route`); no live model call is
 * made anywhere in this file.
 *
 * Scope note — no literal `page.reload()` case here: Playwright's
 * `route.fulfill` delivers a complete response body in one call, so it
 * cannot hold an SSE connection open mid-answer and then let a real
 * navigation interrupt it — genuinely reproducing "reload mid-turn" needs
 * either a live run (session-core actually tracking an active run to
 * reattach to across the reload) or a hand-rolled streaming HTTP server,
 * which is out of proportion for what this pass needs to prove. That case is
 * exactly the one bounded LIVE check design investigation 3 already
 * recommends: the coordinator reuses the EXISTING
 * `product-experience.spec.ts` "Q08 live reload resumes the same run
 * without another invocation" test, run once bracketing a real
 * `model-plane-model-gateway-1` restart — not duplicated here, since this
 * pass allows no live model calls.
 */
const frame = (event: string, data: object, id: number) => `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`

test('Recovery: a dropped connection auto-resumes into one complete turn, with no new invocation', async ({ page }, info) => {
  await openFresh(page, '/chat', { mockedInference: true })
  let invocations = 0
  let resumes = 0
  page.on('request', (r) => {
    if (r.url().endsWith('/api/v1/chat/stream') && r.method() === 'POST') invocations++
    if (r.url().includes('/api/v1/chat/stream/resume/') && r.method() === 'GET') resumes++
  })
  await page.route('**/api/v1/chat/stream', (route) => {
    expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
    // Connected + one chunk, then the response simply ends: no `done`, no
    // `error`, no `stopped`. That is a lost connection, not a cancel — the
    // controller must reattach on its own, without any user action or reload.
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('connected', { request_id: 'q-recovery-drop-1', thread_id: 'Q-RECOVERY-DROP-THREAD-1', ok: true }, 1) +
      frame('chunk', { content: 'Delvis svar før forbindelsen brytes.' }, 2) })
  })
  let resumeCursor: string | undefined
  await page.route('**/api/v1/chat/stream/resume/*', (route) => {
    // The client must actually ask the server to skip what it already has:
    // capture the cursor it sent, so a real bug in cursor construction (e.g.
    // always resuming from 0, or from the wrong frame id) fails this test
    // instead of being masked by the mock's fixed response.
    resumeCursor = route.request().headers()['last-event-id']
    // Frame ids continue past the initial stream's highest id (2): the
    // resumed stream is an incremental continuation (a Last-Event-ID cursor
    // was sent), not a full replay, so ids 1-2 would be deduped and dropped.
    return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
      frame('chunk', { content: ' Resten av svaret etter at forbindelsen kom tilbake.' }, 3) +
      frame('done', { request_id: 'q-recovery-drop-1', model_used: PRODUCT_MODEL }, 4) })
  })
  await composer(page).fill('Transporttest: mist forbindelsen og hent svaret automatisk.')
  await page.locator('form button[type="submit"]').first().click()
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content ?? '', { timeout: 20_000 }).toContain('Delvis svar')
  await expect.poll(() => resumes, { timeout: 20_000, message: 'a dropped connection must trigger an automatic resume' }).toBeGreaterThan(0)
  await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.content ?? '', { timeout: 20_000 }).toContain('Resten av svaret')
  await expect(page.locator('form').getByRole('button', { name: 'Stopp svar', exact: true })).toBeHidden({ timeout: 20_000 })
  const finalTranscript = await transcript(page)
  expect(finalTranscript?.turns.filter((t) => t.role === 'user'), 'no duplicate user turn from the reconnect').toHaveLength(1)
  expect(finalTranscript?.turns.filter((t) => t.role === 'assistant'), 'no duplicate assistant turn from the reconnect').toHaveLength(1)
  expect(finalTranscript?.turns.at(-1)?.content).toBe('Delvis svar før forbindelsen brytes. Resten av svaret etter at forbindelsen kom tilbake.')
  expect(invocations, 'the dropped connection must not cause a second POST invocation').toBe(1)
  expect(resumes, 'exactly one resume GET for the one dropped connection').toBe(1)
  // The cursor itself, not just that a resume happened: it must equal the
  // highest frame id actually delivered before the drop (2), so a client bug
  // that always resumes from 0 or from the wrong id would fail here instead
  // of being hidden by the mock's fixed continuation frames.
  expect(resumeCursor, 'resume must send the Last-Event-ID of the highest frame already applied').toBe('2')
  await expect(composer(page)).toBeEnabled()
  await page.screenshot({ path: info.outputPath('recovered-dropped-connection.png'), animations: 'disabled' })
  const summary = { case: 'dropped-connection-auto-resume', invocations, resumes, finalContent: finalTranscript?.turns.at(-1)?.content, checkedAt: new Date().toISOString() }
  await info.attach('recovery-summary', { body: JSON.stringify(summary), contentType: 'application/json' })
  writeGateEvidence('GATE_EVIDENCE_DIR', 'recovery-dropped-connection-auto-resume', summary)
})

/**
 * The exact codes the broker can emit on the wire the chat client actually
 * receives (`chat-normalizers.ts`'s `streamFailurePresentation` switch, and
 * model-gateway's `subscription_failure.rs` `ALL_CODES`): usage limit,
 * expired authorization (reauth), an unavailable upstream, an unavailable
 * subscription ROUTE, a malformed request, and provider rate limiting.
 *
 * `subscription_context_too_long` is deliberately NOT in this list even
 * though evidence/PRODUCT_RECORDING_READINESS_2026-09-24.md's prose mentions
 * it: it is inference-core's own internal constant for an overlong-context
 * refusal (`codex_subscription.rs`), and model-gateway translates that
 * failure into `prompt_too_long` before it ever reaches the client (see
 * `subscription_failure.rs`'s `PROMPT_TOO_LONG` and `session_flow.rs`'s
 * `sf::PROMPT_TOO_LONG` mapping) — a code `streamFailurePresentation`
 * already has its own case for. The literal string
 * `subscription_context_too_long` never arrives as a `code` field in
 * practice, and `streamFailurePresentation` has no case for it either, so
 * using it here as a stand-in for one of the six real subscription/rate-limit
 * codes both tested an unreachable wire value and silently skipped the real
 * sixth code, `subscription_route_unavailable`.
 *
 * Each recognized code gets its OWN fixed, content-free, bilingual message
 * and its own retry affordance from `chat-normalizers.ts`'s
 * `streamFailurePresentation` (`.verevon-chat-error-notice` in
 * `ChatMessages.tsx`'s `ErrorNotice`) — this is real, already-shipped
 * "actionable, never a silent alternate provider" behavior, not something
 * this test invents. `message`/`hasUnsaved`/`retryVia` below are read
 * straight from that source (with no `resetsAt`/`retryAfterMs` supplied in
 * the mocked frame, matching each function's own no-timing-info branch):
 *  - `retryVia: 'regenerate'` — the notice's own "Prøv igjen" button
 *    (`onRetry={regenerateLatest}`) truncates the failed turn and resends
 *    the SAME user turn: turn count stays 1 user + 1 assistant. Shown for
 *    every code except `action: 'none'` (`ErrorNotice`'s
 *    `<Show when={action() !== 'none'}>`), so both `action: 'retry'` and
 *    `action: 'reconnect'` codes retry this way.
 *  - `retryVia: 'newMessage'` — `subscription_bad_request` is `action:
 *    'none'` ("sending the same turn again cannot help"): no quick button is
 *    offered by design, so recovery here means the composer stays usable for
 *    an edited/new message, which appends a fresh user+assistant pair.
 */
const FAILURE_CODES = [
  'subscription_usage_limit',
  'subscription_reauth_required',
  'subscription_upstream_unavailable',
  'subscription_route_unavailable',
  'subscription_bad_request',
  'rate_limited',
] as const

const EXPECTED: Record<(typeof FAILURE_CODES)[number], { message: string; hasUnsaved: boolean; reconnectLink: boolean; retryVia: 'regenerate' | 'newMessage' }> = {
  subscription_usage_limit: {
    message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Tidspunktet for tilbakestilling er ukjent; sjekk bruken i ChatGPT og prøv igjen senere. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: false, retryVia: 'regenerate',
  },
  subscription_reauth_required: {
    message: 'ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt under Profil → ChatGPT-abonnement, og prøv igjen. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: true, retryVia: 'regenerate',
  },
  subscription_upstream_unavailable: {
    message: 'ChatGPT svarte ikke akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: false, retryVia: 'regenerate',
  },
  subscription_route_unavailable: {
    message: 'Den valgte ChatGPT-abonnementstilkoblingen kunne ikke fullføre svaret. Prøv igjen om litt. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: false, retryVia: 'regenerate',
  },
  subscription_bad_request: {
    message: 'ChatGPT avviste forespørselen. Endre eller kort ned meldingen, og prøv igjen. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: false, retryVia: 'newMessage',
  },
  rate_limited: {
    message: 'ChatGPT begrenser forespørslene akkurat nå. Prøv igjen om litt. Ingen annen modell ble brukt.',
    hasUnsaved: true, reconnectLink: false, retryVia: 'regenerate',
  },
}

for (const code of FAILURE_CODES) {
  test(`Recovery: "${code}" is actionable, keeps the partial answer, and a retry then succeeds`, async ({ page }, info) => {
    const expected = EXPECTED[code]
    await openFresh(page, '/chat', { mockedInference: true })
    let invocations = 0
    await page.route('**/api/v1/chat/stream', (route) => {
      expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
      invocations++
      if (invocations === 1) {
        return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
          frame('connected', { request_id: `q-recovery-${code}`, ok: true }, 1) +
          frame('chunk', { content: 'Et delvis resultat før feilen.' }, 2) +
          frame('error', { code, message: `Simulert feil: ${code}`, retryable: true }, 3) })
      }
      // The retry: a normal, independent second invocation succeeds.
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
        frame('connected', { request_id: `q-recovery-${code}-retry`, ok: true }, 1) +
        frame('chunk', { content: 'Svar etter et vellykket forsøk.' }, 2) +
        frame('done', { request_id: `q-recovery-${code}-retry`, model_used: PRODUCT_MODEL }, 3) })
    })
    await composer(page).fill(`Transporttest for ${code}: behold det delvise svaret og vis en tydelig feil.`)
    await page.locator('form button[type="submit"]').first().click()

    const notice = page.locator('.verevon-chat-error-notice')
    await expect(notice).toBeVisible()
    await expect(notice.locator('p').first()).toContainText(expected.message)
    // Every recognized subscription/rate-limit code says, in its own fixed
    // words, that no other model was used — the literal, user-facing form of
    // "never silently invokes another provider".
    expect(expected.message).toContain('Ingen annen modell ble brukt')
    if (expected.hasUnsaved) {
      const unsaved = notice.locator('.verevon-chat-error-notice__unsaved')
      await unsaved.locator('summary').click()
      await expect(unsaved).toContainText('Et delvis resultat')
    }
    if (expected.reconnectLink) await expect(notice.getByRole('link', { name: 'Koble til ChatGPT på nytt' })).toBeVisible()
    await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.status).toBe('error')
    await expect(page.getByRole('status').filter({ hasText: 'Svaret mislyktes.' })).toBeVisible()
    await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toHaveCount(0)
    expect(invocations, `"${code}" must never silently invoke a different provider or model`).toBe(1)
    await expect(composer(page)).toBeEnabled()
    await page.screenshot({ path: info.outputPath(`recovery-${code}.png`), animations: 'disabled' })

    // Retry must actually work — recovery is not just "fails visibly", it is
    // "and then a retry completes" — using the SAME affordance the product
    // actually offers for this code (see EXPECTED's retryVia doc comment).
    const before = await transcript(page)
    if (expected.retryVia === 'regenerate') {
      await notice.getByRole('button', { name: 'Prøv igjen', exact: true }).click()
    } else {
      await composer(page).fill('Kortere melding etter avvisningen.')
      await page.locator('form button[type="submit"]').first().click()
    }
    await expect(page.locator('.verevon-chat-message--assistant').last()).toContainText('vellykket forsøk')
    await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toBeVisible()
    expect(invocations).toBe(2)
    const retried = await transcript(page)
    // 'regenerate' truncates the failed assistant turn and appends its
    // replacement in place (net assistant count unchanged); 'newMessage' is a
    // genuinely independent second exchange (both roles gain one turn).
    const addedTurn = expected.retryVia === 'newMessage' ? 1 : 0
    const expectedUserCount = (before?.turns.filter((t) => t.role === 'user').length ?? 0) + addedTurn
    const expectedAssistantCount = (before?.turns.filter((t) => t.role === 'assistant').length ?? 0) + addedTurn
    expect(retried?.turns.filter((t) => t.role === 'user'), 'retry must not duplicate or drop the user turn').toHaveLength(expectedUserCount)
    expect(retried?.turns.filter((t) => t.role === 'assistant'), 'retry must not duplicate or drop the assistant turn').toHaveLength(expectedAssistantCount)
    await page.screenshot({ path: info.outputPath(`recovery-${code}-retried.png`), animations: 'disabled' })
    const summary = { case: code, retryVia: expected.retryVia, invocationsBeforeRetry: 1, invocationsAfterRetry: invocations, recovered: true, checkedAt: new Date().toISOString() }
    await info.attach('recovery-summary', { body: JSON.stringify(summary), contentType: 'application/json' })
    writeGateEvidence('GATE_EVIDENCE_DIR', `recovery-failure-code-${code}`, summary)
  })
}

/**
 * A draft that could not be checked against its sources in time or within
 * the attempt limit (Q16b journey 03 take 17 hit the timeout live). The
 * client has no case of its own for these codes: `streamFailurePresentation`
 * shows the gateway's own message (model-gateway `sse.rs`
 * `tool_phase_explanation`, copied verbatim below). The unchecked draft is
 * never published, no other model is invoked, and "Prøv igjen" resends the
 * same turn, which then completes.
 */
const VALIDATION_FAILURES = {
  response_validation_timeout: 'Arbeidet med utkastet tok for lang tid. Dette utkastet er ikke publisert. Prøv igjen.',
  response_validation_failed: 'Et utkast kunne ikke kontrolleres mot kildene innen forsøksgrensen. Dette utkastet er ikke publisert. Prøv igjen med en mindre endring eller tydeligere kilder.',
} as const
for (const [code, message] of Object.entries(VALIDATION_FAILURES)) {
  test(`Recovery: a draft "${code}" is actionable, publishes nothing unchecked, and a retry then succeeds`, async ({ page }, info) => {
    await openFresh(page, '/chat', { mockedInference: true })
    let invocations = 0
    await page.route('**/api/v1/chat/stream', (route) => {
      expect(route.request().postDataJSON()).toMatchObject({ model: PRODUCT_MODEL, provider: PRODUCT_PROVIDER })
      invocations++
      if (invocations === 1) {
        return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
          frame('connected', { request_id: `q-recovery-${code}`, ok: true }, 1) +
          frame('error', { code, message, retryable: true }, 2) })
      }
      return route.fulfill({ status: 200, contentType: 'text/event-stream', body:
        frame('connected', { request_id: `q-recovery-${code}-retry`, ok: true }, 1) +
        frame('chunk', { content: 'Svar etter et vellykket forsøk.' }, 2) +
        frame('done', { request_id: `q-recovery-${code}-retry`, model_used: PRODUCT_MODEL }, 3) })
    })
    await composer(page).fill(`Transporttest for ${code}: vis en tydelig feil og ikke publiser et ukontrollert utkast.`)
    await page.locator('form button[type="submit"]').first().click()

    const notice = page.locator('.verevon-chat-error-notice')
    await expect(notice).toBeVisible()
    await expect(notice.locator('p').first()).toContainText(message)
    await expect.poll(async () => (await transcript(page))?.turns.at(-1)?.status).toBe('error')
    expect((await transcript(page))?.turns.flatMap(turn => turn.artifacts ?? []), 'nothing unchecked is published').toEqual([])
    await expect(page.getByRole('status').filter({ hasText: 'Svaret mislyktes.' })).toBeVisible()
    expect(invocations, `"${code}" must never silently invoke a different provider or model`).toBe(1)
    await expect(composer(page)).toBeEnabled()
    await page.screenshot({ path: info.outputPath(`recovery-${code}.png`), animations: 'disabled' })

    const before = await transcript(page)
    await notice.getByRole('button', { name: 'Prøv igjen', exact: true }).click()
    await expect(page.locator('.verevon-chat-message--assistant').last()).toContainText('vellykket forsøk')
    await expect(page.getByRole('status').filter({ hasText: 'Svar fullført.' })).toBeVisible()
    expect(invocations).toBe(2)
    const retried = await transcript(page)
    expect(retried?.turns.filter(turn => turn.role === 'user'), 'retry must not duplicate or drop the user turn').toHaveLength(before?.turns.filter(turn => turn.role === 'user').length ?? 0)
    expect(retried?.turns.filter(turn => turn.role === 'assistant'), 'retry must not duplicate or drop the assistant turn').toHaveLength(before?.turns.filter(turn => turn.role === 'assistant').length ?? 0)
    const summary = { case: code, retryVia: 'regenerate', invocationsBeforeRetry: 1, invocationsAfterRetry: invocations, recovered: true, checkedAt: new Date().toISOString() }
    await info.attach('recovery-summary', { body: JSON.stringify(summary), contentType: 'application/json' })
    writeGateEvidence('GATE_EVIDENCE_DIR', `recovery-failure-code-${code}`, summary)
  })
}
