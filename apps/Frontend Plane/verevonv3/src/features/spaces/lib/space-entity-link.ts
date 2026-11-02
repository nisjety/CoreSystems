/**
 * One grammar for pointing at a thing inside a Space (WS13-S4a): a message, a thread, and (reserved
 * for WS09) a run. Every consumer that needs a link to one of them (Activity, search hits, the
 * palette, link chips, notification refs) calls {@link spaceEntityHref} instead of assembling a
 * string, and every place that reads a location calls {@link parseSpaceEntityLocation}.
 *
 * A link is an address, never a grant. Opening one always goes through the transcript route and its
 * Control decision, so a forged or stale link can only land on a named "not available" state. The
 * parser fails closed: anything it cannot read exactly (blank, repeated, too long, control
 * characters, `thread_id` and `threadId` that disagree, ids outside `[A-Za-z0-9_-]{1,64}`) is `null`
 * and the page ignores the parameter.
 *
 * Canonical forms:
 *   thread   /spaces/{ref}?thread_id={tid}#chat
 *   message  /spaces/{ref}?thread_id={tid}&m={mid}#chat   (thread known)
 *   message  /spaces/{ref}/m/{mid}                        (only the id; the S4b route resolves it)
 *   run      /spaces/{ref}?thread_id={tid}&run_id={rid}#chat   (grammar only; WS09 opens it)
 */

import { readSpaceThreadParam, spaceThreadHref } from './space-thread-deep-link'
import { spaceRoomFeatureWithDevOverride } from './space-room-features'

export type SpaceEntityRef =
  | { readonly kind: 'message'; readonly spaceRef: string; readonly messageId: string; readonly threadId?: string }
  | { readonly kind: 'thread'; readonly spaceRef: string; readonly threadId: string }
  | { readonly kind: 'run'; readonly spaceRef: string; readonly runId: string; readonly threadId?: string }

const MAX_REF_LENGTH = 200
const ENTITY_ID = /^[A-Za-z0-9_-]{1,64}$/
// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTERS = /[\u0000-\u001F\u007F]/

/** A message or run id, or `null` when it is not exactly one id. */
export function readSpaceEntityId(value: string | readonly string[] | null | undefined): string | null {
  if (typeof value !== 'string') return null
  const id = value.trim()
  return ENTITY_ID.test(id) ? id : null
}

/** The `m` parameter: a message to bring into view inside the open thread. */
export function readSpaceMessageParam(value: string | readonly string[] | null | undefined): string | null {
  return readSpaceEntityId(value)
}

function readSpaceRef(value: string): string | null {
  let ref: string
  try {
    ref = decodeURIComponent(value).trim()
  } catch {
    return null
  }
  if (!ref || ref.length > MAX_REF_LENGTH || CONTROL_CHARACTERS.test(ref) || ref.includes('/')) return null
  return ref
}

/**
 * Read one location as an entity reference. `hash` is accepted so callers pass the whole location,
 * but it names nothing (`#chat` is the tab, not an entity).
 */
export function parseSpaceEntityLocation(pathname: string, search: string, _hash?: string): SpaceEntityRef | null {
  const parts = pathname.replace(/\/+$/, '').split('/')
  if (parts[0] !== '' || parts[1] !== 'spaces' || !parts[2]) return null
  const spaceRef = readSpaceRef(parts[2])
  if (!spaceRef) return null

  // /spaces/{ref}/m/{mid}
  if (parts.length === 5 && parts[3] === 'm') {
    let raw: string
    try {
      raw = decodeURIComponent(parts[4] ?? '')
    } catch {
      return null
    }
    const messageId = readSpaceEntityId(raw)
    return messageId ? { kind: 'message', spaceRef, messageId } : null
  }
  if (parts.length !== 3) return null

  // /spaces/{ref}?thread_id=...
  const params = new URLSearchParams(search)
  const threadValues = [...params.getAll('thread_id'), ...params.getAll('threadId')]
  if (threadValues.length === 0) return null
  const threadIds = threadValues.map((value) => readSpaceThreadParam(value))
  const threadId = threadIds[0]
  // A repeated, unreadable or conflicting thread parameter is ambiguous: name nothing.
  if (!threadId || threadIds.some((candidate) => candidate !== threadId)) return null

  const messages = params.getAll('m')
  const runs = params.getAll('run_id')
  if (messages.length > 1 || runs.length > 1) return null
  if (messages.length === 1) {
    const messageId = readSpaceEntityId(messages[0])
    // A malformed `m` still opens the thread: the thread link is valid on its own.
    if (messageId && runs.length === 0) return { kind: 'message', spaceRef, messageId, threadId }
  }
  if (runs.length === 1) {
    const runId = readSpaceEntityId(runs[0])
    if (runId && messages.length === 0) return { kind: 'run', spaceRef, runId, threadId }
  }
  return { kind: 'thread', spaceRef, threadId }
}

/**
 * The URL for one entity. An id the parser would not read back unchanged yields the plain room link
 * (a link the page would silently ignore is worse than none).
 */
export function spaceEntityHref(ref: SpaceEntityRef): string {
  const room = `/spaces/${encodeURIComponent(ref.spaceRef)}`
  switch (ref.kind) {
    case 'thread':
      return spaceThreadHref(ref.spaceRef, ref.threadId)
    case 'message': {
      const messageId = readSpaceEntityId(ref.messageId)
      if (!messageId || messageId !== ref.messageId) return spaceThreadHref(ref.spaceRef, ref.threadId)
      const threadId = ref.threadId === undefined ? null : readSpaceThreadParam(ref.threadId)
      if (threadId && threadId === ref.threadId) {
        return `${room}?thread_id=${encodeURIComponent(threadId)}&m=${encodeURIComponent(messageId)}#chat`
      }
      return ref.threadId === undefined ? `${room}/m/${encodeURIComponent(messageId)}` : `${room}#chat`
    }
    case 'run': {
      const runId = readSpaceEntityId(ref.runId)
      const threadId = ref.threadId === undefined ? null : readSpaceThreadParam(ref.threadId)
      if (runId && runId === ref.runId && threadId && threadId === ref.threadId) {
        return `${room}?thread_id=${encodeURIComponent(threadId)}&run_id=${encodeURIComponent(runId)}#chat`
      }
      return spaceThreadHref(ref.spaceRef, ref.threadId)
    }
  }
}

/**
 * Whether `?thread_id=&m=` scrolls to and highlights the message (`features.entry_links` from the
 * BFF's flags). On by default in a dev build; `VITE_SPACE_ENTRY_LINKS_UI=true|false` overrides it
 * there. A rollout control, never an authorization input: opening is re-checked by Control.
 */
export function spaceEntryLinksEnabled(): boolean {
  const override = (import.meta.env as Record<string, string | undefined>).VITE_SPACE_ENTRY_LINKS_UI
  if (import.meta.env.DEV === true && override !== 'false') return true
  return spaceRoomFeatureWithDevOverride('entry_links', override)
}
