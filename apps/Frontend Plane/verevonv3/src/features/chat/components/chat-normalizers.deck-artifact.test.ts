import { describe, expect, it } from 'vitest'
import {
  DECK_MANIFEST_VERSION,
  deckArtifactForDisplay,
  deckManifestVersion,
  isChatArtifact,
  transcriptTurnToChatTurn,
} from './chat-normalizers'

// EP-5 S3: a deck is kept with its message as a manifest artifact of kind
// `deck`. The SPA reads back a version 1 manifest as a deck, shows a manifest
// of any other version as its JSON in a code block, and refuses a deck whose
// content is not a JSON object.

const MANIFEST_V1 = JSON.stringify({ version: 1, deck_id: 'deck-01test', title: 'Kvartalstall', slides: [] })

function deck(content: string) {
  return { id: 'deck-01test', kind: 'deck', title: 'Kvartalstall', content, version: 2 }
}

describe('a deck artifact read back from a stored turn', () => {
  it('accepts a version 1 manifest as a deck', () => {
    expect(DECK_MANIFEST_VERSION).toBe(1)
    expect(isChatArtifact(deck(MANIFEST_V1))).toBe(true)
    const turn = transcriptTurnToChatTurn({
      id: 'turn-1',
      role: 'assistant',
      content: 'Presentasjonen er klar.',
      createdAt: '',
      artifacts: [deck(MANIFEST_V1)],
    })
    expect(turn.artifacts).toEqual([expect.objectContaining({ id: 'deck-01test', kind: 'deck', version: 2 })])
  })

  it('shows an unknown manifest version as a plain code block, never as a deck', () => {
    for (const content of [JSON.stringify({ version: 2, slides: [] }), '{"slides":[]}', '{"version":"1"}']) {
      expect(isChatArtifact(deck(content))).toBe(true)
      const shown = deckArtifactForDisplay(deck(content))
      expect(shown.kind).toBe('code')
      expect(shown.content).toBe(content)
    }
    const turn = transcriptTurnToChatTurn({
      id: 'turn-2',
      role: 'assistant',
      content: '',
      createdAt: '',
      artifacts: [deck(JSON.stringify({ version: 9 }))],
    })
    expect(turn.artifacts).toEqual([expect.objectContaining({ id: 'deck-01test', kind: 'code' })])
  })

  it('refuses a deck whose content is not a JSON object', () => {
    for (const content of ['', '{not json', '[1,2]', 'null', '"deck"', '42']) {
      expect(deckManifestVersion(content)).toBeNull()
      expect(isChatArtifact(deck(content))).toBe(false)
    }
    const turn = transcriptTurnToChatTurn({
      id: 'turn-3',
      role: 'assistant',
      content: 'Svar.',
      createdAt: '',
      artifacts: [deck('{not json'), { id: 'doc', kind: 'document', title: 'Notat', content: '# Notat', version: 1 }],
    })
    expect(turn.artifacts?.map((artifact) => artifact.id)).toEqual(['doc'])
  })

  it('leaves every other kind as it is', () => {
    const document = { id: 'doc', kind: 'document', title: 'Notat', content: '{not json', version: 1 }
    expect(isChatArtifact(document)).toBe(true)
    expect(deckArtifactForDisplay(document)).toBe(document)
  })
})
