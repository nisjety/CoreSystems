import { describe, expect, it } from 'vitest'
import { deriveConversationNodes } from './derive'
import type { ChatTurn } from './types'
import {
  messageToTurn,
  transcriptTurnToChatTurn,
  turnsToTranscript,
} from '@/features/chat/components/chat-normalizers'

function assistantTurn(overrides: Partial<ChatTurn> = {}): ChatTurn {
  return {
    id: 'a1',
    role: 'assistant',
    content: 'Bergen er regnfullt i dag.',
    createdAt: '2026-08-22T10:00:00.000Z',
    streaming: false,
    tools: [],
    attachments: [],
    ...overrides,
  }
}

const kinds = (turn: ChatTurn) => deriveConversationNodes(turn).map((node) => node.kind)

describe('deriveConversationNodes', () => {
  it('puts the answer first when there is nothing else', () => {
    expect(kinds(assistantTurn())).toEqual(['answer', 'tool-chips', 'attachments'])
  })

  it('keeps reasoning ahead of the answer', () => {
    const nodes = kinds(assistantTurn({ reasoning: 'let me check the forecast' }))
    expect(nodes.indexOf('reasoning')).toBeLessThan(nodes.indexOf('answer'))
  })

  /**
   * The nested-Show precedence that a flat condition list would get wrong: an
   * errored turn keeps its message in `content`, so testing content first would
   * render the error text as though it were the answer.
   */
  it('an errored turn reports failure rather than treating the message as content', () => {
    const nodes = deriveConversationNodes(
      assistantTurn({ status: 'error', content: 'upstream unavailable' }),
    )
    const answer = nodes.find((node) => node.kind === 'answer')
    expect(answer).toEqual({ kind: 'answer', answer: { state: 'failed', message: 'upstream unavailable' } })
  })

  it('a waiting turn with nothing yet is pending, not empty content', () => {
    const nodes = deriveConversationNodes(assistantTurn({ status: 'waiting', content: '' }))
    expect(nodes.find((node) => node.kind === 'answer')).toEqual({
      kind: 'answer',
      answer: { state: 'pending' },
    })
  })

  it('a waiting turn that already has reasoning is NOT pending', () => {
    // Reasoning arriving before the first token is exactly the extended-thinking
    // case; showing a thinking indicator over visible reasoning is wrong.
    const nodes = deriveConversationNodes(
      assistantTurn({ status: 'waiting', content: '', reasoning: 'considering' }),
    )
    expect(nodes.find((node) => node.kind === 'answer')).toMatchObject({
      answer: { state: 'content', streaming: true },
    })
  })

  it('suppresses notices that are meaningless mid-stream or on error', () => {
    const streaming = assistantTurn({
      status: 'waiting',
      confidence: 0.1,
      memoryRecallCount: 3,
      stopReason: 'max_tokens',
      followUps: ['og videre?'],
    })
    const shown = kinds(streaming)
    for (const suppressed of ['low-confidence', 'memory-recall', 'truncated']) {
      expect(shown).not.toContain(suppressed)
    }

    const settled = assistantTurn({
      confidence: 0.1,
      memoryRecallCount: 3,
      stopReason: 'max_tokens',
      followUps: ['og videre?'],
    })
    for (const shownAfter of ['low-confidence', 'memory-recall', 'truncated']) {
      expect(kinds(settled)).toContain(shownAfter)
    }
    // Follow-ups are not derived in either state: section 5 of the design doc
    // rejects generic curiosity chips, and what the model returns here is one
    // (audit item 22). Asserted so restoring the push fails loudly.
    expect(kinds(settled)).not.toContain('follow-ups')
  })

  it('drops a zero memory-recall count instead of rendering "recalled 0"', () => {
    expect(kinds(assistantTurn({ memoryRecallCount: 0 }))).not.toContain('memory-recall')
    expect(kinds(assistantTurn({ memoryRecallCount: 1 }))).toContain('memory-recall')
  })

  it('preserves the full node order for a turn that has everything', () => {
    const rich = assistantTurn({
      reasoning: 'thinking',
      grounding: { sources: [] } as unknown as ChatTurn['grounding'],
      confidence: 0.2,
      memoryRecallCount: 2,
      stopReason: 'max_tokens',
      toolCalls: [{ id: 't1', name: 'shell' } as unknown as NonNullable<ChatTurn['toolCalls']>[number]],
      pendingApprovals: [{ id: 'ap1' } as unknown as NonNullable<ChatTurn['pendingApprovals']>[number]],
      followUps: ['neste?'],
    })
    // Order is behaviour: a reordered list is a visibly different answer.
    expect(kinds(rich)).toEqual([
      'reasoning',
      'answer',
      'grounding',
      'low-confidence',
      'memory-recall',
      'truncated',
      'tool-chips',
      'attachments',
      'steps',
      'approvals',
    ])
    // No 'follow-ups' tail: see the suppression test above.
    expect(kinds(rich)).not.toContain('follow-ups')
  })
})

