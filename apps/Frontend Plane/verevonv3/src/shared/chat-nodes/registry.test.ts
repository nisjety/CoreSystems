import { describe, expect, it, vi } from 'vitest'
import { createConversationNodeRegistry } from './registry'
import type { ConversationNode, ConversationNodeContext, ConversationNodeKind } from './types'

const ctx: ConversationNodeContext = {
  onViewSteps: () => {},
  onApprovalDecision: () => {},
  onApprovePlan: () => {},
  onRegenerate: () => {},
  threadId: 'thread-1',
}

/** Every kind the union declares — the set a real registry must cover. */
const ALL_KINDS: ConversationNodeKind[] = [
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
  'plan-approval',
  'image-previews',
  'files',
  'artifacts',
  'follow-ups',
]

function stubRegistry(kinds: ConversationNodeKind[]) {
  const definitions = Object.fromEntries(
    kinds.map((kind) => [kind, { kind, render: () => `rendered:${kind}` }]),
  )
  // The stub is intentionally partial in one test, which the exhaustive type
  // forbids — hence the cast, confined to the test helper.
  return createConversationNodeRegistry(
    definitions as unknown as Parameters<typeof createConversationNodeRegistry>[0],
  )
}

describe('createConversationNodeRegistry', () => {
  it('dispatches each node to its own renderer', () => {
    const registry = stubRegistry(ALL_KINDS)
    expect(registry.render({ kind: 'memory-recall', count: 2, memories: [] }, ctx)).toBe('rendered:memory-recall')
    expect(registry.render({ kind: 'truncated', stopReason: 'max_tokens' }, ctx)).toBe(
      'rendered:truncated',
    )
  })

  it('reports the kinds it covers', () => {
    expect(stubRegistry(ALL_KINDS).kinds().sort()).toEqual([...ALL_KINDS].sort())
  })

  /**
   * A missing renderer must be LOUD. Silently rendering nothing makes "someone
   * forgot a renderer" indistinguishable from "this turn had no such content" —
   * the same failure the SSE client's `onUnknownEvent` exists to prevent.
   */
  it('warns once per unregistered kind instead of dropping it silently', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const registry = stubRegistry(ALL_KINDS.filter((kind) => kind !== 'truncated'))
    const node: ConversationNode = { kind: 'truncated', stopReason: 'max_tokens' }

    expect(registry.render(node, ctx)).toBeNull()
    expect(warn).toHaveBeenCalledTimes(1)
    expect(warn.mock.calls[0]?.[0]).toContain('truncated')

    // Repeated renders must not flood the console — a render loop would.
    registry.render(node, ctx)
    registry.render(node, ctx)
    expect(warn).toHaveBeenCalledTimes(1)
    warn.mockRestore()
  })
})
