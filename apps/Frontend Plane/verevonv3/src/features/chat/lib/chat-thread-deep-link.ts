/**
 * Reads a durable-thread deep link without granting access to it. The Chat
 * controller always resolves the resulting ID through the owner-bound BFF
 * transcript route; this helper only chooses the requested local view.
 */
export function readThreadDeepLink(search: string): string | null {
  const params = new URLSearchParams(search)
  const snake = params.get('thread_id')?.trim() ?? ''
  const camel = params.get('threadId')?.trim() ?? ''
  if (snake && camel && snake !== camel) return null
  const threadId = snake || camel
  if (!threadId || threadId.length > 200 || /[\u0000-\u001F\u007F]/.test(threadId)) return null
  return threadId
}

/**
 * The inverse of `readThreadDeepLink`: the Chat URL that opens one durable
 * thread. A surface that lists threads uses it as the `href` of its row, so
 * Ctrl/Cmd-click, middle-click, "open in new tab" and "copy link" name the
 * thread instead of landing on a plain `/chat` that follows the shared
 * active-thread pointer (which may be a different thread by then).
 *
 * An ID the reader would not hand back unchanged (blank, padded, over-long,
 * control characters, a lone surrogate) yields plain `/chat`: a link Chat
 * would silently ignore is worse than none. The builder lives here, next to
 * its reader, because `chat-route-ownership.test.ts` keeps every
 * `/chat?thread_id=` link inside features/chat.
 */
export function chatThreadHref(threadId: string): string {
  const query = `?${new URLSearchParams({ thread_id: threadId }).toString()}`
  return readThreadDeepLink(query) === threadId ? `/chat${query}` : '/chat'
}
