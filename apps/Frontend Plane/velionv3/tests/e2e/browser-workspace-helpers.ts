import type { APIRequestContext } from '@playwright/test'

/**
 * Wire shapes for the subset of `/api/v1/browser/*` gateway responses these
 * specs need. Field names are snake_case where they pass through Quarry's
 * `BrowserObservation`/`DomSummary` contracts (`quarry-core/src/contracts.rs`)
 * untouched, and camelCase for gateway-authored fields.
 */
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
