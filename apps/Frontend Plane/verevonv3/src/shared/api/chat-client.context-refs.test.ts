import { describe, expect, it } from 'vitest'
import { buildChatWireBody, type ChatInvokeRequest } from './chat-client'
import {
  catalogueAdvertisesContextRefs,
  normalizeContextRefs,
  removeContextRef,
  type ChatContextRef,
} from '@/shared/actions/chat-context-refs'

// EP-3 S5: `context_refs` is the wire form of the composer's context chips. These tests pin the
// rules that matter: the field is present only when a chip was attached and the turn may carry it,
// its shape is the gateway's, and the catalogue, not the client, says whether references exist.

const ask: ChatInvokeRequest = { content: 'Hva sier budsjettet?', model: 'verevon-balance' }
const budget: ChatContextRef = { kind: 'document', id: 'doc-1', label: 'Budsjett 2027' }

describe('buildChatWireBody context_refs', () => {
  it('omitted_when_empty', () => {
    expect(buildChatWireBody(ask)).not.toHaveProperty('context_refs')
    expect(buildChatWireBody({ ...ask, contextRefs: [] })).not.toHaveProperty('context_refs')
  })

  it('sends the attached chips as kind, id and label', () => {
    expect(buildChatWireBody({ ...ask, contextRefs: [budget, { kind: 'skill', id: 'skill.a', label: 'Skriv' }] }))
      .toMatchObject({ context_refs: [budget, { kind: 'skill', id: 'skill.a', label: 'Skriv' }] })
  })

  it('never_sent_on_temporary', () => {
    // A temporary chat and a zero-retention workspace both send `zdr`; the gateway refuses references there.
    expect(buildChatWireBody({ ...ask, zdr: true, contextRefs: [budget] })).not.toHaveProperty('context_refs')
  })

  it('is withheld on a subscription route', () => {
    expect(buildChatWireBody({
      ...ask,
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-1',
      contextRefs: [budget],
    })).not.toHaveProperty('context_refs')
  })
})

describe('normalizeContextRefs', () => {
  it('drops unknown kinds, malformed ids and repeats, and keeps the gateway limits', () => {
    const refs = normalizeContextRefs([
      budget,
      budget,
      { kind: 'mention', id: 'x', label: 'x' },
      { kind: 'document', id: 'a b', label: 'bad' },
      { kind: 'skill', id: ' skill.a ', label: '  Skriv  ' },
      'doc-2',
    ])
    expect(refs).toEqual([budget, { kind: 'skill', id: 'skill.a', label: 'Skriv' }])
    const many = Array.from({ length: 25 }, (_, n) => ({ kind: 'document', id: `d${n}`, label: '' }))
    expect(normalizeContextRefs(many)).toHaveLength(20)
  })

  it('cuts a label to 120 characters', () => {
    const [ref] = normalizeContextRefs([{ ...budget, label: 'é'.repeat(200) }])
    expect(Array.from(ref?.label ?? '')).toHaveLength(120)
  })

  it('removes a chip whole', () => {
    expect(removeContextRef([budget, { kind: 'skill', id: 'doc-1', label: 'same id' }], budget))
      .toEqual([{ kind: 'skill', id: 'doc-1', label: 'same id' }])
  })
})

describe('catalogueAdvertisesContextRefs', () => {
  it('advertises references only when a chat model lists the capability', () => {
    expect(catalogueAdvertisesContextRefs([{ modality: 'chat', capabilities: ['context_refs'] }])).toBe(true)
    expect(catalogueAdvertisesContextRefs([{ modality: 'image', capabilities: ['context_refs'] }])).toBe(false)
    expect(catalogueAdvertisesContextRefs([{ modality: 'chat', capabilities: ['vision'] }])).toBe(false)
    expect(catalogueAdvertisesContextRefs([])).toBe(false)
  })
})
