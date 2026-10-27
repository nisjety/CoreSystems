import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { z } from 'zod'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { cleanWebHistoryText, cleanWebHistoryUrl, webHistoryHost } from '@/features/dashboard/home/web-history'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'
import { omniboxHistoryStorageKey } from '@/shared/session/home-local-content'
import { getSession } from '@/shared/session/session-store'

// The pages a person has navigated to from the browser omnibox, newest first, so
// the dropdown can offer them again. Presentation only: it never reaches the
// server, a model or a grant. Per user and workspace, like the Nett record.
//
// Only what the address bar needs is kept: the URL (http or https, no user name,
// no password, no fragment, and a query only when it is short and carries no
// credential-looking key) and the page title. Page text, screenshots and form
// values are never stored. A zero-data-retention workspace keeps none of it, and
// the purge in home-local-content removes whatever was there.

/** Most entries kept. */
export const OMNIBOX_HISTORY_MAX_ENTRIES = 200
/** A query longer than this is dropped from the stored URL. */
export const OMNIBOX_HISTORY_MAX_QUERY = 256
// A hand-edited record can be any length; only its head is looked at.
const READ_LIMIT = 300
// A query key that looks like it carries a credential: the whole query is dropped.
const CREDENTIAL_KEY = /token|key|secret|password|auth|session|code|sig|jwt/i

export type OmniboxHistoryEntry = {
  url: string
  title: string
  lastVisitedAt: string
  visits: number
}

/**
 * The URL as it may be stored, or null. Scrubbed: http(s) only, user name and
 * password removed, fragment dropped, and the query kept only when it is at most
 * {@link OMNIBOX_HISTORY_MAX_QUERY} characters and no key looks like a credential.
 * `https://example.com/?q=a` keeps `?q=a`; `https://example.com/?token=x` keeps no query.
 */
export function scrubOmniboxUrl(value: unknown): string | null {
  const cleaned = cleanWebHistoryUrl(value)
  if (!cleaned) return null
  const parsed = new URL(cleaned)
  parsed.hash = ''
  const credentialKey = [...parsed.searchParams.keys()].some((key) => CREDENTIAL_KEY.test(key))
  if (credentialKey || parsed.search.length - 1 > OMNIBOX_HISTORY_MAX_QUERY) parsed.search = ''
  return parsed.href
}

const storedSchema = z.object({
  url: z.string(),
  title: z.string().optional(),
  lastVisitedAt: z.string(),
  visits: z.number().int().positive().optional(),
})

function entryFrom(raw: unknown): OmniboxHistoryEntry | null {
  const parsed = storedSchema.safeParse(raw)
  if (!parsed.success) return null
  const url = scrubOmniboxUrl(parsed.data.url)
  const at = Date.parse(parsed.data.lastVisitedAt)
  if (!url || Number.isNaN(at)) return null
  return {
    url,
    title: cleanWebHistoryText(parsed.data.title ?? '') || webHistoryHost(url),
    lastVisitedAt: new Date(at).toISOString(),
    visits: parsed.data.visits ?? 1,
  }
}

/** Newest first; the first entry for a URL wins, and the list is cut to the cap. */
function tidy(entries: OmniboxHistoryEntry[]): OmniboxHistoryEntry[] {
  const sorted = [...entries].sort((a, b) => b.lastVisitedAt.localeCompare(a.lastVisitedAt))
  const seen = new Set<string>()
  const kept: OmniboxHistoryEntry[] = []
  for (const entry of sorted) {
    if (seen.has(entry.url)) continue
    seen.add(entry.url)
    kept.push(entry)
    if (kept.length >= OMNIBOX_HISTORY_MAX_ENTRIES) break
  }
  return kept
}

const isList = (value: unknown): value is unknown[] => Array.isArray(value)

/**
 * What is stored under `key`, newest first. Entries that fail the scrub are
 * skipped, and a corrupt record reads as empty. Nothing is read for a null key.
 */
export function readOmniboxHistory(key: string | null): OmniboxHistoryEntry[] {
  if (!key) return []
  const stored = readClientJson(key, isList)
  if (!stored) return []
  const entries: OmniboxHistoryEntry[] = []
  for (const item of stored.slice(0, READ_LIMIT)) {
    const entry = entryFrom(item)
    if (entry) entries.push(entry)
  }
  return tidy(entries)
}

/**
 * Records a visit under `key` and returns the list now stored. A page that cannot
 * be kept (no key, not an http(s) URL) leaves the record as it was.
 */
export function addOmniboxVisit(
  key: string | null,
  page: { url: string; title?: string | null },
  now: Date = new Date(),
): OmniboxHistoryEntry[] {
  const current = readOmniboxHistory(key)
  if (!key) return current
  const url = scrubOmniboxUrl(page.url)
  if (!url) return current
  const previous = current.find((entry) => entry.url === url)
  const next = tidy([
    {
      url,
      title: cleanWebHistoryText(page.title ?? '') || webHistoryHost(url),
      lastVisitedAt: now.toISOString(),
      visits: (previous?.visits ?? 0) + 1,
    },
    ...current,
  ])
  writeClientJson(key, next)
  return next
}

/** Removes one URL from the record under `key`, and returns what is left. */
export function removeOmniboxEntry(key: string | null, url: string): OmniboxHistoryEntry[] {
  const current = readOmniboxHistory(key)
  if (!key) return current
  const next = current.filter((entry) => entry.url !== url)
  if (next.length === 0) removeClientValue(key)
  else writeClientJson(key, next)
  return next
}

// Bumped by every change made here, so each reader follows the record. It is internal state, so
// `ownedWrite` lets a visit be recorded from inside an owned scope (a root or a computation).
const [revision, setRevision] = createSignal(0, { ownedWrite: true })

// Read when a visit or removal happens, not when a component was created: the
// person or workspace may have changed since.
function currentKey(): string | null {
  return untrack(() => {
    const session = getSession()
    return omniboxHistoryStorageKey(session.user?.id, session.activeOrg?.id)
  })
}

/** The history key of the identity that is current right now, for the omnibox's dropdown. */
export function currentOmniboxHistoryKey(): string | null {
  return currentKey()
}

/**
 * Remembers a page the omnibox navigated to. Nothing is written in a workspace
 * that is zero-data-retention (`zdr`, the session's own flag, or the org's
 * retention verdict as the SPA holds it), nor before the user and workspace are known.
 */
export function recordOmniboxVisit(page: { url: string; title?: string | null }, options: { zdr: boolean }): void {
  if (options.zdr || !isLocalRetentionAllowed()) return
  const key = currentKey()
  if (!key) return
  addOmniboxVisit(key, page)
  setRevision((value) => value + 1)
}

/** Forgets one remembered page for the current user and workspace. */
export function deleteOmniboxEntry(url: string): void {
  removeOmniboxEntry(currentKey(), url)
  setRevision((value) => value + 1)
}

/** Forgets everything for the current user and workspace. Other accounts' records are left alone. */
export function clearOmniboxHistory(): void {
  const key = currentKey()
  if (key) removeClientValue(key)
  setRevision((value) => value + 1)
}

/**
 * The remembered pages for the signed-in person and workspace, newest first. It
 * is read as the component is created, and again after any change made here.
 */
export function useOmniboxHistory(): Accessor<OmniboxHistoryEntry[]> {
  const session = getSession()
  const key = createMemo(() => omniboxHistoryStorageKey(session.user?.id, session.activeOrg?.id))
  return createMemo(() => {
    revision()
    return readOmniboxHistory(key())
  })
}
