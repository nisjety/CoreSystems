/**
 * Server-derived effect evidence for a chat turn.
 *
 * The value is never inferred from model prose. It is only populated after a
 * durable proof bundle has been read, so an absent value means that the client
 * has not established evidence yet rather than that the turn was harmless.
 */
export type ChatEffectClass =
  | 'read_only'
  | 'proposed_effect'
  | 'effectful'
  | 'external_receipt'
  | 'unknown'

export function isEffectfulChatTurn(effectClass: ChatEffectClass | undefined): boolean {
  return effectClass === 'effectful' || effectClass === 'external_receipt'
}
