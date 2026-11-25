import { describe, expect, it } from 'vitest'
import type { ChatTurn } from './chat-types'
import {
  savedAnswerForTrailingTurn,
  transcriptTurnToChatTurn,
  truncationReachesLeadingTurns,
  turnsOutsideLoadedWindow,
} from './chat-normalizers'

const turnOf = (id: string, role: ChatTurn['role'] = 'user', content = id): ChatTurn => ({
  id,
  role,
  content,
  createdAt: '2026-11-23T10:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
})

const SOURCE = { id: 'src-1', title: 'Rapport', url: 'https://example.test/rapport', snippet: 'Shared folder' }

describe('turnsOutsideLoadedWindow (F-f01-25)', () => {
  const turns = ['a', 'b', 'c', 'd'].map((id) => turnOf(id))

  it('names the leading turns, the ones before the window, while older pages remain', () => {
    expect([...turnsOutsideLoadedWindow(turns, 2, true)]).toEqual(['a', 'b'])
  })

  it('names no turn once no older page remains', () => {
    expect(turnsOutsideLoadedWindow(turns, 2, false).size).toBe(0)
  })

  it('names no turn when there are no leading turns', () => {
    expect(turnsOutsideLoadedWindow(turns, 0, true).size).toBe(0)
  })

  it('names no more turns than the list holds', () => {
    expect([...turnsOutsideLoadedWindow(turns.slice(0, 1), 5, true)]).toEqual(['a'])
  })

  it('leaves the turns themselves alone: it only names them', () => {
    turnsOutsideLoadedWindow(turns, 2, true)
    expect(turns.map((turn) => turn.id)).toEqual(['a', 'b', 'c', 'd'])
  })
})

describe('truncationReachesLeadingTurns (F-f01-25)', () => {
  it('is true when a cut keeps fewer turns than the leading ones', () => {
    expect(truncationReachesLeadingTurns(1, 2)).toBe(true)
    expect(truncationReachesLeadingTurns(0, 2)).toBe(true)
  })

  it('is false when a cut keeps the leading turns whole, or leaves the window alone', () => {
    expect(truncationReachesLeadingTurns(2, 2)).toBe(false)
    expect(truncationReachesLeadingTurns(3, 2)).toBe(false)
    expect(truncationReachesLeadingTurns(0, 0)).toBe(false)
  })
})

describe('saved answer read through the transcript (F-f01-24)', () => {
  it('adopts a saved answer with the citations the transcript carries for it', () => {
    const local = [turnOf('q', 'user', 'Question?'), { ...turnOf('a', 'assistant', 'Partial'), streaming: true }]
    const server = [
      transcriptTurnToChatTurn({ id: 'q-srv', role: 'user', content: 'Question?', createdAt: '' }),
      transcriptTurnToChatTurn({
        id: 'a-srv',
        role: 'assistant',
        content: 'The saved answer.',
        createdAt: '',
        citations: [SOURCE],
      }),
    ]

    const answer = savedAnswerForTrailingTurn(local, 'a', server, { partial: true })

    expect(answer?.content).toBe('The saved answer.')
    expect(answer?.citations).toMatchObject([{ title: SOURCE.title, url: SOURCE.url }])
  })
})
