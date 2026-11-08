import { z } from 'zod'
import {
  readSessionValue,
  removeSessionValue,
  removeSessionValuesWithPrefix,
  writeSessionValue,
} from './client-storage'

// The unsent Home chat draft: text only, never attachments. It lives in
// sessionStorage and nowhere else, so it ends with the tab, and it is keyed by
// workspace and user like the chat organizer. Sign-out removes every draft in
// the tab (see clearSession), as it does the support-chat thread bindings.
const KEY_PREFIX = 'verevon.home-draft.v1'

/** Long enough for any real message, small enough to never crowd out the pending chat launch. */
export const HOME_DRAFT_MAX_CHARS = 20_000

const storedDraftSchema = z.object({ text: z.string() })

/**
 * The key a draft lives under. `scope` (a Space room draft: one per room and thread) appends a
 * further segment; without one the key is exactly the Home key, so Home's drafts are untouched and
 * {@link clearHomeDrafts}'s prefix purge still clears every scoped draft on sign-out.
 */
export function homeDraftStorageKey(
  userId: string | undefined,
  orgId: string | undefined,
  scope?: string,
): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  if (!user || !org) return null
  const base = `${KEY_PREFIX}:${encodeURIComponent(org)}:${encodeURIComponent(user)}`
  if (scope === undefined) return base
  const part = scope.trim()
  return part ? `${base}:${encodeURIComponent(part)}` : null
}

/** Cuts to the cap without leaving half of a surrogate pair at the end. */
export function capHomeDraft(text: string): string {
  if (text.length <= HOME_DRAFT_MAX_CHARS) return text
  const cut = text.slice(0, HOME_DRAFT_MAX_CHARS)
  const last = cut.charCodeAt(cut.length - 1)
  return last >= 0xd800 && last <= 0xdbff ? cut.slice(0, -1) : cut
}

/** The stored text, or '' when nothing valid is stored. */
export function readHomeDraft(key: string | null): string {
  if (!key) return ''
  const raw = readSessionValue(key)
  if (!raw) return ''
  try {
    const parsed = storedDraftSchema.safeParse(JSON.parse(raw))
    return parsed.success ? capHomeDraft(parsed.data.text) : ''
  } catch {
    return ''
  }
}

/**
 * Stores the draft under `key` and returns the text that is now stored, or
 * null when nothing was stored. Blank text removes the entry instead, so an
 * emptied composer leaves nothing behind.
 */
export function writeHomeDraft(key: string | null, text: string): string | null {
  if (!key) return null
  if (!text.trim()) {
    removeSessionValue(key)
    return ''
  }
  const capped = capHomeDraft(text)
  return writeSessionValue(key, JSON.stringify({ text: capped })) ? capped : null
}

export function removeHomeDraft(key: string | null): void {
  if (key) removeSessionValue(key)
}

let clearEpoch = 0

/** Bumped by every {@link clearHomeDrafts}, so a write scheduled before a sign-out can tell it was overtaken. */
export function homeDraftClearEpoch(): number {
  return clearEpoch
}

/** Removes every account's draft from this tab. */
export function clearHomeDrafts(): void {
  clearEpoch += 1
  removeSessionValuesWithPrefix(`${KEY_PREFIX}:`)
}

// The unsent text of each chat thread, kept for this tab in the same way as the Home draft. A thread's
// draft lives under a key of its own ('new' stands for a chat with no thread yet). An index keeps the
// drafts of the most recently written threads, so one long session cannot grow the tab's storage without
// limit.
const CHAT_KEY_PREFIX = 'verevon.chat-draft.v1'
const CHAT_INDEX_KEY = `${CHAT_KEY_PREFIX}:index`

/** How many thread drafts a tab keeps; writing one past this removes the least recently written. */
export const CHAT_DRAFT_MAX_THREADS = 30

/** Where the draft of one chat thread lives; null until the user, the workspace and the thread are known. */
export function chatDraftStorageKey(userId: string | undefined, orgId: string | undefined, threadId: string): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  const thread = threadId.trim()
  if (!user || !org || !thread) return null
  return `${CHAT_KEY_PREFIX}:${encodeURIComponent(org)}:${encodeURIComponent(user)}:${encodeURIComponent(thread)}`
}

function readChatIndex(): string[] {
  const raw = readSessionValue(CHAT_INDEX_KEY)
  if (!raw) return []
  try {
    const parsed: unknown = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed.filter((key): key is string => typeof key === 'string') : []
  } catch {
    return []
  }
}

/**
 * Stores a thread's draft and returns the text that is now stored, or null when nothing was stored.
 * Blank text removes the entry, as for the Home draft. A write makes its draft the most recent one.
 */
export function writeChatDraft(key: string | null, text: string): string | null {
  if (!key) return null
  if (!text.trim()) {
    removeChatDraft(key)
    return ''
  }
  const capped = capHomeDraft(text)
  if (!writeSessionValue(key, JSON.stringify({ text: capped }))) return null
  const keys = readChatIndex().filter((entry) => entry !== key)
  keys.push(key)
  for (const stale of keys.splice(0, Math.max(0, keys.length - CHAT_DRAFT_MAX_THREADS))) removeSessionValue(stale)
  writeSessionValue(CHAT_INDEX_KEY, JSON.stringify(keys))
  return capped
}

export function removeChatDraft(key: string | null): void {
  if (!key) return
  removeSessionValue(key)
  const keys = readChatIndex()
  if (keys.includes(key)) writeSessionValue(CHAT_INDEX_KEY, JSON.stringify(keys.filter((entry) => entry !== key)))
}

let chatClearEpoch = 0

/** Bumped by every {@link clearChatDrafts}, so a chat write scheduled before a sign-out can tell it was overtaken. */
export function chatDraftClearEpoch(): number {
  return chatClearEpoch
}

/** Removes every chat thread draft from this tab. The Home draft has its own prefix and is left alone. */
export function clearChatDrafts(): void {
  chatClearEpoch += 1
  removeSessionValuesWithPrefix(`${CHAT_KEY_PREFIX}:`)
}
