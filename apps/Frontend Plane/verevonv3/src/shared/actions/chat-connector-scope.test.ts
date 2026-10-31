import { describe, expect, it } from 'vitest'
import {
  CONNECTOR_SCOPE_LIMIT,
  connectorScopeBlock,
  normalizeConnectorScope,
  toggleConnectorScope,
} from '@/shared/actions/chat-connector-scope'

const openTurn = {
  zeroRetention: false,
  spaceScoped: false,
  conversationOnly: false,
  subscriptionBacked: false,
  retrievalScope: 'auto' as const,
}

describe('normalizeConnectorScope', () => {
  it('keeps distinct, well-formed ids in the order they were picked', () => {
    expect(normalizeConnectorScope(['conn-ms-1', 'conn_g-2', 'conn-ms-1'])).toEqual(['conn-ms-1', 'conn_g-2'])
  })

  it('trims whitespace and drops ids the gateway would refuse', () => {
    expect(normalizeConnectorScope(['  conn-1  ', '', 'has space', 'semi;colon', 'x'.repeat(129), 42, null])).toEqual([
      'conn-1',
    ])
    expect(normalizeConnectorScope(['x'.repeat(128)])).toEqual(['x'.repeat(128)])
  })

  it('keeps at most the gateway limit of distinct connections', () => {
    const ids = Array.from({ length: CONNECTOR_SCOPE_LIMIT + 2 }, (_, index) => `conn-${index}`)
    expect(normalizeConnectorScope(ids)).toEqual(ids.slice(0, CONNECTOR_SCOPE_LIMIT))
  })

  it('reads anything that is not a list as an empty selection', () => {
    expect(normalizeConnectorScope(undefined)).toEqual([])
    expect(normalizeConnectorScope('conn-1')).toEqual([])
    expect(normalizeConnectorScope({ 0: 'conn-1' })).toEqual([])
  })
})

describe('toggleConnectorScope', () => {
  it('adds a connection that is not picked and removes one that is', () => {
    const added = toggleConnectorScope([], 'conn-1')
    expect(added).toEqual(['conn-1'])
    expect(toggleConnectorScope(added, 'conn-1')).toEqual([])
  })

  it('refuses a ninth distinct connection by leaving the selection as it was', () => {
    const full = Array.from({ length: CONNECTOR_SCOPE_LIMIT }, (_, index) => `conn-${index}`)
    expect(toggleConnectorScope(full, 'conn-extra')).toEqual(full)
    // Picking one already in the full selection still removes it, so the person can make room.
    expect(toggleConnectorScope(full, 'conn-0')).toEqual(full.slice(1))
  })

  it('ignores an id the gateway would refuse', () => {
    expect(toggleConnectorScope(['conn-1'], 'not valid')).toEqual(['conn-1'])
    expect(toggleConnectorScope([], '   ')).toEqual([])
  })
})

describe('connectorScopeBlock', () => {
  it('offers connected apps on an open turn', () => {
    expect(connectorScopeBlock(openTurn)).toBeNull()
  })

  it('names each reason that takes connected apps off the turn', () => {
    expect(connectorScopeBlock({ ...openTurn, zeroRetention: true })).toBe('zero_retention')
    expect(connectorScopeBlock({ ...openTurn, spaceScoped: true })).toBe('space')
    expect(connectorScopeBlock({ ...openTurn, conversationOnly: true })).toBe('conversation_only')
    expect(connectorScopeBlock({ ...openTurn, subscriptionBacked: true })).toBe('subscription')
    expect(connectorScopeBlock({ ...openTurn, retrievalScope: 'web_only' })).toBe('web_only')
    expect(connectorScopeBlock({ ...openTurn, retrievalScope: 'knowledge_only' })).toBe('knowledge_only')
  })

  it('reports the reason a person reads first when several apply', () => {
    expect(connectorScopeBlock({ ...openTurn, zeroRetention: true, spaceScoped: true, retrievalScope: 'web_only' }))
      .toBe('zero_retention')
    expect(connectorScopeBlock({ ...openTurn, spaceScoped: true, conversationOnly: true })).toBe('space')
  })
})
