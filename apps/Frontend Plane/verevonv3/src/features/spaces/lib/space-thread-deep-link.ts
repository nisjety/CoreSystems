/**
 * `?thread_id=` for the Space room: the one place that reads and writes it.
 *
 * Reading a thread id from the URL grants nothing. The pane always resolves the
 * id through the Space transcript route, which asks Control whether the caller
 * is a current recipient of the Space, so a forged or stale link can only ever
 * land on the restricted state. The builder lives here, inside `features/spaces`,
 * because `chat-route-ownership.test.ts` keeps every `/chat?thread_id=` link
 * inside `features/chat`: a Space thread must never route into Chat.
 */

const MAX_THREAD_ID_LENGTH = 200
// eslint-disable-next-line no-control-regex
const CONTROL_CHARACTERS = /[\u0000-\u001F\u007F]/

/**
 * Normalise one router search-param value (which may be absent or repeated)
 * into a thread id, or `null`. A repeated parameter is ambiguous, so it names
 * nothing rather than picking one.
 */
export function readSpaceThreadParam(value: string | readonly string[] | null | undefined): string | null {
  if (typeof value !== 'string') return null
  const threadId = value.trim()
  if (!threadId || threadId.length > MAX_THREAD_ID_LENGTH || CONTROL_CHARACTERS.test(threadId)) return null
  return threadId
}

/** The same read, from a raw `location.search` string. */
export function readSpaceThreadFromSearch(search: string): string | null {
  return readSpaceThreadParam(new URLSearchParams(search).get('thread_id'))
}

/**
 * The room URL that opens one thread in the side pane. A thread id the reader
 * would not hand back unchanged yields the plain room link: a link the page
 * would silently ignore is worse than none. `#chat` because that is where the
 * conversation lives.
 */
export function spaceThreadHref(spaceRef: string, threadId: string | null | undefined): string {
  const room = `/spaces/${encodeURIComponent(spaceRef)}`
  const id = typeof threadId === 'string' ? readSpaceThreadParam(threadId) : null
  return id && id === threadId ? `${room}?thread_id=${encodeURIComponent(id)}#chat` : `${room}#chat`
}

/**
 * Build-time switch for the thread pane until the BFF delivers
 * `features.thread_pane` (WS01-S5). On in dev, off in production unless
 * `VITE_SPACE_THREAD_PANE_UI=true`; `VITE_SPACE_THREAD_PANE_UI=false` turns it
 * off in dev too. A rollout control only, never an authorization input.
 */
export function spaceThreadPaneEnabled(): boolean {
  const flag = import.meta.env.VITE_SPACE_THREAD_PANE_UI
  if (flag === 'true') return true
  if (flag === 'false') return false
  return import.meta.env.DEV === true
}
