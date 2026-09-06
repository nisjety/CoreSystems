/**
 * Which messages the user pinned into context, per thread.
 *
 * A pin means one thing mechanically: model-gateway re-expresses that message
 * as leading `system` context, where neither history shedder reaches it
 * (`compaction::hoist_pinned_messages`). Both `plan_head_summary` and
 * `drop_oldest_group` start at the first non-system message, so a pinned turn
 * survives a long thread being summarised and a provider rejecting the prompt
 * for length.
 *
 * What travels to the server is only the ID LIST. The client never sends the
 * pinned text: the server resolves each id against the durable thread, so the
 * browser can select an earlier message but cannot invent one. An id that
 * matches nothing is ignored, which is also what happens to a turn that
 * session-core has not persisted yet — the pin starts working once it has.
 *
 * Stored locally rather than server-side because a pin is a reading choice,
 * like the sidebar's collapsed state: it steers the next turn the person sends
 * from this browser, and there is no durable per-user pin contract to write it
 * to. That is a deliberate limit — see the note on `PINNED_MESSAGES_KEY`.
 */

/**
 * One key for every thread's pins.
 *
 * Versioned (`.v1`) like the other chat keys so a shape change can be dropped
 * rather than mis-parsed. Not server-owned: unlike THREAD pins
 * (`ChatThreadSession.pinned`, written through the gateway), a message pin does
 * not follow the user to another device. Making it durable needs a per-user
 * pin contract in session-core; until that exists, promising cross-device pins
 * would be a lie the storage cannot keep.
 */
const PINNED_MESSAGES_KEY = 'verevon.chat.pinnedMessages.v1'

/**
 * Mirrors `compaction::MAX_PINNED_MESSAGES` in model-gateway.
 *
 * Enforced here as well as there so the UI refuses the sixth pin instead of
 * accepting it and letting the server silently ignore it — a control that
 * appears to work and does nothing is worse than a disabled one. The server
 * cap stays authoritative; this one exists to keep the UI honest about it.
 */
export const MAX_PINNED_MESSAGES = 5

type PinnedStore = Record<string, string[]>

function readStore(): PinnedStore {
  try {
    const raw = window.localStorage.getItem(PINNED_MESSAGES_KEY)
    if (!raw) return {}
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    const store: PinnedStore = {}
    for (const [threadId, ids] of Object.entries(parsed as Record<string, unknown>)) {
      if (!Array.isArray(ids)) continue
      const clean = ids.filter((id): id is string => typeof id === 'string' && id.trim() !== '')
      if (clean.length > 0) store[threadId] = clean.slice(0, MAX_PINNED_MESSAGES)
    }
    return store
  } catch {
    // A private window, cleared site data, or a hand-edited value. Pins are a
    // convenience; failing to read them must never break the composer.
    return {}
  }
}

function writeStore(store: PinnedStore): void {
  try {
    const entries = Object.entries(store).filter(([, ids]) => ids.length > 0)
    if (entries.length === 0) {
      window.localStorage.removeItem(PINNED_MESSAGES_KEY)
      return
    }
    window.localStorage.setItem(PINNED_MESSAGES_KEY, JSON.stringify(Object.fromEntries(entries)))
  } catch {
    // Storage full or blocked. The pin is lost, which the UI will show on the
    // next read; silently pretending it stuck would be worse.
  }
}

/** Pinned message ids for one thread, in the order the user pinned them. */
export function readPinnedMessages(threadId: string): string[] {
  if (!threadId.trim()) return []
  return readStore()[threadId] ?? []
}

/**
 * Toggle one message's pin, returning the thread's new list.
 *
 * Order is preserved on purpose: the server keeps the caller's order when the
 * cap bites, so the earliest choices are the ones that survive. Pinning past
 * the cap is refused rather than silently rotating the oldest out — a pin
 * disappearing because you added another is not something a user can predict.
 */
export function togglePinnedMessage(threadId: string, messageId: string): string[] {
  if (!threadId.trim() || !messageId.trim()) return readPinnedMessages(threadId)
  const store = readStore()
  const current = store[threadId] ?? []
  const next = current.includes(messageId)
    ? current.filter((id) => id !== messageId)
    : current.length >= MAX_PINNED_MESSAGES
      ? current
      : [...current, messageId]
  if (next.length === 0) delete store[threadId]
  else store[threadId] = next
  writeStore(store)
  return next
}

/** True when this thread has no room for another pin. */
export function pinnedMessagesFull(pinned: readonly string[]): boolean {
  return pinned.length >= MAX_PINNED_MESSAGES
}

/** Forget a thread's pins — used when a thread is deleted. */
export function clearPinnedMessages(threadId: string): void {
  if (!threadId.trim()) return
  const store = readStore()
  if (!(threadId in store)) return
  delete store[threadId]
  writeStore(store)
}
