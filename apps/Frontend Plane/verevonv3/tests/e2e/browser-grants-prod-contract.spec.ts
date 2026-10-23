import { test, expect, request as playwrightRequest, type APIRequestContext, type APIResponse, type TestInfo } from '@playwright/test'
import {
  GRANT_INACTIVE_MESSAGE,
  GRANT_OTHER_USER_MESSAGE,
  OUTSIDE_DOMAINS_MESSAGE,
  classifyKnownRefusal,
  knownRefusalFromEnv,
  type SeenRefusal,
} from './browser-grants-prod-contract-refusal'

/**
 * SC1 L2 — the production-mode browser grant contract (plan 6.4 A, WS1.A2).
 *
 * API legs only (Playwright's `request` fixture; nothing is rendered), against a
 * stack that runs quarry-edge with the PRODUCTION posture: `docker-compose.yml` +
 * `docker-compose.production.yml` + `docker-compose.grant-contract.yml`,
 * `ENVIRONMENT=production`, the broker's grant TTL at 45 s and the gateway at
 * `BROWSER_GRANT_ENFORCEMENT=strict`. It is NOT part of any default run: the
 * `local` project picks it up, and it skips itself unless
 * `BROWSER_GRANT_PROD_CONTRACT=1`. `scripts/tests/browser-grant-prod-contract.sh`
 * is the wrapper that boots or checks the stack, opens a loopback tunnel to
 * quarry-edge (production publishes no port), runs the `@happy` tests, scans the
 * logs of every hop, then runs the `@negative` tests.
 *
 * Accounts are the seeded development accounts (`local-auth.setup.ts` and
 * `auth.setup.ts`); the second one only proves that a grant is its owner's.
 * Nothing here reads or prints a token or a password: tokens are minted from the
 * signed-in session for the duration of one request and are never logged.
 *
 * Legs:
 *   A (green once WS1.A2 is deployed): A1 the three user tokens mint; A2
 *     `POST /v1/browser/grants` returns a grant; A3 a Quarry start with that grant
 *     and hostile client hosts returns a run; A4 `/step` navigate to example.com is
 *     200 and to another host is refused (the hosts are the broker's); A5 no grant,
 *     forged id, revoked grant (403, not 5xx: G6) and expired grant (after the
 *     TTL) are refused; A6 another user's grant is refused. A7 (no host names in
 *     the broker, model-gateway or gateway logs) is the wrapper's log scan.
 *   B and C (gateway session, AI run through the gateway): they go green with
 *     WS1.B on the production stack. Until then they are inverted by `untilWs1b`
 *     (not `test.fail()`, which would accept a transport error or a 500 as "the
 *     expected failure"): only the KNOWN refusal passes (the session start answered
 *     403 "browser grant_id is required", see browser-grants-prod-contract-refusal.ts;
 *     a 401 or any authentication message never counts, and the sign-in is
 *     verified before the leg runs), every answer is recorded as an annotation (`observed`) before
 *     anything is asserted so the wrapper's transcript carries it, and an
 *     UNEXPECTED PASS fails the suite on purpose. After WS1.B run with
 *     CONTRACT_BC_GREEN=1.
 */

const ENABLED = process.env.BROWSER_GRANT_PROD_CONTRACT === '1'
const AUTH_CORE_URL = (process.env.AUTH_CORE_URL || 'http://localhost:3011').replace(/\/$/, '')
const MODEL_GATEWAY_URL = (process.env.MODEL_GATEWAY_URL || 'http://localhost:8080').replace(/\/$/, '')
/** The loopback tunnel to quarry-edge the wrapper opens (production publishes no port). */
const QUARRY_EDGE_URL = (process.env.QUARRY_EDGE_URL || 'http://127.0.0.1:18082').replace(/\/$/, '')
const LOCAL_ORIGIN = (process.env.LOCAL_E2E_BASE_URL || 'http://localhost:5173').replace(/\/$/, '')
/** The broker's grant TTL on the contract stack (the wrapper sets 45; the broker clamps to 30-3600). */
const GRANT_TTL_SECONDS = Number(process.env.CONTRACT_GRANT_TTL_SECONDS || 45)