/**
 * The DeepSeek acceptance gate for the node model.
 *
 * A `ChatTurn` reaches the renderer by three different routes, and each has its
 * own normalizer: a full thread load (`messageToTurn` over the server's
 * messages), a cached/paged rehydrate (`transcriptTurnToChatTurn` over a stored
 * transcript), and a live stream append (the controller mutating a turn in
 * place). If those three disagree, the same answer renders differently depending
 * on how the user arrived at it — reload vs scroll-back vs watching it stream —
 * which is precisely the class of bug that is invisible in any single path's
 * tests.
 *
 * Asserting it on the derived NODE LIST rather than on the turn objects is the
 * point: turns legitimately differ in transport-only fields (`lastFrameId`,
 * `requestId`, `streaming`). What must match is what gets rendered.
 */
describe('three-path equivalence', () => {
  it('a full load, a transcript rehydrate, and a live append derive the same nodes', () => {
    const live = assistantTurn({
      content: 'Bergen er regnfullt i dag.',
      confidence: 0.9,
      // Transport-only fields differ per path by design.
      requestId: 'req-live',
      lastFrameId: '42',
    })

    // Path A — full thread load from the server's message shape.
    const fullLoad = messageToTurn({
      id: 'a1',
      role: 'assistant',
      content: live.content,
      createdAt: live.createdAt,
    } as unknown as Parameters<typeof messageToTurn>[0])

    // Path B — rehydrate from the persisted transcript (the paged/prepend path).
    const [transcriptTurn] = turnsToTranscript([live])
    expect(transcriptTurn).toBeDefined()
    const rehydrated = transcriptTurnToChatTurn(transcriptTurn!)

    const liveNodes = deriveConversationNodes(live)
    const fullLoadNodes = deriveConversationNodes(fullLoad)
    const rehydratedNodes = deriveConversationNodes(rehydrated)

    // The answer content is what a reader actually sees; assert it directly
    // rather than only comparing kind sequences, which would pass on empty text.
    for (const [label, nodes] of [
      ['full load', fullLoadNodes],
      ['rehydrated', rehydratedNodes],
    ] as const) {
      const answer = nodes.find((node) => node.kind === 'answer')
      expect(answer, `${label} produced no answer node`).toMatchObject({
        answer: { state: 'content', content: live.content },
      })
    }

    expect(fullLoadNodes.map((n) => n.kind)).toEqual(liveNodes.map((n) => n.kind))
    expect(rehydratedNodes.map((n) => n.kind)).toEqual(liveNodes.map((n) => n.kind))
  })

  it('a partial live turn and its persisted form agree once it settles', () => {
    // Mid-stream the live turn is `waiting`; the persisted form never is. The
    // gate is that the SETTLED live turn matches the rehydrated one — otherwise
    // an answer changes shape the moment it is reloaded.
    const settled = assistantTurn({ content: 'ferdig', memoryRecallCount: 2, stopReason: 'end_turn' })
    const [persisted] = turnsToTranscript([settled])
    const rehydrated = transcriptTurnToChatTurn(persisted!)
    expect(deriveConversationNodes(rehydrated).map((n) => n.kind)).toEqual(
      deriveConversationNodes(settled).map((n) => n.kind),
    )
  })
})

