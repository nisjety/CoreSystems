import type { RetrievalScope } from '@/shared/actions/chat-source-scope'

/**
 * Connected apps for one chat turn (WS23 S6). The person names the connected
 * accounts a turn may read, and that selection is consent for that turn only.
 * The gateway enforces the same bounds (model-gateway `connector_scope`: at most
 * 8 distinct ids of 1-128 characters from `[A-Za-z0-9_-]`), so nothing here
 * sends an id the gateway would refuse.
 */
export const CONNECTOR_SCOPE_LIMIT = 8

const CONNECTOR_SCOPE_ID = /^[A-Za-z0-9_-]{1,128}$/

export type ConnectorScopeSelection = readonly string[]

/** Trims, drops ids the gateway would refuse, removes repeats and keeps at most `CONNECTOR_SCOPE_LIMIT`. */
export function normalizeConnectorScope(value: unknown): string[] {
  if (!Array.isArray(value)) return []
  const ids: string[] = []
  for (const item of value) {
    if (typeof item !== 'string') continue
    const id = item.trim()
    if (!CONNECTOR_SCOPE_ID.test(id) || ids.includes(id)) continue
    ids.push(id)
    if (ids.length === CONNECTOR_SCOPE_LIMIT) break
  }
  return ids
}

/**
 * The selection with one connection toggled. A ninth connection is not added:
 * the chip row disables that chip, so this is only the guard behind it.
 */
export function toggleConnectorScope(selection: ConnectorScopeSelection, connectionId: string): string[] {
  const id = connectionId.trim()
  const current = normalizeConnectorScope(selection)
  if (current.includes(id)) return current.filter((item) => item !== id)
  return normalizeConnectorScope([...current, id])
}

/**
 * Why connected apps cannot take part in a turn. The reasons are closed: the
 * composer words each one, and the gateway refuses the same turns (zero
 * retention, Space rooms, filtered sources) before any read happens.
 */
export type ConnectorScopeBlock =
  | 'zero_retention'
  | 'space'
  | 'conversation_only'
  | 'subscription'
  | 'web_only'
  | 'knowledge_only'

/** The first reason that applies, in the order a person reads the controls; `null` when connected apps may take part. */
export function connectorScopeBlock(turn: {
  zeroRetention: boolean
  spaceScoped: boolean
  conversationOnly: boolean
  subscriptionBacked: boolean
  retrievalScope: RetrievalScope
}): ConnectorScopeBlock | null {
  if (turn.zeroRetention) return 'zero_retention'
  if (turn.spaceScoped) return 'space'
  if (turn.conversationOnly) return 'conversation_only'
  if (turn.subscriptionBacked) return 'subscription'
  if (turn.retrievalScope === 'web_only') return 'web_only'
  if (turn.retrievalScope === 'knowledge_only') return 'knowledge_only'
  return null
}