const ALLOWED_URL = 'https://example.com'
const ALLOWED_HOST = 'example.com'
const FOREIGN_URL = 'https://evil.example'
/** What a hostile or careless client asks for: wider than the grant. */
const HOSTILE_HOSTS = ['evil.example', 'example.org', ALLOWED_HOST]
const GRANT_ID = /^(z_)?[0-9a-f]{32}$/
const FORGED_GRANT_ID = '00000000000000000000000000000000'

const SECOND_EMAIL = process.env.E2E_EMAIL || 'e2e@verevon.dev'
const SECOND_PASSWORD = process.env.E2E_PASSWORD || 'e2e-Verevon-Pass-123'

// A response body of a service under test: its shape is what the assertions check.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Json = Record<string, any>

/** The one refusal legs B and C show until WS1.B runs on the stack (status and message, pinned). */
const KNOWN_BC_REFUSAL = knownRefusalFromEnv(process.env)
/** Legs B and C are expected to pass (after WS1.B): the inversion below is off. */
const BC_GREEN = process.env.CONTRACT_BC_GREEN === '1'

/**
 * Runs a leg that is expected to fail with a known refusal until WS1.B lands.
 * `seen` records each HTTP answer (status and message only, never a body) as an
 * `observed` annotation before anything is asserted. The sign-in is verified FIRST
 * (`precondition`, outside the inversion), so a signed-out cookie jar is a failure
 * of its own and never reads as the known refusal. The leg then passes only when it
 * throws AFTER the known refusal (classifyKnownRefusal: the session start, the listed
 * status, the missing-grant message word for word, nothing authentication-shaped); a
 * throw with no HTTP answer (transport error), or after any other answer (a 401, a
 * 403 with another message, a 200 that went on to fail, a 404, a 500), is a real
 * failure and is rethrown; a leg that completes is an unexpected pass and fails on
 * purpose. With CONTRACT_BC_GREEN=1 the body runs as an ordinary test.
 */
async function untilWs1b(
  info: TestInfo,
  leg: string,
  precondition: () => Promise<void>,
  body: (seen: (step: string, status: number, message: string) => void) => Promise<void>,
): Promise<void> {
  let last: SeenRefusal | undefined
  const seen = (step: string, status: number, message: string): void => {
    last = { step, status, message }
    info.annotations.push({ type: 'observed', description: `${leg} ${step} status=${status} message=${message}` })
  }
  await precondition()
  if (BC_GREEN) {
    await body(seen)
    return
  }
  try {
    await body(seen)
  } catch (error) {
    if (!last) {
      info.annotations.push({
        type: 'observed',
        description: `${leg} no HTTP answer (${error instanceof Error ? error.name : typeof error})`,
      })
      throw error
    }
    const verdict = classifyKnownRefusal(last, KNOWN_BC_REFUSAL)
    if (!verdict.known) {
      info.annotations.push({ type: 'observed', description: `${leg} NOT the known failure: ${verdict.reason}` })
      throw error
    }
    info.annotations.push({
      type: 'expected-failure',
      description: `${leg} refused at ${last.step} with ${last.status}, the known failure until WS1.B`,
    })
    return
  }
  throw new Error(
    `UNEXPECTED PASS: leg ${leg} is green, so WS1.B is on this stack; set CONTRACT_BC_GREEN=1 (and drop the inversion in this spec)`,
  )
}

interface Tokens {
  browser: string
  model: string
  quarry: string
}

/** One user token for an audience, minted by Auth Core from the signed-in session cookie. */
async function userToken(api: APIRequestContext, audience: string): Promise<string> {
  const response = await api.get(`${AUTH_CORE_URL}/api/${audience}/token`)
  expect(response.status(), `GET /api/${audience}/token (a session cookie must reach Auth Core)`).toBe(200)
  const body = (await response.json()) as Json
  expect(typeof body.token, `${audience} token`).toBe('string')
  return body.token as string
}

/** The signed-in session is real: Auth Core mints a quarry token for it (a 401 here is the test's own failure). */
async function signedIn(api: APIRequestContext): Promise<void> {
  await userToken(api, 'quarry')
}

async function tokensFor(api: APIRequestContext): Promise<Tokens> {
  return {
    browser: await userToken(api, 'browser-broker'),
    model: await userToken(api, 'model-plane'),
    quarry: await userToken(api, 'quarry'),
  }
}

