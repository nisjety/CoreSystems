import { createMemo, createSignal, untrack, type Accessor } from 'solid-js'
import { z } from 'zod'
import { isLocalRetentionAllowed } from '@/features/chat/lib/chat-retention'
import { readClientJson, removeClientValue, writeClientJson } from '@/shared/session/client-storage'
import { bookmarksStorageKey } from '@/shared/session/home-local-content'
import { getSession } from '@/shared/session/session-store'

// Browser-only bookmarks for the in-app browser (F-t2-068, local slice).
// Presentation only: the list lives on this device per user and workspace,
// never reaches the server, feeds no model and grants no access. A
// zero-retention workspace records nothing and purges what is stored
// (chat-retention / home-local-content). Everything read back is validated:
// only plain http(s) URLs without credentials are kept.

export const BOOKMARKS_MAX_ENTRIES = 50
export const BOOKMARKS_MAX_LABEL = 120
export const BOOKMARKS_MAX_URL = 2048

export type BrowserBookmark = { url: string; label: string; at: string }

const invisibleCharacters = /[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u2028\u2029\u202a-\u202e\u2066-\u2069\ufeff]+/g

function cap(text: string, max: number): string {
  if (text.length <= max) return text
  const cut = text.slice(0, max)
  const last = cut.charCodeAt(cut.length - 1)
  return last >= 0xd800 && last <= 0xdbff ? cut.slice(0, -1) : cut
}

export function cleanBookmarkLabel(value: unknown): string {
  if (typeof value !== 'string') return ''
  return cap(value.replace(invisibleCharacters, ' ').replace(/\s+/g, ' ').trim(), BOOKMARKS_MAX_LABEL).trimEnd()
}

/** The URL as it may be stored, or null. Only absolute http(s), no credentials. */
export function cleanBookmarkUrl(value: unknown): string | null {
  if (typeof value !== 'string') return null
  const trimmed = value.trim()
  if (!trimmed || trimmed.length > BOOKMARKS_MAX_URL) return null
  let parsed: URL
  try {
    parsed = new URL(trimmed)
  } catch {
    return null
  }
  if ((parsed.protocol !== 'http:' && parsed.protocol !== 'https:') || !parsed.hostname) return null
  parsed.username = ''
  parsed.password = ''
  return parsed.href.length > BOOKMARKS_MAX_URL ? null : parsed.href
}

const toInstant = (value: string) => {
  const time = Date.parse(value)
  return Number.isNaN(time) ? null : new Date(time).toISOString()
}

const storedBookmarkSchema = z.object({ url: z.string(), label: z.string(), at: z.string() })

export const browserBookmarkSchema = storedBookmarkSchema.transform((raw): BrowserBookmark | null => {
  const at = toInstant(raw.at)
  if (!at) return null
  const url = cleanBookmarkUrl(raw.url)
  if (!url) return null
  const label = cleanBookmarkLabel(raw.label)
  return { url, label: label || new URL(url).hostname, at }
})

const isList = (value: unknown): value is unknown[] => Array.isArray(value)
const READ_LIMIT = 100

export function readBookmarks(key: string | null): BrowserBookmark[] {
  if (!key) return []
  const stored = readClientJson(key, isList)
  if (!stored) return []
  const seen = new Set<string>()
  const bookmarks: BrowserBookmark[] = []
  for (const item of stored.slice(0, READ_LIMIT)) {
    const parsed = browserBookmarkSchema.safeParse(item)
    if (!parsed.success || !parsed.data || seen.has(parsed.data.url)) continue
    seen.add(parsed.data.url)
    bookmarks.push(parsed.data)
    if (bookmarks.length >= BOOKMARKS_MAX_ENTRIES) break
  }
  return bookmarks
}

export function toggleBookmark(
  key: string | null,
  input: { url: string; label?: string | null },
  now: Date = new Date(),
): BrowserBookmark[] {
  const current = readBookmarks(key)
  if (!key) return current
  const url = cleanBookmarkUrl(input.url)
  if (!url) return current
  const existing = current.find((bookmark) => bookmark.url === url)
  const next = existing
    ? current.filter((bookmark) => bookmark.url !== url)
    : [
        { url, label: cleanBookmarkLabel(input.label) || new URL(url).hostname, at: now.toISOString() },
        ...current,
      ].slice(0, BOOKMARKS_MAX_ENTRIES)
  writeClientJson(key, next)
  return next
}

const [revision, setRevision] = createSignal(0)

function currentKey(): string | null {
  return untrack(() => {
    const session = getSession()
    return bookmarksStorageKey(session.user?.id, session.activeOrg?.id)
  })
}

function recordingKey(): string | null {
  return isLocalRetentionAllowed() ? currentKey() : null
}

/** Saves the page if it is not bookmarked, removes it if it is. */
export function toggleBrowserBookmark(input: { url: string; label?: string | null }): void {
  const key = recordingKey()
  if (!key) return
  toggleBookmark(key, input)
  setRevision((value) => value + 1)
}

/** Forgets every bookmark of the current user and workspace. */
export function clearBrowserBookmarks(): void {
  const key = currentKey()
  if (key) removeClientValue(key)
  setRevision((value) => value + 1)
}

/** Whether the page is bookmarked. */
export function isBookmarked(url: string): boolean {
  const clean = cleanBookmarkUrl(url)
  return Boolean(clean) && readBookmarks(currentKey()).some((bookmark) => bookmark.url === clean)
}

/** The bookmarks of the signed-in person and workspace, newest first. */
export function useBrowserBookmarks(): Accessor<BrowserBookmark[]> {
  const session = getSession()
  const key = createMemo(() => bookmarksStorageKey(session.user?.id, session.activeOrg?.id))
  return createMemo(() => {
    revision()
    return readBookmarks(key())
  })
}
