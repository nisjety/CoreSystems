import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { z } from 'zod'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'
import { webHistoryStorageKey } from '@/shared/session/home-local-content'
import { getSession } from '@/shared/session/session-store'

export { webHistoryStorageKey }

// Browser-only memory for the Nett card "Fortsett der du slapp": the last web
// searches that were run and the last pages opened from their results. There is
// no server-side search or visit history, so this is the whole record. It is
// presentation only: it never reaches the server, feeds a model or grants access.
//
// The record belongs to the workspace and user it was made under, like the
// layout record (home-layout-state). Nothing is read or written until both ids
// are known, and a write only ever goes to the key of the identity that is
// current at that moment, never one captured earlier. Sign-out leaves the record
// in place, as it does for the other per-account keys: the identity in the key
// is the isolation, and the next account computes a different key.
//
// A workspace that is zero-data-retention allows no local copy of what a person
// searched for or opened, so nothing is recorded there, and the moment the server
// says so everything stored is removed (chat-retention purges this record along
// with the chat history).
//
// Everything read back is checked again. The card renders these URLs as links,
// so an entry that was edited by hand, or written by another build, is dropped
// unless it is a plain http(s) URL.

/** The most entries kept, searches and pages together. */
export const WEB_HISTORY_MAX_ENTRIES = 10
/** Longest stored query or page title. */
export const WEB_HISTORY_MAX_TEXT = 200
/** Longest stored URL. A longer one is dropped rather than cut, because a cut URL is another page. */
export const WEB_HISTORY_MAX_URL = 2048
/** How many rows the card shows, as many as the recent-work card it stands in for. */
export const WEB_HISTORY_CARD_ROWS = 4
// A hand-edited record can be any length; only its head is looked at.
const READ_LIMIT = 100

export type WebHistoryEntry =
  | { kind: 'query'; text: string; at: string }
  | { kind: 'page'; text: string; url: string; at: string }

/** What a caller hands over; the timestamp is taken when it is stored. */
export type WebHistoryInput =
  | { kind: 'query'; text: string }
  | { kind: 'page'; text?: string | null; url: string }

// Control characters, bidirectional overrides and zero-width marks: none belong in a one-line label.
const invisibleCharacters = /[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069\ufeff]+/g

function cap(text: string, max: number): string {
  if (text.length <= max) return text
  const cut = text.slice(0, max)
  const last = cut.charCodeAt(cut.length - 1)
  // Never end on half of a surrogate pair.
  return last >= 0xd800 && last <= 0xdbff ? cut.slice(0, -1) : cut
}

/** One trimmed line, without control characters, cut to {@link WEB_HISTORY_MAX_TEXT}. */
export function cleanWebHistoryText(value: string): string {
  return cap(value.replace(invisibleCharacters, ' ').replace(/\s+/g, ' ').trim(), WEB_HISTORY_MAX_TEXT).trimEnd()
}

/**
 * The URL as it may be stored, or null. Only absolute http and https URLs
 * pass, and any user name or password in them is removed.
 */
export function cleanWebHistoryUrl(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (!trimmed || trimmed.length > WEB_HISTORY_MAX_URL) return null
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return null
  }
  if ((parsed.protocol !== 'http:' && parsed.protocol !== 'https:') || !parsed.hostname) return null
  parsed.username = ''
  parsed.password = ''
  return parsed.href.length > WEB_HISTORY_MAX_URL ? null : parsed.href
}

/** The address without a leading www., as the search results show it. */
export function webHistoryHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

const toInstant = (value: string) => {
  const time = Date.parse(value)
  return Number.isNaN(time) ? null : new Date(time).toISOString()
}

const storedEntrySchema = z.object({
  kind: z.enum(['query', 'page']),
  text: z.string(),
  url: z.string().optional(),
  at: z.string(),
})

/** Validates one stored or incoming entry and returns it cleaned, or null when it cannot be kept. */
export const webHistoryEntrySchema = storedEntrySchema.transform((raw): WebHistoryEntry | null => {
  const at = toInstant(raw.at)
  if (!at) return null
  const text = cleanWebHistoryText(raw.text)
  if (raw.kind === 'query') return text ? { kind: 'query', text, at } : null
  const url = cleanWebHistoryUrl(raw.url)
  return url ? { kind: 'page', text: text || webHistoryHost(url), url, at } : null
})

/** The same search (ignoring case) or the same page URL counts as one entry. */
const sameAs = (entry: WebHistoryEntry) => (entry.kind === 'page' ? `page:${entry.url}` : `query:${entry.text.toLowerCase()}`)

