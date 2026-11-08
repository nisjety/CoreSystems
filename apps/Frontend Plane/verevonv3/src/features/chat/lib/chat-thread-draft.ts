import { createEffect, createMemo, createSignal, onCleanup, untrack, type Accessor, type Setter } from 'solid-js'
import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import {
  capHomeDraft,
  chatDraftClearEpoch,
  chatDraftStorageKey,
  readHomeDraft,
  removeChatDraft,
  writeChatDraft,
} from '@/shared/session/home-draft-storage'
import { getSession } from '@/shared/session/session-store'

/** How long typing must pause before the draft is written: the Home draft's pause. */
export const CHAT_DRAFT_DEBOUNCE_MS = 400

/** How a send ends: `accepted` once the gateway has taken the turn, `failed` when it has not. */
export type ChatDraftOutcome = 'accepted' | 'failed'

export type ChatThreadDraft = {
  /** The composer's text. */
  message: Accessor<string>
  /** Writes the composer's text, from the person typing or from a suggestion. */
  setMessage: Setter<string>
  /** The draft for a chat without a thread, including the current unsaved text if shown. */
  readNewDraft: () => string
  /**
   * A send is starting from the composer, which it clears. The text stays stored until the send
   * settles: `accepted` discards it, `failed` gives it back to the composer.
   */
  beginSend: () => (outcome: ChatDraftOutcome) => void
  /**
   * The conversation takes a new id (a new chat's first send, or the id the server gives it), and its
   * draft moves with it. `from` is `null` for a chat that has no thread yet.
   */
  carry: (from: string | null, to: string) => void
  /** A new chat: keeps the draft of the thread being left, and starts with an empty composer. */
  reset: () => void
}

type Sent = { key: string | null; text: string }

// The unsent text of a chat thread's composer, kept for this tab only (sessionStorage) so that a reload,
// a trip to another thread or a failed send does not lose it. Text only: attachments are never stored,
// and nothing here sends anything.
//
// Each thread has its own draft. A chat that has no thread yet keeps its draft under 'new'; when the
// conversation takes an id the draft moves with it (`carry`), so the change is not read as a switch to
// another thread, which would show that thread's draft instead.
//
// Writes follow the Home draft's gate. While the next turn is zero retention (Temporary chat, or a
// temporary thread) or the workspace denies local copies, nothing is written, and turning either on
// removes what was stored. Text that has been in a zero-retention composer is not stored again until the
// composer is empty.
//
// A send clears the composer, but the text it sends stays stored until the send settles, so that a failed
// send can give it back. It is discarded only when the turn is accepted.