/**
 * The merge path is the fourth route a turn can take: a thread refresh maps the
 * server's persisted messages and folds cached session metadata back in. The
 * server stores neither `memoryRecallCount` nor `stopReason` (both are derived
 * from SSE events, not from the message), so without a fallback a refresh
 * silently drops them — and dropping `stopReason` turns an answer that "may be
 * cut off" into one that looks complete.
 */
describe('server/cache merge preserves session-only notices', () => {
  it('keeps the recall count and stop reason a refresh would otherwise drop', async () => {
    const { mergeServerTurnsWithCachedMetadata } = await import(
      '@/features/chat/components/chat-normalizers'
    )
    const cached = assistantTurn({ memoryRecallCount: 3, stopReason: 'max_tokens' })
    // What the server actually returns: the message, with no session metadata.
    const fromServer = assistantTurn({ memoryRecallCount: undefined, stopReason: undefined })

    const [merged] = mergeServerTurnsWithCachedMetadata([fromServer], [cached])
    expect(merged).toBeDefined()
    const kindsAfter = deriveConversationNodes(merged!).map((node) => node.kind)
    expect(kindsAfter).toContain('memory-recall')
    expect(kindsAfter).toContain('truncated')
  })
})

/**
 * The recalled-memory list is session-derived like `memoryRecallCount` and
 * `stopReason`, so it has the same three-path exposure those two had — and it is
 * the one that matters most to preserve: the disclosure exists so a wrong
 * remembered fact can be found and corrected later, which requires it to still
 * be there later.
 */
describe('recalled memories survive every path', () => {
  const recalled = [
    {
      memoryId: 'mem-1',
      role: 'recall' as const,
      origin: 'stated' as const,
      label: 'USER',
      preview: 'Prefers metric units',
    },
  ]

  it('round-trips through the persisted transcript', async () => {
    const { turnsToTranscript, transcriptTurnToChatTurn } = await import(
      '@/features/chat/components/chat-normalizers'
    )
    const live = assistantTurn({ memoryRecallCount: 1, recalledMemories: recalled })
    const [persisted] = turnsToTranscript([live])
    const rehydrated = transcriptTurnToChatTurn(persisted!)
    expect(rehydrated.recalledMemories).toEqual(recalled)

    const node = deriveConversationNodes(rehydrated).find((entry) => entry.kind === 'memory-recall')
    expect(node).toMatchObject({ count: 1, memories: recalled })
  })

  it('survives a thread refresh, which the server cannot repopulate', async () => {
    const { mergeServerTurnsWithCachedMetadata } = await import(
      '@/features/chat/components/chat-normalizers'
    )
    const cached = assistantTurn({ memoryRecallCount: 1, recalledMemories: recalled })
    const fromServer = assistantTurn({ memoryRecallCount: undefined, recalledMemories: undefined })
    const [merged] = mergeServerTurnsWithCachedMetadata([fromServer], [cached])
    expect(merged?.recalledMemories).toEqual(recalled)
  })

  /**
   * A stored row whose origin this build does not recognise must be rejected by
   * the guard rather than rehydrated as `stated`.
   */
  it('rejects a persisted row with an unrecognised origin', async () => {
    const { transcriptTurnToChatTurn } = await import(
      '@/features/chat/components/chat-normalizers'
    )
    const rehydrated = transcriptTurnToChatTurn({
      id: 'a1',
      role: 'assistant',
      content: 'x',
      createdAt: '2026-08-22T10:00:00.000Z',
      recalledMemories: [
        { memoryId: 'm1', role: 'recall', origin: 'some_future_value', label: 'X', preview: 'p' },
      ],
    })
    expect(rehydrated.recalledMemories).toEqual([])
  })

  /** A count with no list still renders the notice — it degrades, never vanishes. */
  it('a count without a list still produces the node', () => {
    const node = deriveConversationNodes(assistantTurn({ memoryRecallCount: 2 })).find(
      (entry) => entry.kind === 'memory-recall',
    )
    expect(node).toMatchObject({ count: 2, memories: [] })
  })
})
