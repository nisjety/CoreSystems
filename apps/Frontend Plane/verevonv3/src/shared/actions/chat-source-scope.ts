/** Source selection is a request contract, enforced by Model and Session Core.
 * A conversation's scope is fixed by its first message and survives reload.
 * This controls grounding and learning; it does not change retention policy.
 */
export type ChatSourceScope = 'workspace' | 'conversation'
export const CONVERSATION_ONLY_FEATURE = 'conversation_only'
export function normalizeChatSourceScope(value: unknown): ChatSourceScope {
  return value === 'conversation' ? 'conversation' : 'workspace'
}
