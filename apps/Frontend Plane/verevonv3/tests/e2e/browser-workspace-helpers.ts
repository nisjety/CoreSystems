import { expect, type APIRequestContext, type Page, type Response } from '@playwright/test'

/**
 * Shared helpers for the Phase 6 browser-workspace E2E specs
 * (`browser-workspace-*.spec.ts`). Two families live here:
 *
 * 1. API-direct helpers (`createBrowserSession`, `closeBrowserSession`,
 *    `createBrowserProfile`, `deleteBrowserProfile`) — call the gateway
 *    routes directly. Used by `browser-workspace-zdr.spec.ts`, which only
 *    needs to prove a server-side data-isolation guarantee and gains nothing
 *    from going through the UI.
 *
 * 2. UI-driven helpers (`resetHome`, `openIngestTab`, `submitBrowserUrl`,
 *    `closeSessionViaApi`, `deleteProfileViaApi`, `closeSessionViaUi`) —
 *    drive the REAL Home -> "Innhent" composer UI (`KnowledgeComposer.tsx` +
 *    `BrowserChrome.tsx`), the same surface a human uses, rather than
 *    calling the gateway routes directly, so a regression in the wiring
 *    between the SPA and the gateway (not just the gateway itself, which the
 *    API-direct helpers and `browser-live.spec.ts` already cover) would fail
 *    these specs.
 *
 * Selector policy: `BrowserChrome.tsx`'s approval/status/control labels are
 * plain hardcoded Bokmal strings (never run through `i18n.tr`), so matching
 * on that literal text is locale-independent. `KnowledgeComposer.tsx`'s mode
 * buttons/placeholders/submit label DO go through `i18n.tr(nb, en)`, so
 * structural composer elements are targeted by CSS class instead to stay
 * correct regardless of the account's language preference.
 */

// ---------------------------------------------------------------------------
// API-direct helpers (wire shapes match Quarry's `BrowserObservation`/
// `DomSummary` contracts, `quarry-core/src/contracts.rs`, untouched).
// ---------------------------------------------------------------------------

export interface BrowserDomSummary {
  node_count?: number
  interactive_elements?: unknown[]
  text_snippet?: string | null
}

export interface BrowserObservation {
  url?: string
  title?: string
  dom_summary?: BrowserDomSummary | null
  [key: string]: unknown
}

export interface BrowserSession {
  id: string
  [key: string]: unknown
}

export interface BrowserSessionResponse {
  session: BrowserSession
  observation?: BrowserObservation | null
}

export interface BrowserProfileResponse {
  profile_id: string
  name?: string | null
  scope: string
}

/**
 * Creates a browser session via the gateway and returns its `data` payload
 * (the session plus the observation from its first, automatic navigation to
 * `opts.url`). Pass `profileId` to attach a persistent profile; omit it for
 * an ephemeral, unattached session.
 */
export async function createBrowserSession(
  api: APIRequestContext,
  origin: string,
  opts: { url: string; profileId?: string },
): Promise<BrowserSessionResponse> {
  const response = await api.post('/api/v1/browser/sessions', {
    headers: { 'content-type': 'application/json', origin },
    data: {
      url: opts.url,
      ...(opts.profileId ? { profileId: opts.profileId } : {}),
    },
  })
  if (!response.ok()) {
    throw new Error(`create browser session failed: ${response.status()} ${await response.text()}`)
  }
  const body = await response.json() as { data: BrowserSessionResponse }
  return body.data
}

/** Best-effort session teardown — tolerant of an already-closed session. */
export async function closeBrowserSession(api: APIRequestContext, sessionId: string): Promise<void> {
  await api.delete(`/api/v1/browser/sessions/${encodeURIComponent(sessionId)}`).catch(() => undefined)
}

/** Creates a named, explicitly-scoped persistent profile ("user_private" by default). */
export async function createBrowserProfile(
  api: APIRequestContext,
  origin: string,
  opts: { name: string; scope?: 'user_private' | 'org_shared' | 'run_scoped' },
): Promise<BrowserProfileResponse> {
  const response = await api.post('/api/v1/browser/profiles', {
    headers: { 'content-type': 'application/json', origin },
    data: { name: opts.name, scope: opts.scope ?? 'user_private' },
  })
  if (!response.ok()) {
    throw new Error(`create browser profile failed: ${response.status()} ${await response.text()}`)
  }
  const body = await response.json() as { data: BrowserProfileResponse }
  return body.data
}

/** Best-effort profile teardown — tolerant of an already-deleted profile. */
export async function deleteBrowserProfile(api: APIRequestContext, profileId: string): Promise<void> {
  await api.delete(`/api/v1/browser/profiles/${encodeURIComponent(profileId)}`).catch(() => undefined)
}

