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
    // No action keys on an ordinary failure: its shape is what it always was.
    expect(Object.keys((answer as Extract<typeof answer, { kind: 'answer' }>).answer)).toEqual(['state', 'message'])
  })

  it('carries what the reader can do about a failure, and when, onto the failed answer', () => {
    const answer = deriveConversationNodes(assistantTurn({
      status: 'error',
      content: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex.',
      failureAction: 'wait',
      retryAt: '2026-09-26T01:21:00Z',
    })).find((node) => node.kind === 'answer')
    expect(answer).toEqual({
      kind: 'answer',
      answer: {
        state: 'failed',
        message: 'Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex.',
        action: 'wait',
        retryAt: '2026-09-26T01:21:00Z',
      },
    })
    // Session-only: a reload never brings a wait back.
    const [stored] = turnsToTranscript([assistantTurn({ status: 'error', failureAction: 'reconnect', retryAt: '2026-09-26T01:21:00Z' })])
    expect(stored).not.toHaveProperty('failureAction')
    expect(stored).not.toHaveProperty('retryAt')
    expect(transcriptTurnToChatTurn(stored!)).not.toHaveProperty('failureAction')
  })

  it('a waiting turn with nothing yet is pending, not empty content', () => {
    const nodes = deriveConversationNodes(assistantTurn({ status: 'waiting', content: '' }))
    expect(nodes.find((node) => node.kind === 'answer')).toEqual({
      kind: 'answer',
      // The elapsed-wait counter is anchored to the turn, not to the indicator's
      // mount: the transcript's `<For>` rebuilds every node on every derivation,
      // so a clock owned by the component restarted on each streamed tool call.
      answer: { state: 'pending', since: '2026-08-22T10:00:00.000Z' },
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
    for (const suppressed of ['confidence', 'memory-recall', 'truncated']) {
      expect(shown).not.toContain(suppressed)
    }

    const settled = assistantTurn({
      confidence: 0.1,
      memoryRecallCount: 3,
      stopReason: 'max_tokens',
      followUps: ['og videre?'],
    })
    for (const shownAfter of ['confidence', 'memory-recall', 'truncated']) {
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
      'confidence',
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
 * plan-gate §2.5: the `plan-approval` node derives for every FINISHED
 * plan-mode turn, exactly as it did before plan-gate existed — a proposed
 * plan (once the model's `plan.propose` call completes) is layered on top of
 * that same node, never a precondition for it. Requiring `proposedPlan`
 * would strand the run in plan mode with no way to grant or reject any
 * authority whenever the model never calls the tool (it is instructed to,
 * never guaranteed to) or the browser reloads before the ephemeral
 * `proposedPlan` field — never persisted into thread history — survives the
 * round trip; either way the turn is genuinely finished and needs an exit.
 */
describe('plan-approval node', () => {
  it('derives the rung-only node for a finished plan-mode turn with no proposed plan', () => {
    // Covers both a model that never called `plan.propose` and a reload that
    // lost the ephemeral field — in both cases `PlanApprovalControl` itself
    // degrades to the rung-only control (its own `hasStructuredPlan()` is
    // `Boolean(props.planId)`), which is the whole point: the node must
    // still exist so the run is never stuck in plan mode with no way out.
    const turn = assistantTurn({ planMode: true, streaming: false })
    const node = deriveConversationNodes(turn).find((n) => n.kind === 'plan-approval')
    expect(node).toEqual({
      kind: 'plan-approval',
      planId: undefined,
      summary: undefined,
      steps: undefined,
      grantedRung: undefined,
      pending: false,
      error: undefined,
    })
  })

  it('derives nothing while the plan-mode turn is still streaming, even with a proposed plan', () => {
    const turn = assistantTurn({
      planMode: true,
      streaming: true,
      proposedPlan: {
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      },
    })
    expect(kinds(turn)).not.toContain('plan-approval')
  })

  it('derives a plan-approval node carrying the proposed steps once finished', () => {
    const turn = assistantTurn({
      planMode: true,
      streaming: false,
      proposedPlan: {
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [
          { id: 'step_1', title: 'Draft the email' },
          { id: 'step_2', title: 'Send it', operation: 'email_send' },
        ],
      },
    })
    const node = deriveConversationNodes(turn).find((n) => n.kind === 'plan-approval')
    expect(node).toEqual({
      kind: 'plan-approval',
      planId: 'plan_1',
      summary: 'Send the overdue invoice',
      steps: [
        { id: 'step_1', title: 'Draft the email' },
        { id: 'step_2', title: 'Send it', operation: 'email_send' },
      ],
      grantedRung: undefined,
      pending: false,
      error: undefined,
    })
  })

  it('carries the granted rung once the plan has been approved', () => {
    const turn = assistantTurn({
      planMode: true,
      streaming: false,
      grantedRung: 'workspace_write',
      proposedPlan: {
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      },
    })
    const node = deriveConversationNodes(turn).find((n) => n.kind === 'plan-approval')
    expect(node && 'grantedRung' in node ? node.grantedRung : undefined).toBe('workspace_write')
  })

  it('carries a rejection so the card can say the plan was rejected', () => {
    const turn = assistantTurn({
      planMode: true,
      streaming: false,
      planRejected: true,
      proposedPlan: {
        planId: 'plan_1',
        summary: 'Send the overdue invoice',
        steps: [{ id: 'step_1', title: 'Draft the email' }],
      },
    })
    const node = deriveConversationNodes(turn).find((n) => n.kind === 'plan-approval')
    expect(node && 'rejected' in node ? node.rejected : undefined).toBe(true)
    // And a plan nobody rejected derives no `rejected` key at all.
    const open = deriveConversationNodes(
      assistantTurn({ planMode: true, streaming: false }),
    ).find((n) => n.kind === 'plan-approval')
    expect(open && 'rejected' in open).toBe(false)
  })

  it('is absent entirely for an ordinary (non-plan-mode) turn', () => {
    const turn = assistantTurn({
      proposedPlan: {
        planId: 'plan_1',
        summary: 'Should never render without planMode',
        steps: [{ id: 'step_1', title: 'x' }],
      },
    })
    expect(kinds(turn)).not.toContain('plan-approval')
  })
})

/** chat-wave2 scope spec §2.2: the per-turn retrieval-source filter receipt. */
describe('source-scope node', () => {
  it('is absent for an ordinary (auto) turn', () => {
    expect(kinds(assistantTurn())).not.toContain('source-scope')
    expect(kinds(assistantTurn({ retrievalScope: 'auto' }))).not.toContain('source-scope')
  })

  it("prefers the server's own confirmation (scopeApplied) over the client-sent request", () => {
    const node = deriveConversationNodes(assistantTurn({
      retrievalScope: 'knowledge_only',
      scopeApplied: { scope: 'web_only', excludedTools: ['knowledge_search'] },
    })).find((entry) => entry.kind === 'source-scope')
    expect(node).toEqual({ kind: 'source-scope', scope: 'web_only', excludedTools: ['knowledge_search'] })
  })

  it('falls back to the client-sent retrievalScope before the receipt frame arrives', () => {
    const node = deriveConversationNodes(assistantTurn({ retrievalScope: 'web_only' }))
      .find((entry) => entry.kind === 'source-scope')
    expect(node).toEqual({ kind: 'source-scope', scope: 'web_only', excludedTools: [] })
  })

  it('is suppressed mid-stream, like confidence/memory-recall/truncated', () => {
    const streaming = assistantTurn({
      status: 'waiting',
      retrievalScope: 'knowledge_only',
      scopeApplied: { scope: 'knowledge_only', excludedTools: ['web_search'] },
    })
    expect(kinds(streaming)).not.toContain('source-scope')
  })
})

/**
 * RUN-LOG finding 12. The confidence notice hedges on every scored answer, but
 * "ingen kilder ble brukt" is a second, separate claim — and it appeared under
 * answers whose numbers a code-interpreter run had just produced, contradicting
 * the Arbeid panel one click away. `hasEvidence` decides whether there is
 * anything for a reader to go and check; the hedge itself stays ungated.
 */
describe('confidence hasEvidence', () => {
  const toolCall = (overrides: Partial<{ id: string; name: string; status: string; error: string }> = {}) =>
    ({ id: 'tc1', name: 'run_python', ...overrides }) as NonNullable<ChatTurn['toolCalls']>[number]
  const evidence = (overrides: Partial<ChatTurn>) => {
    const node = deriveConversationNodes(assistantTurn({ confidence: 0.3, ...overrides }))
      .find((candidate) => candidate.kind === 'confidence')
    expect(node, 'a scored, settled turn must emit a confidence node').toBeDefined()
    return (node as Extract<typeof node, { kind: 'confidence' }>).hasEvidence
  }

  it('reports nothing to check when the turn produced no citations, grounding or tools', () => {
    expect(evidence({})).toBe(false)
  })

  it('counts a citation', () => {
    expect(evidence({
      citations: [{ id: 'c1', title: 'Mattilsynet', url: 'https://mattilsynet.no', snippet: 'Regelverk.' }],
    })).toBe(true)
  })

  it('counts internal knowledge grounding', () => {
    expect(evidence({ grounding: { sources: [] } as unknown as ChatTurn['grounding'] })).toBe(true)
  })

  it('counts a tool call that succeeded', () => {
    // The finding itself: the sandbox printed the figures the answer quotes.
    expect(evidence({ toolCalls: [toolCall({ status: 'completed' })] })).toBe(true)
    // No status at all is not a failure — older frames carry none.
    expect(evidence({ toolCalls: [toolCall()] })).toBe(true)
  })

  it('does not count a tool call that failed', () => {
    // A tool that errored verified nothing, so telling the reader to go and
    // check would point them at the failure rather than at evidence.
    expect(evidence({ toolCalls: [toolCall({ error: 'permission denied' })] })).toBe(false)
    expect(evidence({ toolCalls: [toolCall({ status: 'error' })] })).toBe(false)
    expect(evidence({ toolCalls: [toolCall({ status: 'failed' })] })).toBe(false)
  })

  it('counts a run where one tool failed and another did not', () => {
    expect(evidence({
      toolCalls: [toolCall({ status: 'failed' }), toolCall({ id: 'tc2', status: 'completed' })],
    })).toBe(true)
  })

  it('treats an empty citation list as no evidence rather than as a list', () => {
    expect(evidence({ citations: [] })).toBe(false)
    expect(evidence({ toolCalls: [] })).toBe(false)
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
      // No `confidence` here on purpose — see the test below. It is a
      // usage-derived field the server's message shape cannot carry, so
      // including it would assert an equivalence that is not achievable and
      // hide the two paths that DO agree.
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

  /**
   * The score reaches the renderer by all three routes.
   *
   * It arrives on the `usage` SSE event, so it used to live only in the
   * browser that watched the turn stream and in the local transcript: a thread
   * opened on another device replayed the answer with no score at all, and
   * "unscored" is indistinguishable from "nothing was ever checked". The
   * backend now persists it in the turn's metadata, which the messages
   * endpoint flattens onto the message — so a cold load carries it too.
   *
   * A row written before that persistence still has none, and must stay
   * distinguishable from a score of zero.
   */
  it('confidence reaches the renderer from a rehydrate and from a cold load', () => {
    const scored = assistantTurn({ content: 'Oslo.', confidence: 0.88 })
    const [persisted] = turnsToTranscript([scored])
    const rehydrated = transcriptTurnToChatTurn(persisted!)
    expect(deriveConversationNodes(rehydrated).map((n) => n.kind)).toContain('confidence')

    const coldLoad = messageToTurn({
      id: 'a1',
      role: 'assistant',
      content: scored.content,
      createdAt: scored.createdAt,
      confidence: 0.88,
      verification: {
        verdict: 'unrelated',
        kbCitations: 0,
        webCitations: 0,
        webAllowed: false,
      },
    } as unknown as Parameters<typeof messageToTurn>[0])
    expect(coldLoad.confidence).toBe(0.88)
    const coldNodes = deriveConversationNodes(coldLoad)
    expect(coldNodes.map((n) => n.kind)).toContain('confidence')
    // ...and it carries what verification found, so the renderer can say
    // "checked and found nothing" rather than implying nobody looked.
    const node = coldNodes.find((n) => n.kind === 'confidence')
    expect(node).toMatchObject({ verification: { verdict: 'unrelated' } })

    // A pre-persistence row has no score and derives no node.
    const legacy = messageToTurn({
      id: 'a0',
      role: 'assistant',
      content: scored.content,
      createdAt: scored.createdAt,
    } as unknown as Parameters<typeof messageToTurn>[0])
    expect(legacy.confidence).toBeUndefined()
    expect(deriveConversationNodes(legacy).map((n) => n.kind)).not.toContain('confidence')
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
    // Which memory, never its text: a memory forgotten or corrected after this
    // turn must not come back from the stored copy (the chip reads the current
    // text when it is opened).
    const withoutText = recalled.map((memory) => ({ ...memory, preview: '' }))
    expect(rehydrated.recalledMemories).toEqual(withoutText)
    expect(JSON.stringify(persisted)).not.toContain('Prefers metric units')

    const node = deriveConversationNodes(rehydrated).find((entry) => entry.kind === 'memory-recall')
    expect(node).toMatchObject({ count: 1, memories: withoutText })
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
