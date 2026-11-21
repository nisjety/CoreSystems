// @vitest-environment jsdom

/**
 * The note a retracted mid-run message leaves in the thread (O-01, DEC-F01-03).
 *
 * The server answers a message the person took back as a text-free row flagged `retracted`. The thread
 * shows it as one quiet note, and nothing replays it: it is not the turn a regenerate or a retry reads,
 * and two of them close together stay two.
 */

import { cleanup, render } from '@solidjs/testing-library'
import { afterEach, describe, expect, it } from 'vitest'
import type { ChatMessage } from '@/shared/api/chat-client'
import { lastUserIndex } from '@/features/chat/lib/chat-versions'
import { MessageBlock } from './ChatMessages'
import { dedupeChatTurns, messageToTurn } from './chat-normalizers'
import type { ChatTurn } from './chat-types'

afterEach(() => cleanup())

const note = (id: string, createdAt = '2026-11-20T10:00:00Z'): ChatMessage => ({
  id,
  role: 'user',
  content: '',
  createdAt,
  retracted: true,
})

const turn = (id: string, role: ChatTurn['role'], content: string): ChatTurn => ({
  id,
  role,
  content,
  createdAt: '2026-11-20T10:00:00Z',
  streaming: false,
  tools: [],
  attachments: [],
})

describe('a retracted message in the thread', () => {
  it('is read from the server row as a flagged turn with no text', () => {
    const converted = messageToTurn(note('m-1'))
    expect(converted.retracted).toBe(true)
    expect(converted.content).toBe('')
  })

  it('leaves every other turn exactly as it was', () => {
    const ordinary = messageToTurn({ id: 'm-2', role: 'user', content: 'hei', createdAt: '2026-11-20T10:00:00Z' })
    expect('retracted' in ordinary).toBe(false)
  })

  it('shows one text-free note, and offers nothing to do with it', () => {
    // Only the retracted branch runs, which reads nothing but the turn.
    const props = { message: messageToTurn(note('m-1')) } as unknown as Parameters<typeof MessageBlock>[0]
    const view = render(() => <MessageBlock {...props} />)

    const shown = view.container.querySelector('.verevon-chat-retracted')
    expect(shown?.getAttribute('role')).toBe('note')
    expect(shown?.textContent).toBe('Melding trukket tilbake')
    expect(view.container.querySelector('button')).toBeNull()
    expect(view.container.textContent).toBe('Melding trukket tilbake')
  })

  it('stays two notes when two messages were taken back close together', () => {
    const turns = [messageToTurn(note('m-1')), messageToTurn(note('m-2', '2026-11-20T10:00:20Z'))]
    expect(dedupeChatTurns(turns).map((shown) => shown.id)).toEqual(['m-1', 'm-2'])
    // The same words from the same minute are still collapsed for ordinary turns.
    const twice = [turn('a', 'user', 'hei'), turn('b', 'user', 'hei')]
    expect(dedupeChatTurns(twice)).toHaveLength(1)
  })

  it('is never the turn a regenerate or a retry replays', () => {
    const turns = [turn('u1', 'user', 'første'), turn('a1', 'assistant', 'svar'), messageToTurn(note('m-1'))]
    expect(lastUserIndex(turns)).toBe(0)
    expect(lastUserIndex([messageToTurn(note('m-1'))])).toBe(-1)
  })
})