// ---------------------------------------------------------------------------
// UI-driven helpers (drive the real Home -> "Innhent" composer + chrome).
// ---------------------------------------------------------------------------

export const BROWSER_PREVIEW_STORAGE_PREFIX = 'verevon.dashboard.browser.preview.'

/** A session created against this session_affinity_key-free target never
 * collides with another test's own session because every `POST
 * /api/v1/browser/sessions` call mints a brand new Quarry `run_id`. */
export const EXAMPLE_URL = 'https://example.com/'

type CreateSessionResponseBody = {
  data: {
    session: { id: string; profile?: { id?: string; scope?: string; storage?: string } }
  }
}

/** Navigate home and drop any stale `browserSession` the composer restores
 * from localStorage (left over from a previous manual/dev session on this
 * browser profile) so every spec starts from a deterministic, sessionless
 * composer. */
export async function resetHome(page: Page): Promise<void> {
  await page.goto('/')
  await page.evaluate((prefix) => {
    for (const key of Object.keys(window.localStorage)) {
      if (key.startsWith(prefix)) window.localStorage.removeItem(key)
    }
  }, BROWSER_PREVIEW_STORAGE_PREFIX)
  await page.reload()
  await openIngestTab(page)
}

/** Click the Home page's "Innhent" (ingest) tab, where the browser workspace
 * composer lives. */
export async function openIngestTab(page: Page): Promise<void> {
  const tab = page.getByRole('button', { name: 'Innhent', exact: true })
  await tab.click()
  await expect(tab).toHaveAttribute('aria-pressed', 'true')
}

/** Fill the composer's URL field and submit it in "Lenke" (link) mode,
 * waiting for the real `POST /api/v1/browser/sessions` round trip and the
 * live BrowserChrome region to render. Returns the real session id (read off
 * the network response, not guessed) so callers can assert against it or
 * clean it up via the API as a belt-and-braces measure. */
export async function submitBrowserUrl(page: Page, url: string): Promise<string> {
  // "Lenke" (link) is the default mode; click it defensively in case a prior
  // interaction in this test left another mode selected.
  await page.locator('.dashboard-knowledge-composer__modes button').first().click()

  const input = page.locator('.dashboard-knowledge-composer__input')
  await expect(input).toBeEnabled({ timeout: 15_000 })
  await input.fill(url)

  const [response] = await Promise.all([
    page.waitForResponse((res: Response) => res.url().includes('/api/v1/browser/sessions') && res.request().method() === 'POST'),
    page.locator('.dashboard-knowledge-composer__submit').click(),
  ])
  expect(response.status(), await response.text()).toBe(200)
  const body = await response.json() as CreateSessionResponseBody
  const sessionId = body.data.session.id
  expect(sessionId).toBeTruthy()

  await expect(page.getByRole('region', { name: 'Nettleser' })).toBeVisible({ timeout: 20_000 })
  await expect(page.locator('.knowledge-browser-status--live')).toBeVisible({ timeout: 20_000 })
  return sessionId
}

/** Best-effort direct API cleanup for a session — the UI's own close button
 * already does this for a happy-path test, but specs call this in a
 * `finally` so a failed assertion mid-test never leaks a live Quarry
 * session/chromium tab. */
export async function closeSessionViaApi(request: APIRequestContext, sessionId: string | null | undefined): Promise<void> {
  if (!sessionId) return
  await closeBrowserSession(request, sessionId)
}

export async function deleteProfileViaApi(request: APIRequestContext, profileId: string | null | undefined): Promise<void> {
  if (!profileId) return
  await deleteBrowserProfile(request, profileId)
}

/** Click the BrowserChrome close button ("Lukk nettleseren og ga tilbake"),
 * which calls the real `closeBrowserSession` client (a genuine `DELETE
 * /api/v1/browser/sessions/:id`) and returns the composer to its pre-session
 * state. Waits for the DELETE response itself — the click handler fires it
 * off without awaiting, so a caller that immediately probes server-side
 * effects of the close (e.g. a profile snapshot flush) right after the click
 * would otherwise race it. */
export async function closeSessionViaUi(page: Page): Promise<void> {
  const closeButton = page.getByRole('button', { name: 'Lukk nettleseren og gå tilbake' })
  if (!(await closeButton.count())) return
  const [response] = await Promise.all([
    page.waitForResponse((res) => res.url().includes('/api/v1/browser/sessions/') && res.request().method() === 'DELETE'),
    closeButton.click(),
  ])
  expect(response.status(), await response.text()).toBe(200)
}
