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
