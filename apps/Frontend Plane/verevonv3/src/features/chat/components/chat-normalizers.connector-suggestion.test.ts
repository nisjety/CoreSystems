import { describe, expect, it } from 'vitest'
import {
  CONNECTOR_SUGGESTION_MAX_REASON_CHARS,
  mergeServerTurnsWithCachedMetadata,
  normalizeConnectorSuggestion,
  restoredConnectorSuggestion,
  transcriptTurnToChatTurn,
  turnsToTranscript,
} from './chat-normalizers'
import type { ChatTurn } from './chat-types'

const frame = {
  suggestion_id: 'sug_01',
  provider: 'google',
  reason: 'Your mail would answer this.',
  request_id: 'req-1',
}

function assistantTurn(overrides: Partial<ChatTurn> = {}): ChatTurn {
  return {
    id: 'a1',
    role: 'assistant',
    content: 'I cannot see your mail on this turn.',
    createdAt: '2026-11-05T10:00:00Z',
    streaming: false,
    tools: [],
    attachments: [],
    ...overrides,
  }
}

describe('connector_suggestion frames', () => {
  it('reads the wire frame and the stream client spelling alike', () => {
    const expected = {
      suggestionId: 'sug_01',
      provider: 'google',
      reason: 'Your mail would answer this.',
      requestId: 'req-1',
    }
    expect(normalizeConnectorSuggestion(frame)).toEqual(expected)
    expect(normalizeConnectorSuggestion(expected)).toEqual(expected)
  })

  it('accepts only the closed provider list', () => {
    expect(normalizeConnectorSuggestion({ ...frame, provider: 'microsoft' })?.provider).toBe('microsoft')
    for (const provider of ['slack', 'Google', '', undefined, 1]) {
      expect(normalizeConnectorSuggestion({ ...frame, provider })).toBeNull()
    }
  })

  it('drops a frame with no id or no reason, and anything that is not an object', () => {
    expect(normalizeConnectorSuggestion({ ...frame, suggestion_id: '' })).toBeNull()
    expect(normalizeConnectorSuggestion({ ...frame, suggestion_id: '   ' })).toBeNull()
    expect(normalizeConnectorSuggestion({ ...frame, reason: '  \n ' })).toBeNull()
    expect(normalizeConnectorSuggestion({ ...frame, reason: 5 })).toBeNull()
    for (const value of [null, undefined, 'sug_01', 3]) {
      expect(normalizeConnectorSuggestion(value)).toBeNull()
    }
  })

  it('shows the reason as one bounded line', () => {
    expect(normalizeConnectorSuggestion({ ...frame, reason: ' Your\nmail\t would  help ' })?.reason).toBe(
      'Your mail would help',
    )
    const long = normalizeConnectorSuggestion({ ...frame, reason: 'a'.repeat(CONNECTOR_SUGGESTION_MAX_REASON_CHARS + 50) })
    expect(long?.reason).toHaveLength(CONNECTOR_SUGGESTION_MAX_REASON_CHARS)
  })

  it('leaves out a request id the frame does not carry', () => {
    const { request_id: _requestId, ...withoutRequest } = frame
    expect(normalizeConnectorSuggestion(withoutRequest)).not.toHaveProperty('requestId')
  })
})

describe('a suggestion after a reload', () => {
  it('is marked restored, so the card offers nothing', () => {
    expect(restoredConnectorSuggestion(frame)).toMatchObject({ suggestionId: 'sug_01', restored: true })
    expect(restoredConnectorSuggestion({ ...frame, provider: 'slack' })).toBeUndefined()
    expect(restoredConnectorSuggestion(undefined)).toBeUndefined()
  })

  it('survives the cached transcript and comes back restored', () => {
    const live = assistantTurn({ connectorSuggestion: normalizeConnectorSuggestion(frame) ?? undefined })
    const [stored] = turnsToTranscript([live])
    expect(stored?.connectorSuggestion).toEqual(live.connectorSuggestion)
    const restored = transcriptTurnToChatTurn(JSON.parse(JSON.stringify(stored)))
    expect(restored.connectorSuggestion).toMatchObject({ provider: 'google', restored: true })
  })

  it('is carried onto the server turn from the cache, since the server message never has it', () => {
    const cached = assistantTurn({ connectorSuggestion: normalizeConnectorSuggestion(frame) ?? undefined })
    const [merged] = mergeServerTurnsWithCachedMetadata([assistantTurn()], [cached])
    expect(merged?.connectorSuggestion).toMatchObject({ suggestionId: 'sug_01', restored: true })
  })

  it('a turn without one stays without one', () => {
    const [stored] = turnsToTranscript([assistantTurn()])
    expect(transcriptTurnToChatTurn(stored as NonNullable<typeof stored>).connectorSuggestion).toBeUndefined()
    const [merged] = mergeServerTurnsWithCachedMetadata([assistantTurn()], [assistantTurn()])
    expect(merged?.connectorSuggestion).toBeUndefined()
  })
})
