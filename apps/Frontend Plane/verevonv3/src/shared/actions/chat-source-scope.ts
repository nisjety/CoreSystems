/** Source selection is a request contract, enforced by Model and Session Core.
 * A conversation's scope is fixed by its first message and survives reload.
 * This controls grounding and learning; it does not change retention policy.
 */
export type ChatSourceScope = 'workspace' | 'conversation'
export const CONVERSATION_ONLY_FEATURE = 'conversation_only'
export function normalizeChatSourceScope(value: unknown): ChatSourceScope {
  return value === 'conversation' ? 'conversation' : 'workspace'
}

/** Per-turn retrieval-source filter (Perplexity-Focus style), orthogonal to
 * `ChatSourceScope`. `ChatSourceScope` is a privacy/retention commitment
 * that is thread-locked; `RetrievalScope` only selects which sources a
 * single turn may draw on and can change turn to turn. It only has meaning
 * while the thread's `ChatSourceScope` is `'workspace'` — under
 * `'conversation'` it is always forced to `'auto'` and omitted on the wire.
 */
export type RetrievalScope = 'auto' | 'knowledge_only' | 'web_only'
export const DEFAULT_RETRIEVAL_SCOPE: RetrievalScope = 'auto'
export function normalizeRetrievalScope(value: unknown): RetrievalScope {
  return value === 'knowledge_only' || value === 'web_only' ? value : 'auto'
}