/** The second seeded user, signed in through the same gateway, with its own cookie jar. */
async function secondUser(): Promise<APIRequestContext> {
  const api = await playwrightRequest.newContext({ baseURL: LOCAL_ORIGIN })
  const signin = await api.post('/api/v1/auth/sign-in', {
    headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
    data: { email: SECOND_EMAIL, password: SECOND_PASSWORD },
  })
  expect(signin.ok(), `the second seeded account must sign in (status ${signin.status()})`).toBeTruthy()
  return api
}

/** `POST /v1/browser/grants` at the model-gateway for the user behind `tokens`. */
async function issueGrant(api: APIRequestContext, tokens: Tokens): Promise<{ status: number; body: Json }> {
  const response = await api.post(`${MODEL_GATEWAY_URL}/v1/browser/grants`, {
    headers: {
      authorization: `Bearer ${tokens.model}`,
      'x-browser-authorization': `Bearer ${tokens.browser}`,
      'content-type': 'application/json',
    },
    data: { allowed_domains: [ALLOWED_HOST] },
  })
  return { status: response.status(), body: await safeJson(response) }
}

async function revokeGrant(api: APIRequestContext, tokens: Tokens, grantId: string): Promise<APIResponse> {
  return api.delete(`${MODEL_GATEWAY_URL}/v1/browser/grants/${encodeURIComponent(grantId)}`, {
    headers: {
      authorization: `Bearer ${tokens.model}`,
      'x-browser-authorization': `Bearer ${tokens.browser}`,
    },
  })
}

/** A direct call to quarry-edge with the caller's quarry token. */
async function quarry(
  api: APIRequestContext,
  tokens: Tokens,
  path: string,
  data: Json,
): Promise<{ status: number; body: Json }> {
  const response = await api.post(`${QUARRY_EDGE_URL}${path}`, {
    headers: { authorization: `Bearer ${tokens.quarry}`, 'content-type': 'application/json' },
    data,
  })
  return { status: response.status(), body: await safeJson(response) }
}

async function safeJson(response: APIResponse): Promise<Json> {
  const text = await response.text()
  try {
    return JSON.parse(text) as Json
  } catch {
    return { raw: text.slice(0, 200) }
  }
}

const startBody = (grantId: string): Json => ({
  grant_id: grantId,
  // A client asking for more than the grant: the grant's hosts must win.
  constraints: { max_steps: 20, allowed_domains: HOSTILE_HOSTS },
})

const navigate = (url: string): Json => ({ action: { type: 'navigate', url } })

/** The message of a Quarry error envelope, or ''. */
const errorMessage = (body: Json): string => String(body?.error?.message ?? '')