export function useChatThreadDraft(options: {
  /** The thread on screen, or `null` for a chat that has none yet. */
  threadId: Accessor<string | null | undefined>
  /** Whether the next turn is zero retention: Temporary chat, or a temporary thread. */
  zeroRetention: Accessor<boolean>
}): ChatThreadDraft {
  const session = getSession()
  const keyFor = (threadId: string | null | undefined) =>
    chatDraftStorageKey(session.user?.id, session.activeOrg?.id, threadId ?? 'new')
  const key = createMemo(() => keyFor(options.threadId()))

  // The key the composer's text belongs to, and what storage holds under it. The first paint already
  // shows a known thread's draft. A workspace that denies local copies has nothing to restore.
  let bound = untrack(key)
  const restore = (from: string | null) => (isLocalRetentionAllowed() ? readHomeDraft(from) : '')
  let saved = restore(bound)
  let retention = untrack(options.zeroRetention)
  // Text that has been in a zero-retention composer since the composer was last emptied.
  let tainted = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let pending: { text: string; epoch: number } | undefined
  // The send in flight, and the key whose draft holds its text.
  let inFlight: Sent | undefined
  const [message, setMessage] = createSignal(saved)
  const readNewDraft = () => {
    const newKey = keyFor(null)
    return bound === newKey ? untrack(message) : restore(newKey)
  }

  const zeroRetention = () => retention || !isLocalRetentionAllowed()
  const writable = () => bound !== null && !zeroRetention() && !tainted
  const isInFlight = () => inFlight !== undefined && inFlight.key === bound

  const cancel = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    pending = undefined
  }
  const forget = () => {
    cancel()
    removeChatDraft(bound)
    saved = ''
  }
  const save = () => {
    const write = pending
    cancel()
    // A sign-out clears every draft; a write scheduled before it must not bring one back.
    if (!write || write.epoch !== chatDraftClearEpoch() || !writable()) return
    const stored = writeChatDraft(bound, write.text)
    if (stored !== null) saved = stored
  }
  const schedule = (text: string) => {
    cancel()
    pending = { text, epoch: chatDraftClearEpoch() }
    timer = setTimeout(save, CHAT_DRAFT_DEBOUNCE_MS)
  }
  // Brings storage in line with the composer, if the gate is open.
  const reconcile = () => {
    const text = untrack(message)
    if (!writable() || !text.trim()) return
    if (text === saved) cancel()
    else schedule(text)
  }

  // Moves the composer to the thread the key now names. A retention change can arrive in the same pass as
  // a thread switch, so the switch is made first: the retention change must not act on the thread left.
  const sync = () => {
    const next = untrack(key)
    if (next !== bound) rebind(next)
  }

  const onText = (text: string) => {
    if (!text.trim()) {
      // A send clears the composer on purpose; the text it is sending stays stored until it settles.
      if (isInFlight()) {
        save()
        return
      }
      tainted = false
      forget()
    } else if (zeroRetention()) {
      tainted = true
      forget()
    } else {
      reconcile()
    }
  }

  // Turning Temporary chat on removes what was stored for this thread. The text on screen is not stored
  // again until the composer is emptied, so turning it back off cannot smuggle it into storage.
  const setRetention = (next: boolean) => {
    sync()
    if (next === retention) return
    retention = next
    if (next) {
      tainted = untrack(message).trim() !== ''
      forget()
    }
  }

  // A workspace that denies local copies has already had its storage cleared; what is on screen is not
  // stored again until it is emptied, as with Temporary chat.
  const onWorkspaceRetention = (allowed: boolean) => {
    if (allowed) return
    sync()
    tainted = untrack(message).trim() !== ''
    forget()
  }

  const rebind = (next: string | null) => {
    if (next === bound) return
    const previous = bound
    // The thread being left keeps the text typed in it, before the composer shows another thread's draft.
    if (next !== null) save()
    cancel()
    const carried = untrack(message)
    bound = next
    saved = restore(next)
    if (next === null) return
    if (previous === null && carried.trim()) {
      // Typed before the identity was known: the person's text wins over an older draft.
      if (zeroRetention()) tainted = true
      else reconcile()
    } else if (carried !== saved) {
      setMessage(saved)
    }
  }

  const beginSend = () => {
    save()
    const sent: Sent = { key: bound, text: untrack(message) }
    inFlight = sent
    return (outcome: ChatDraftOutcome) => {
      if (inFlight !== sent) return
      inFlight = undefined
      if (outcome === 'accepted') accepted(sent)
      else failed(sent)
    }
  }

  // The turn is on the thread now: its draft goes, unless the person has typed a newer one since.
  const accepted = (sent: Sent) => {
    const sameThread = sent.key === bound
    if (sameThread && !untrack(message).trim()) {
      forget()
      tainted = false
    } else if (readHomeDraft(sent.key) === capHomeDraft(sent.text)) {
      removeChatDraft(sent.key)
      if (sameThread) saved = ''
    }
  }

  // The send did not reach the gateway. Its text goes back to the composer if that composer still belongs to
  // the thread that sent it and the person has not started a new draft; otherwise the text stays in the
  // draft of its own thread.
  const failed = (sent: Sent) => {
    if (sent.key !== bound || untrack(message).trim()) return
    setMessage(sent.text)
  }

  const carry = (from: string | null, to: string) => {
    const next = keyFor(to)
    if (!next || next === bound || bound !== keyFor(from)) return
    save()
    const text = readHomeDraft(bound)
    removeChatDraft(bound)
    if (inFlight?.key === bound) inFlight.key = next
    bound = next
    saved = text ? (writeChatDraft(next, text) ?? '') : ''
  }

  const reset = () => {
    const next = keyFor(null)
    if (next !== bound) {
      save()
      cancel()
      bound = next
    }
    removeChatDraft(bound)
    saved = ''
    tainted = false
    setMessage('')
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

  return { message, setMessage, readNewDraft, beginSend, carry, reset }
}
