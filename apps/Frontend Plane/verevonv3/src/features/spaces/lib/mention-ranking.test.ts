import { describe, expect, it } from 'vitest'

import type { MentionCandidate } from './mention-model'
import { orderBySection, rankMentionCandidates, sectionOf } from './mention-ranking'

const person = (subjectId: string, name: string): MentionCandidate => ({
  id: `user:${subjectId}`,
  kind: 'person',
  subjectId,
  name,
  label: name,
  role: 'editor',
})

const agent = (subjectId: string, name: string): MentionCandidate => ({
  id: `agent:${subjectId}`,
  kind: 'agent',
  subjectId,
  name,
  label: name,
})

const names = (list: readonly MentionCandidate[]) => list.map((candidate) => candidate.name)

describe('rankMentionCandidates (WS08-S6)', () => {
  it('matches on the diacritic-folded name, so a plain query finds a name with diacritics', () => {
    const candidates = [person('user-0001', 'Åse'), person('user-0002', 'Ola')]
    expect(names(rankMentionCandidates(candidates, 'as', []))).toEqual(['Åse'])
    expect(names(rankMentionCandidates(candidates, 'AS', []))).toEqual(['Åse'])
    expect(names(rankMentionCandidates(candidates, 'Å', []))).toEqual(['Åse'])
  })

  it('matches the start of any word, split on spaces, dashes, underscores and dots', () => {
    const candidates = [person('user-0001', 'Kari Hansen'), person('user-0002', 'Per-Anders')]
    expect(names(rankMentionCandidates(candidates, 'han', []))).toEqual(['Kari Hansen'])
    expect(names(rankMentionCandidates(candidates, 'anders', []))).toEqual(['Per-Anders'])
  })

  it('ranks a name that starts with the query ahead of one where only a later word does, whatever the recency', () => {
    const candidates = [person('user-0001', 'Kari Hansen'), person('user-0002', 'Hanne Berg')]
    // "Hanne" starts with "han"; "Hansen" only starts a later word of "Kari Hansen".
    expect(names(rankMentionCandidates(candidates, 'han', []))).toEqual(['Hanne Berg', 'Kari Hansen'])
    expect(names(rankMentionCandidates(candidates, 'han', ['user-0001']))).toEqual(['Hanne Berg', 'Kari Hansen'])
  })

  it('orders equal matches by recency, most recent first, and puts unseen candidates after them', () => {
    const candidates = [person('user-0001', 'Ola'), person('user-0002', 'Kari'), person('user-0003', 'Per')]
    expect(names(rankMentionCandidates(candidates, '', ['user-0003', 'user-0002']))).toEqual(['Per', 'Kari', 'Ola'])
  })

  it('breaks a tie with no recency by name, alphabetically', () => {
    const candidates = [person('user-0001', 'Per'), person('user-0002', 'Kari'), person('user-0003', 'Ola')]
    expect(names(rankMentionCandidates(candidates, '', []))).toEqual(['Kari', 'Ola', 'Per'])
  })
})

describe('orderBySection (WS08-S6)', () => {
  it('lays people out before agents and keeps the ranked order inside each section', () => {
    const ranked = [
      agent('agent-0001', 'Lager'),
      person('user-0002', 'Kari'),
      agent('agent-0003', 'Innkjop'),
      person('user-0004', 'Ola'),
    ]
    expect(names(orderBySection(ranked))).toEqual(['Kari', 'Ola', 'Lager', 'Innkjop'])
    expect(ranked.map((candidate) => sectionOf(candidate))).toEqual(['agents', 'people', 'agents', 'people'])
  })
})
