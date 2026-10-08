import { createEffect, createMemo, createSignal, onCleanup, untrack, type Accessor, type Setter } from 'solid-js'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import {
  homeDraftClearEpoch,
  homeDraftStorageKey,
  readHomeDraft,
  removeHomeDraft,
  writeHomeDraft,
} from '@/shared/session/home-draft-storage'
import { getSession } from '@/shared/session/session-store'

/** How long typing must pause before the draft is written. */
export const HOME_DRAFT_DEBOUNCE_MS = 400

// The unsent Home chat draft, kept for this tab only (sessionStorage) so a
// reload or a trip to another page does not lose it. Text only: attachments
// are never stored. It is presentation state: it never sends anything.
//
// It is written only when the composer has said the next turn is an ordinary
// one. Temporary chat is zero data retention, so a draft typed there must not
// reach storage, and turning it on removes what was stored. Until the composer
// reports, and whenever it is gone, nothing is written: the report is the only
// way to open the gate. Once text has been in a zero-retention composer it is
// not stored again until the composer has been emptied, so switching Temporary
// chat back off cannot smuggle it into storage.
//
// A workspace that is zero-data-retention as a whole is the same case, and it is
// the one that can occur on Home, which has no Temporary chat switch: the
// server's verdict (chat-retention) closes the gate whatever the composer says,
// removes what was stored the moment it arrives, and taints what is on screen.
//
// The draft belongs to the workspace and user it was typed under. Nothing is
// read or written until both are known; a change of either saves what was
// typed for the identity it came from and shows the new identity's own draft.

export type HomeDraft = [
  message: Accessor<string>,
  setMessage: Setter<string>,
  /** The message was sent or handed off: forget the stored draft now. */
  discard: () => void,
]

export function useHomeDraft(options: {
  /**
   * Whether the next turn is zero-retention (Temporary chat): `true` or
   * `false` as the composer reports it, `undefined` while it is not known.
   */
  zeroRetention: Accessor<boolean | undefined>
}): HomeDraft {
  const session = getSession()
  const key = createMemo(() => homeDraftStorageKey(session.user?.id, session.activeOrg?.id))

  // The identity the draft is bound to and what storage holds under it. The
  // first paint already shows a known identity's draft.
  let bound = untrack(key)
  // A workspace that denies local copies has nothing to restore.
  const restore = (from: string | null) => (isLocalRetentionAllowed() ? readHomeDraft(from) : '')
  let saved = restore(bound)
  let retention = untrack(options.zeroRetention)
  // Text that has been in a zero-retention composer since it was last empty.
  let tainted = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let pending: { text: string; epoch: number } | undefined
  const [message, setMessage] = createSignal(saved)

  // Zero retention for the next turn (Temporary chat) or for the whole workspace.
  const zeroRetention = () => retention === true || !isLocalRetentionAllowed()
  const writable = () => bound !== null && retention === false && !tainted && isLocalRetentionAllowed()
  const cancel = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    pending = undefined
  }
  const forget = () => {
    cancel()
    removeHomeDraft(bound)
    saved = ''
  }
  const save = () => {
    const write = pending
    cancel()
    // A sign-out clears every draft; a write scheduled before it must not bring one back.
    if (!write || write.epoch !== homeDraftClearEpoch() || !writable()) return
    const stored = writeHomeDraft(bound, write.text)
    if (stored !== null) saved = stored
  }
  const schedule = (text: string) => {
    cancel()
    pending = { text, epoch: homeDraftClearEpoch() }
    timer = setTimeout(save, HOME_DRAFT_DEBOUNCE_MS)
  }
  // Brings storage in line with the composer, if the gate is open.
  const reconcile = () => {
    const text = untrack(message)
    if (!writable() || !text.trim()) return
    if (text === saved) cancel()
    else schedule(text)
  }

  const onText = (text: string) => {
    if (!text.trim()) {
      tainted = false
      forget()
    } else if (zeroRetention()) {
      tainted = true
      forget()
    } else {
      reconcile()
    }
  }

  // The workspace's verdict changed. A denial has already emptied storage; what is
  // on screen may not be stored until it has been emptied, like Temporary chat.
  const onWorkspaceRetention = (allowed: boolean) => {
    if (!allowed) {
      tainted = untrack(message).trim() !== ''
      forget()
    } else {
      reconcile()
    }
  }

  const setRetention = (next: boolean | undefined) => {
    if (next === retention) return
    if (next === true) {
      tainted = untrack(message).trim() !== ''
      forget()
    } else if (next === undefined) {
      // The composer is going away after an ordinary turn: keep what was typed.
      save()
    }
    retention = next
    if (next === false) reconcile()
  }

  const rebind = (next: string | null) => {
    if (next === bound) return
    const previous = bound
    // Text typed under the previous identity is saved for it before the switch.
    // On sign-out nothing is written: clearSession has just emptied the drafts.
    if (next !== null) save()
    cancel()
    const carried = untrack(message)
    const carriedIsSaved = previous !== null && carried === saved
    bound = next
    saved = restore(next)
    if (next === null) return
    if (!carried.trim() || carriedIsSaved) {
      // Nothing typed, or text that stays with the identity it was typed under.
      if (carried !== saved) setMessage(saved)
    } else if (zeroRetention()) {
      tainted = true
    } else {
      // Typed while the identity was unknown: the person's text wins over an older draft.
      reconcile()
    }
  }

  createEffect(() => key(), rebind)
  createEffect(() => options.zeroRetention(), setRetention)
  createEffect(() => message(), onText, { defer: true })
  onCleanup(subscribeLocalRetention(onWorkspaceRetention))

  const onHidden = () => {
    if (document.visibilityState === 'hidden') save()
  }
  document.addEventListener('visibilitychange', onHidden)
  window.addEventListener('pagehide', save)
  onCleanup(() => {
    document.removeEventListener('visibilitychange', onHidden)
    window.removeEventListener('pagehide', save)
    save()
  })

  const discard = () => {
    tainted = false
    forget()
  }
  return [message, setMessage, discard]
}
