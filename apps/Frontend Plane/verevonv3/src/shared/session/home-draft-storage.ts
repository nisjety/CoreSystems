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

export function homeDraftStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  return user && org ? `${KEY_PREFIX}:${encodeURIComponent(org)}:${encodeURIComponent(user)}` : null
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
