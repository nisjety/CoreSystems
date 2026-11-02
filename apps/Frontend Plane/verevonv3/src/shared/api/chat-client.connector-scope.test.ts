import { describe, expect, it } from 'vitest'
import {
  buildChatWireBody,
  catalogueAdvertisesConnectorScope,
  type ChatInvokeRequest,
} from './chat-client'

// WS23 S6: `connector_scope` is the wire form of the composer's connected-apps chips. These
// tests pin the two rules that matter: the field is present only for a turn that picked an
// account and may take one, and the catalogue, not the client, says whether the reads exist.

const ask: ChatInvokeRequest = { content: 'Hva sendte Ada meg?', model: 'verevon-balance' }

describe('buildChatWireBody connector_scope', () => {
  it('leaves the field out of an ordinary turn', () => {
    expect(buildChatWireBody(ask)).not.toHaveProperty('connector_scope')
    expect(buildChatWireBody({ ...ask, connectorScope: [] })).not.toHaveProperty('connector_scope')
  })

  it('sends the picked connections on a turn that may take them', () => {
    expect(buildChatWireBody({ ...ask, connectorScope: ['conn-ms-1', 'conn-g-2'] }))
      .toMatchObject({ connector_scope: ['conn-ms-1', 'conn-g-2'] })
  })

  it('sends only well-formed, distinct ids, at most the gateway limit', () => {
    const picked = ['conn-ms-1', 'conn-ms-1', 'bad id', ...Array.from({ length: 9 }, (_, i) => `conn-${i}`)]
    expect(buildChatWireBody({ ...ask, connectorScope: picked }).connector_scope).toEqual(
      ['conn-ms-1', 'conn-0', 'conn-1', 'conn-2', 'conn-3', 'conn-4', 'conn-5', 'conn-6'],
    )
  })

  it('withholds the field on every turn the composer blocks, even when a pick is present', () => {
    const picked = ['conn-ms-1']
    const blocked: Array<Partial<ChatInvokeRequest>> = [
      { zdr: true },
      { spaceRef: 'space-1' },
      { sourceScope: 'conversation' },
      { retrievalScope: 'web_only' },
      { retrievalScope: 'knowledge_only' },
      { provider: 'openai-codex-subscription' },
      // An execute turn runs an approved plan under the grant it was approved with: it has no picks.
      { threadId: 'thread-1', executeGranted: true },
    ]
    for (const turn of blocked) {
      expect(buildChatWireBody({ ...ask, ...turn, connectorScope: picked }), JSON.stringify(turn))
        .not.toHaveProperty('connector_scope')
    }
  })
})

describe('catalogueAdvertisesConnectorScope', () => {
  it('advertises connected apps when some chat model lists the capability', () => {
    expect(catalogueAdvertisesConnectorScope([
      { modality: 'chat', capabilities: ['vision'] },
      { modality: 'chat', capabilities: ['connector_scope'] },
    ])).toBe(true)
  })

  it('does not advertise it from a non-chat model, or from a catalogue that does not list it', () => {
    expect(catalogueAdvertisesConnectorScope([{ modality: 'image', capabilities: ['connector_scope'] }])).toBe(false)
    expect(catalogueAdvertisesConnectorScope([{ modality: 'chat', capabilities: ['agentic'] }])).toBe(false)
    expect(catalogueAdvertisesConnectorScope([{ modality: 'chat' }])).toBe(false)
    // A catalogue that has not answered never switches the chips on by guess.
    expect(catalogueAdvertisesConnectorScope([])).toBe(false)
  })
})