/** Newest first: the first of each duplicate wins, and the list is cut to the cap. */
function tidy(entries: WebHistoryEntry[]): WebHistoryEntry[] {
  const seen = new Set<string>()
  const kept: WebHistoryEntry[] = []
  for (const entry of entries) {
    const id = sameAs(entry)
    if (seen.has(id)) continue
    seen.add(id)
    kept.push(entry)
    if (kept.length >= WEB_HISTORY_MAX_ENTRIES) break
  }
  return kept
}

const isList = (value: unknown): value is unknown[] => Array.isArray(value)

/** What is stored under `key`, newest first. Invalid entries are skipped; a corrupt record reads as empty. */
export function readWebHistory(key: string | null): WebHistoryEntry[] {
  if (!key) return []
  const stored = readClientJson(key, isList)
  if (!stored) return []
  const entries: WebHistoryEntry[] = []
  for (const item of stored.slice(0, READ_LIMIT)) {
    const parsed = webHistoryEntrySchema.safeParse(item)
    if (parsed.success && parsed.data) entries.push(parsed.data)
  }
  return tidy(entries)
}

/**
 * Puts an entry at the top of what is stored under `key`, replacing an earlier
 * copy of the same search or page, and returns the list now stored. An input
 * that cannot be kept (no key, an empty search, a URL that is not http or https)
 * leaves the record as it was.
 */
export function addWebHistory(key: string | null, input: WebHistoryInput, now: Date = new Date()): WebHistoryEntry[] {
  const current = readWebHistory(key)
  if (!key) return current
  const parsed = webHistoryEntrySchema.safeParse({
    kind: input.kind,
    text: input.text ?? '',
    url: input.kind === 'page' ? input.url : undefined,
    at: now.toISOString(),
  })
  if (!parsed.success || !parsed.data) return current
  const next = tidy([parsed.data, ...current])
  writeClientJson(key, next)
  return next
}

export function removeWebHistory(key: string | null): void {
  if (key) removeClientValue(key)
}

// Bumped by every change made here, so each card showing the record follows it.
const [revision, setRevision] = createSignal(0)

// Read when a search or click happens, not when a component was created: the
// person or workspace may have changed since.
function currentKey(): string | null {
  return untrack(() => {
    const session = getSession()
    return webHistoryStorageKey(session.user?.id, session.activeOrg?.id)
  })
}

// Where a search or page may be recorded, or null: the user and workspace are not
// known yet, or the workspace does not allow local copies.
function recordingKey(): string | null {
  return isLocalRetentionAllowed() ? currentKey() : null
}

/** Remembers a web search that was run. Does nothing until the user and workspace are known, or in a zero-retention workspace. */
export function recordWebQuery(text: string): void {
  const key = recordingKey()
  if (!key) return
  addWebHistory(key, { kind: 'query', text })
  setRevision((value) => value + 1)
}

/** Remembers a page opened from the search results. Anything but an http(s) URL is ignored. */
export function recordWebPage(page: { url: string; title?: string | null }): void {
  const key = recordingKey()
  if (!key) return
  addWebHistory(key, { kind: 'page', text: page.title, url: page.url })
  setRevision((value) => value + 1)
}

/** Forgets everything for the current user and workspace. Other accounts' records are left alone. */
export function clearWebHistory(): void {
  removeWebHistory(currentKey())
  setRevision((value) => value + 1)
}

/**
 * The remembered searches and pages for the signed-in person and workspace,
 * newest first. It is read as the component is created, so a known identity
 * paints its history on the first frame, and read again when the person or
 * workspace changes or anything is recorded or cleared.
 */
export function useWebHistory(): Accessor<WebHistoryEntry[]> {
  const session = getSession()
  const key = createMemo(() => webHistoryStorageKey(session.user?.id, session.activeOrg?.id))
  return createMemo(() => {
    revision()
    return readWebHistory(key())
  })
}

/**
 * The rows the card shows. Searches and pages each get up to half of them, so a
 * run of opened pages cannot push every search out of sight (or the other way
 * round); rows one kind leaves unused go to the other. Still newest first.
 */
export function webHistoryCardRows(entries: readonly WebHistoryEntry[], rows: number = WEB_HISTORY_CARD_ROWS): WebHistoryEntry[] {
  const chosen = new Set<WebHistoryEntry>()
  const half = Math.ceil(rows / 2)
  for (const kind of ['query', 'page'] as const) {
    for (const entry of entries.filter((candidate) => candidate.kind === kind).slice(0, half)) chosen.add(entry)
  }
  for (const entry of entries) {
    if (chosen.size >= rows) break
    chosen.add(entry)
  }
  return entries.filter((entry) => chosen.has(entry)).slice(0, rows)
}
