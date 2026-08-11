/**
 * Server-authoritative retention policy for the SPA's own chat storage.
 *
 * # Why this exists
 *
 * The SPA keeps a full copy of conversations in `localStorage` — turns, task
 * steps, previews — so a reload paints instantly instead of waiting on the
 * network. That copy is genuinely useful, and it was also the least governed
 * copy in the system: its only Zero Data Retention gate was `temporaryThreadIds`,
 * an in-memory `Set` in `use-chat-controller.ts`, which is gone on reload and
 * absent from any other entry point. And because it lives on the user's own
 * device, it is the one copy no server-side erasure fan-out can ever reach.
 *
 * The fix is not to delete the cache — that would trade a real UX property for
 * a policy the server can enforce anyway. The fix is to stop letting the CLIENT
 * decide. This module makes `localStorage` a *derived* cache the server can
 * invalidate, never an independent store:
 *
 *   * [`applyServerRetention`] — every listing carries the org's ZDR posture
 *     (`GET /api/v1/chat/threads` → `retention.zdr`). Turning it on purges
 *     everything local, immediately.
 *   * [`isLocalRetentionAllowed`] — the write gate, read by the single funnel
 *     all local content writes go through.
 *   * [`forgetThreadLocally`] — a save answered `retained: false` means the
 *     server kept nothing, so neither may we.
 *   * [`reconcileLocalThreads`] — a thread the server no longer lists has been
 *     erased upstream. This is how a GDPR erasure finally reaches the device.
 *
 * Nothing here is a substitute for the server-side gate in
 * `domains/chat/history.rs`; it is the same policy, applied at the one boundary
 * the server cannot reach on its own.
 */

import {
  clearChatThreadHistory,
  readChatThreadHistory,
  removeChatThreadHistoryItem,
  readActiveChatThreadId,
} from './chat-thread-history'

/**
 * The last posture the server stated.
 *
 * Defaults to ALLOWED, not denied. A denied default would wipe every user's
 * sidebar on first paint, before any listing has returned — punishing the
 * common case for a posture almost no org has. The server gate is what actually
 * protects the durable copy; this one converges the moment the first listing
 * lands, and every write path below re-checks it.
 */
let retentionAllowed = true

/**
 * Threads younger than this are never reconciled away.
 *
 * A thread created seconds ago may legitimately not be in the server listing
 * yet: the SPA debounces its snapshot save by 500ms, and the listing merges the
 * BFF index with session-core. Without this window, sending the first message in
 * a new chat and immediately opening the sidebar could delete the conversation
 * being typed into.
 */
const RECONCILE_GRACE_MS = 5 * 60 * 1000

/** Whether the SPA may write conversation content to local storage. */
export function isLocalRetentionAllowed(): boolean {
  return retentionAllowed
}

/**
 * Record the server's retention verdict, purging local storage when it denies.
 *
 * Idempotent and cheap on the common path (posture unchanged, nothing to do).
 */
export function applyServerRetention(zdr: boolean | undefined): void {
  // An absent field means an older gateway that does not state a posture.
  // Treat it as "no change" rather than as permission or denial — inventing
  // either from a missing field is how a policy silently flips.
  if (zdr === undefined) return
  const allowed = !zdr
  if (allowed === retentionAllowed) return
  retentionAllowed = allowed
  if (!allowed) clearChatThreadHistory()
}

/** Drop one thread's local copy — used when a save comes back unretained. */
export function forgetThreadLocally(threadId: string): void {
  if (!threadId.trim()) return
  removeChatThreadHistoryItem(threadId)
}

/**
 * Drop local threads the server no longer knows about.
 *
 * This is the erasure path reaching the device: once the gateway and
 * session-core have both purged a conversation, it stops appearing in the
 * listing, and the local copy goes with it.
 *
 * Two guards keep this from eating live data:
 *   * the ACTIVE thread is never reconciled away, and
 *   * neither is anything updated within {@link RECONCILE_GRACE_MS}, because a
 *     just-created thread has not necessarily reached the listing yet.
 *
 * Only call this with a listing that actually SUCCEEDED. An empty array from a
 * failed request would erase the user's whole history — which is why the caller
 * in `chat-client.ts` sits after the response parses, never in a catch.
 *
 * # An empty SUCCESSFUL listing is treated as authoritative — deliberately
 *
 * A gateway running without Dragonfly (a common local-dev setup) legitimately
 * answers "no threads", and this will then drop local copies older than the
 * grace window. That is accepted rather than special-cased, because the two
 * mistakes are not equally bad: a false purge costs a *cache* the user can
 * re-fetch from session-core, while a missed purge leaves erased conversations
 * on the device indefinitely — and the total-erasure case, where every thread
 * is gone, is exactly the one an "ignore empty listings" guard would skip. The
 * active-thread and grace-window guards above already protect the conversation
 * actually in use.
 */
export function reconcileLocalThreads(serverThreadIds: readonly string[]): void {
  const known = new Set(serverThreadIds.filter((id) => id.trim()))
  const active = readActiveChatThreadId()
  const cutoff = Date.now() - RECONCILE_GRACE_MS

  for (const item of readChatThreadHistory()) {
    if (known.has(item.threadId)) continue
    if (item.threadId === active) continue
    const updatedAt = Date.parse(item.updatedAt ?? '')
    // An unparsable timestamp is treated as recent, not as ancient: guessing
    // "old" here would delete it.
    if (Number.isNaN(updatedAt) || updatedAt > cutoff) continue
    removeChatThreadHistoryItem(item.threadId)
  }
}

/** Test-only reset so one spec's posture cannot leak into the next. */
export function __resetRetentionForTests(): void {
  retentionAllowed = true
}
