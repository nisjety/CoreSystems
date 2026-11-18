import { describe, expect, it } from 'vitest'

import {
  buildMentionsPayload,
  MAX_ADDRESSED_AGENTS,
  MAX_MENTIONED_PEOPLE,
  mentionPresent,
  qualifyCandidates,
  type AddressedAgent,
} from './mention-model'

describe('mentionPresent (WS08-S6)', () => {
  it('finds @label as a whole word: @Kari is not inside @Karin or @Kari2', () => {
    expect(mentionPresent('Hei @Kari', 'Kari')).toBe(true)
    expect(mentionPresent('Hei @Karin', 'Kari')).toBe(false)
    expect(mentionPresent('Hei @Kari2', 'Kari')).toBe(false)
  })

  it('needs the @ to start the text or follow whitespace', () => {
    expect(mentionPresent('@Kari', 'Kari')).toBe(true)
    expect(mentionPresent('post@Kari', 'Kari')).toBe(false)
  })

  it('compares case-insensitively', () => {
    expect(mentionPresent('hei @KARI', 'Kari')).toBe(true)
    expect(mentionPresent('Hei @kari', 'KARI')).toBe(true)
  })

  it('compares in Unicode NFC, so a decomposed and a composed letter match in either direction', () => {
    const composed = 'Åse'
    const decomposed = 'Åse'
    expect(mentionPresent(`Hei @${decomposed}`, composed)).toBe(true)
    expect(mentionPresent(`Hei @${composed}`, decomposed)).toBe(true)
    // Only NFC and case are folded: a plain A is a different name.
    expect(mentionPresent('Hei @Ase', composed)).toBe(false)
  })

  it('never matches an empty label', () => {
    expect(mentionPresent('Hei @', '')).toBe(false)
  })
})

describe('qualifyCandidates namesakes (WS08-S6)', () => {
  it('leaves a name nobody else shares bare, with the id the gateway checks against', () => {
    const [kari, lager] = qualifyCandidates([
      { kind: 'person', subjectId: 'user-0001', name: 'Kari', role: 'editor' },
      { kind: 'agent', subjectId: 'agent-0002', name: 'Lager' },
    ])
    expect(kari).toMatchObject({ id: 'user:user-0001', label: 'Kari' })
    expect(lager).toMatchObject({ id: 'agent:agent-0002', label: 'Lager' })
  })

  it('adds the room role to namesakes that have different roles', () => {
    const out = qualifyCandidates([
      { kind: 'person', subjectId: 'user-0001', name: 'Kari', role: 'editor' },
      { kind: 'person', subjectId: 'user-0002', name: 'Kari', role: 'manager' },
    ])
    expect(out.map((candidate) => candidate.label)).toEqual(['Kari (editor)', 'Kari (manager)'])
  })

  it('adds the last four characters of the subject id when the role does not tell namesakes apart', () => {
    const out = qualifyCandidates([
      { kind: 'person', subjectId: 'user-1111', name: 'Kari', role: 'editor' },
      { kind: 'person', subjectId: 'user-2222', name: 'Kari', role: 'editor' },
    ])
    expect(out.map((candidate) => candidate.label)).toEqual(['Kari (editor 1111)', 'Kari (editor 2222)'])
  })

  it('counts an agent as a namesake of a person, with the role agent', () => {
    const out = qualifyCandidates([
      { kind: 'person', subjectId: 'user-0001', name: 'Lager', role: 'editor' },
      { kind: 'agent', subjectId: 'agent-0002', name: 'Lager' },
    ])
    expect(out.map((candidate) => candidate.label)).toEqual(['Lager (editor)', 'Lager (agent)'])
  })

  it('treats names that differ only in case or diacritics as the same name', () => {
    const out = qualifyCandidates([
      { kind: 'person', subjectId: 'user-0001', name: 'Åse', role: 'editor' },
      { kind: 'person', subjectId: 'user-0002', name: 'ase', role: 'manager' },
    ])
    expect(out.map((candidate) => candidate.label)).toEqual(['Åse (editor)', 'ase (manager)'])
  })
})

describe('buildMentionsPayload (WS08-S6)', () => {
  const kari = { subjectId: 'user-0001', label: 'Kari' }
  const ola = { subjectId: 'user-0002', label: 'Ola' }
  const lager: AddressedAgent = { subjectId: 'agent-0001', label: 'Lager', source: 'typed' }
  const innkjop: AddressedAgent = { subjectId: 'agent-0002', label: 'Innkjop', source: 'locked' }

  it('keeps a person or a typed agent only while its label is still in the words', () => {
    expect(buildMentionsPayload({ text: 'Hei @Kari', people: [kari, ola], agents: [lager] })).toEqual({
      people: [{ subjectId: 'user-0001', label: 'Kari' }],
      agents: [],
    })
  })

  it('always sends a locked agent, even when its label is not in the words', () => {
    expect(buildMentionsPayload({ text: 'Hei', people: [], agents: [innkjop, lager] })).toEqual({
      people: [],
      agents: [{ subjectId: 'agent-0002', source: 'locked' }],
    })
  })

  it('sends a typed agent without a source, and a locked agent with source locked', () => {
    expect(buildMentionsPayload({ text: 'Hei @Lager', people: [], agents: [lager, innkjop] })).toEqual({
      people: [],
      agents: [{ subjectId: 'agent-0001' }, { subjectId: 'agent-0002', source: 'locked' }],
    })
  })

  it('dedupes a subject id that is picked twice', () => {
    expect(
      buildMentionsPayload({
        text: '@Kari @Kari @Lager',
        people: [kari, { ...kari }],
        agents: [lager, { ...lager }],
      }),
    ).toEqual({
      people: [{ subjectId: 'user-0001', label: 'Kari' }],
      agents: [{ subjectId: 'agent-0001' }],
    })
  })

  it('caps the people at 20 and the agents at 3, keeping the first in order', () => {
    expect(MAX_MENTIONED_PEOPLE).toBe(20)
    expect(MAX_ADDRESSED_AGENTS).toBe(3)
    const people = Array.from({ length: 25 }, (_, index) => ({
      subjectId: `user-${index}`,
      label: `P${String(index).padStart(2, '0')}`,
    }))
    const agents: AddressedAgent[] = Array.from({ length: 5 }, (_, index) => ({
      subjectId: `agent-${index}`,
      label: `A${index}`,
      source: 'typed',
    }))
    const text = [...people, ...agents].map((pick) => `@${pick.label}`).join(' ')
    const payload = buildMentionsPayload({ text, people, agents })
    expect(payload?.people).toHaveLength(20)
    expect(payload?.people[19]?.subjectId).toBe('user-19')
    expect(payload?.agents).toHaveLength(3)
  })

  it('is undefined when nothing is left to send', () => {
    expect(buildMentionsPayload({ text: 'Hei', people: [kari], agents: [lager] })).toBeUndefined()
    expect(buildMentionsPayload({ text: 'Hei', people: [], agents: [] })).toBeUndefined()
  })
})