test.describe('SC1 production-mode browser grants (L2)', () => {
  test.skip(!ENABLED, 'set BROWSER_GRANT_PROD_CONTRACT=1 (the wrapper does) on the production-overlay stack')

  // ---- leg A, the happy path (one chain: the grant is used by the next test) ----------

  test.describe('A1 to A4', () => {
    test.describe.configure({ mode: 'serial' })
    const happy: { grantId?: string; runId?: string } = {}

    test('A1 the three user tokens mint from the signed-in session @happy', async ({ request }) => {
      const tokens = await tokensFor(request)
      expect(tokens.browser).not.toEqual(tokens.model)
      expect(tokens.quarry.split('.')).toHaveLength(3)
    })

    test('A2 the model-gateway issues a session grant to the signed-in user @happy', async ({ request }) => {
      const issued = await issueGrant(request, await tokensFor(request))
      expect(issued.status, JSON.stringify(issued.body)).toBe(201)
      expect(String(issued.body.grant_id)).toMatch(GRANT_ID)
      expect(issued.body.kind).toBe('session')
      happy.grantId = issued.body.grant_id as string
    })

    test('A3 a Quarry start with the grant ignores the hosts the client asked for @happy', async ({ request }) => {
      expect(happy.grantId, 'A2 must have issued a grant').toBeTruthy()
      const started = await quarry(request, await tokensFor(request), '/v1/agent/runs', startBody(happy.grantId!))
      expect(started.status, JSON.stringify(started.body)).toBe(200)
      expect(String(started.body.data?.run_id ?? '')).not.toEqual('')
      happy.runId = started.body.data.run_id as string
    })

    test('A4 a step to the granted host is 200 and to any other host is refused @happy', async ({ request }) => {
      expect(happy.runId, 'A3 must have started a run').toBeTruthy()
      const tokens = await tokensFor(request)
      const allowed = await quarry(request, tokens, `/v1/agent/runs/${happy.runId}/step`, navigate(ALLOWED_URL))
      expect(allowed.status, JSON.stringify(allowed.body)).toBe(200)
      // The client asked for evil.example and example.org in A3; the broker's hosts won.
      const foreign = await quarry(request, tokens, `/v1/agent/runs/${happy.runId}/step`, navigate(FOREIGN_URL))
      expect(foreign.status, JSON.stringify(foreign.body)).toBe(403)
      expect(errorMessage(foreign.body), 'refused for the host, not for another reason').toBe(OUTSIDE_DOMAINS_MESSAGE)
      const sibling = await quarry(request, tokens, `/v1/agent/runs/${happy.runId}/step`, navigate('https://example.org'))
      expect(sibling.status, JSON.stringify(sibling.body)).toBe(403)
      expect(errorMessage(sibling.body), 'refused for the host, not for another reason').toBe(OUTSIDE_DOMAINS_MESSAGE)
    })
  })

  // ---- legs B and C: the known failure until WS1.B is on this stack ---------------------
  //
  // Not `test.fail()`: that accepts ANY throw, so a transport error or a 500 from a
  // broken stack would pass as "the expected failure". `untilWs1b` accepts exactly one
  // shape: the session start answered 403 "browser grant_id is required" (pinned in
  // browser-grants-prod-contract-refusal.ts; a 401 or an authentication message is
  // never the known failure, and the sign-in is verified before each leg). The status and message of every answer are recorded as an
  // `observed` annotation BEFORE anything is asserted, and a transport error records
  // its class and is rethrown, so the transcript always carries what happened.

  test('B a gateway browser session starts with a grant and takes its first step @legBC', async ({ request }, info) => {
    await untilWs1b(info, 'B', () => signedIn(request), async (seen) => {
      const created = await request.post('/api/v1/browser/sessions', {
        headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
        data: { url: ALLOWED_URL },
      })
      const body = await safeJson(created)
      seen('session', created.status(), errorMessage(body))
      try {
        expect(created.status(), JSON.stringify(body).slice(0, 300)).toBe(200)
        expect(String(body.data?.sessionId ?? body.data?.id ?? '')).not.toEqual('')
      } finally {
        const id = String(body.data?.sessionId ?? body.data?.id ?? '')
        if (id) await request.delete(`/api/v1/browser/sessions/${encodeURIComponent(id)}`).catch(() => undefined)
      }
    })
  })

  test('C an AI run through the gateway acts and observes with no 401 or 403 @legBC', async ({ request }, info) => {
    test.setTimeout(150_000)
    await untilWs1b(info, 'C', () => signedIn(request), async (seen) => {
      const created = await request.post('/api/v1/browser/sessions', {
        headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
        data: { url: ALLOWED_URL },
      })
      const session = await safeJson(created)
      const sessionId = String(session.data?.sessionId ?? session.data?.id ?? '')
      seen('session', created.status(), errorMessage(session))
      expect(created.status(), 'the session must be created for the AI run').toBe(200)
      expect(sessionId, 'the session must exist for the AI run').not.toEqual('')
      try {
        const run = await request.post(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/ai-runs`, {
          headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
          data: { goal: 'Open example.com and read its title.', allowed_domains: [ALLOWED_HOST], max_steps: 3 },
        })
        const runBody = await safeJson(run)
        seen('run', run.status(), errorMessage(runBody))
        expect(run.status(), JSON.stringify(runBody).slice(0, 300)).toBe(200)
        const runId = String(runBody.data?.run_id ?? runBody.data?.runId ?? '')
        expect(runId).not.toEqual('')
        await expect
          .poll(
            async () => {
              const replay = await request.get(`/api/v1/runs/${encodeURIComponent(runId)}/events/replay`)
              return replay.ok() ? await replay.text() : ''
            },
            { timeout: 90_000, intervals: [2_000, 3_000, 5_000] },
          )
          .toContain('BrowserObservationReceived')
        const replay = await (await request.get(`/api/v1/runs/${encodeURIComponent(runId)}/events/replay`)).text()
        expect(replay).toContain('BrowserActionDispatched')
        expect(replay).not.toMatch(/\b40[13]\b|PermissionDenied|Unauthenticated|not authorized/)
      } finally {
        await request.delete(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}`).catch(() => undefined)
      }
    })
  })

  // ---- leg A5 and A6, the negative controls (each is self-contained) -------------------

  test('A5a a start with no grant_id is 403 "browser grant_id is required" (the posture is production) @negative', async ({ request }) => {
    const started = await quarry(request, await tokensFor(request), '/v1/agent/runs', {})
    // A 200 here means quarry-edge is not in production mode and the whole run proves nothing.
    expect(started.status, `NOT PROD MODE if 200: ${JSON.stringify(started.body).slice(0, 200)}`).toBe(403)
    expect(errorMessage(started.body)).toBe('browser grant_id is required')
  })

  test('A5b a forged grant id is 403 @negative', async ({ request }) => {
    const started = await quarry(request, await tokensFor(request), '/v1/agent/runs', startBody(FORGED_GRANT_ID))
    expect(started.status, JSON.stringify(started.body)).toBe(403)
    expect(errorMessage(started.body), 'a forged id is a grant the broker does not know, not an authentication failure').toBe(
      GRANT_INACTIVE_MESSAGE,
    )
  })

  test('A5c a revoked grant ends the next step with 403, not a 5xx (G6) @negative', async ({ request }) => {
    const tokens = await tokensFor(request)
    const issued = await issueGrant(request, tokens)
    expect(issued.status, JSON.stringify(issued.body)).toBe(201)
    const grantId = issued.body.grant_id as string
    const started = await quarry(request, tokens, '/v1/agent/runs', startBody(grantId))
    expect(started.status, JSON.stringify(started.body)).toBe(200)
    const runId = started.body.data.run_id as string

    const revoked = await revokeGrant(request, tokens, grantId)
    expect(revoked.status(), 'the grant is revoked at the broker').toBe(200)

    const step = await quarry(request, tokens, `/v1/agent/runs/${runId}/step`, navigate(ALLOWED_URL))
    expect(step.status, JSON.stringify(step.body)).toBe(403)
    expect(errorMessage(step.body)).toBe(GRANT_INACTIVE_MESSAGE)
    // A new run on the revoked grant is refused at the start as well.
    const again = await quarry(request, tokens, '/v1/agent/runs', startBody(grantId))
    expect(again.status, JSON.stringify(again.body)).toBe(403)
    expect(errorMessage(again.body)).toBe(GRANT_INACTIVE_MESSAGE)
  })

  test('A5d a grant past its TTL is 403, not a 5xx @negative', async ({ request }) => {
    test.setTimeout((GRANT_TTL_SECONDS + 90) * 1000)
    const tokens = await tokensFor(request)
    const issued = await issueGrant(request, tokens)
    expect(issued.status, JSON.stringify(issued.body)).toBe(201)
    const grantId = issued.body.grant_id as string
    const started = await quarry(request, tokens, '/v1/agent/runs', startBody(grantId))
    expect(started.status, JSON.stringify(started.body)).toBe(200)
    const runId = started.body.data.run_id as string

    await new Promise((resolve) => setTimeout(resolve, (GRANT_TTL_SECONDS + 5) * 1000))

    // The tokens are 300 s old at most; mint fresh ones anyway so only the grant has expired.
    const fresh = await tokensFor(request)
    const step = await quarry(request, fresh, `/v1/agent/runs/${runId}/step`, navigate(ALLOWED_URL))
    expect(step.status, JSON.stringify(step.body)).toBe(403)
    expect(errorMessage(step.body)).toBe(GRANT_INACTIVE_MESSAGE)
  })

  test("A6 another user's grant is refused with 403 @negative", async ({ request }) => {
    const owner = await tokensFor(request)
    const issued = await issueGrant(request, owner)
    expect(issued.status, JSON.stringify(issued.body)).toBe(201)
    const grantId = issued.body.grant_id as string

    const stranger = await secondUser()
    try {
      const strangerTokens = await tokensFor(stranger)
      const started = await quarry(stranger, strangerTokens, '/v1/agent/runs', startBody(grantId))
      // 403, and for the grant's own reason: the broker does not know it for the stranger's
      // organization (inactive) or the owner differs (another user). A 401/404/5xx is a failure.
      expect(started.status, JSON.stringify(started.body)).toBe(403)
      expect([GRANT_INACTIVE_MESSAGE, GRANT_OTHER_USER_MESSAGE], JSON.stringify(started.body)).toContain(errorMessage(started.body))
    } finally {
      await stranger.dispose()
      await revokeGrant(request, owner, grantId).catch(() => undefined)
    }
  })
})
