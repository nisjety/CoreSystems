import { test, expect, request as playwrightRequest, type APIRequestContext, type APIResponse } from '@playwright/test'

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
 *   B and C (gateway session, AI run through the gateway): written with
 *     `test.fail()`. They go green with WS1.B on the production stack; the failure
 *     they are expected to have is recorded as an annotation (`observed`) so the
 *     wrapper's transcript carries it. An UNEXPECTED PASS fails the suite on
 *     purpose: whoever lands WS1.B on this stack removes `test.fail()` here.
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

type Json = Record<string, any>

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
      const sibling = await quarry(request, tokens, `/v1/agent/runs/${happy.runId}/step`, navigate('https://example.org'))
      expect(sibling.status, JSON.stringify(sibling.body)).toBe(403)
    })
  })

  // ---- legs B and C: expected to fail until WS1.B is on this stack (test.fail) --------

  test('B a gateway browser session starts with a grant and takes its first step @legBC', async ({ request }, info) => {
    test.fail(true, 'expected to fail until WS1.B runs on the production stack; remove with it')
    const created = await request.post('/api/v1/browser/sessions', {
      headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
      data: { url: ALLOWED_URL },
    })
    const body = await safeJson(created)
    // Recorded for the wrapper's transcript: status and message only, never a body.
    info.annotations.push({ type: 'observed', description: `B status=${created.status()} message=${errorMessage(body)}` })
    try {
      expect(created.status(), JSON.stringify(body).slice(0, 300)).toBe(200)
      expect(String(body.data?.sessionId ?? body.data?.id ?? '')).not.toEqual('')
    } finally {
      const id = String(body.data?.sessionId ?? body.data?.id ?? '')
      if (id) await request.delete(`/api/v1/browser/sessions/${encodeURIComponent(id)}`).catch(() => undefined)
    }
  })

  test('C an AI run through the gateway acts and observes with no 401 or 403 @legBC', async ({ request }, info) => {
    test.fail(true, 'expected to fail until WS1.B runs on the production stack; remove with it')
    test.setTimeout(150_000)
    const created = await request.post('/api/v1/browser/sessions', {
      headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
      data: { url: ALLOWED_URL },
    })
    const session = await safeJson(created)
    const sessionId = String(session.data?.sessionId ?? session.data?.id ?? '')
    info.annotations.push({ type: 'observed', description: `C session status=${created.status()} message=${errorMessage(session)}` })
    expect(sessionId, 'the session must exist for the AI run').not.toEqual('')
    try {
      const run = await request.post(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}/ai-runs`, {
        headers: { 'content-type': 'application/json', origin: LOCAL_ORIGIN },
        data: { goal: 'Open example.com and read its title.', allowed_domains: [ALLOWED_HOST], max_steps: 3 },
      })
      const runBody = await safeJson(run)
      info.annotations.push({ type: 'observed', description: `C run status=${run.status()} message=${errorMessage(runBody)}` })
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
    expect(started.status).toBeLessThan(500)
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
    expect(errorMessage(step.body)).toBe('browser grant is inactive or expired')
    // A new run on the revoked grant is refused at the start as well.
    const again = await quarry(request, tokens, '/v1/agent/runs', startBody(grantId))
    expect(again.status, JSON.stringify(again.body)).toBe(403)
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
    expect(errorMessage(step.body)).toBe('browser grant is inactive or expired')
  })

  test("A6 another user's grant is refused (403 or 404) @negative", async ({ request }) => {
    const owner = await tokensFor(request)
    const issued = await issueGrant(request, owner)
    expect(issued.status, JSON.stringify(issued.body)).toBe(201)
    const grantId = issued.body.grant_id as string

    const stranger = await secondUser()
    try {
      const strangerTokens = await tokensFor(stranger)
      const started = await quarry(stranger, strangerTokens, '/v1/agent/runs', startBody(grantId))
      expect([403, 404], JSON.stringify(started.body)).toContain(started.status)
    } finally {
      await stranger.dispose()
      await revokeGrant(request, owner, grantId).catch(() => undefined)
    }
  })
})
