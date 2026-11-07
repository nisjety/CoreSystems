// @vitest-environment jsdom

import { createMemo, createRoot, createSignal, flush } from 'solid-js'
import { describe, expect, it } from 'vitest'
import { createConversationNodeRegistry } from './registry'
import type {
  ConversationNode,
  ConversationNodeContext,
  ConversationNodeEntry,
  ConversationNodeKind,
} from './types'

const ctx = (overrides: Partial<ConversationNodeContext> = {}): ConversationNodeContext => ({
  onViewSteps: () => {},
  onApprovalDecision: () => {},
  onApprovePlan: () => {},
  onRegenerate: () => {},
  threadId: 'thread-1',
  ...overrides,
})

const ALL_KINDS: ConversationNodeKind[] = [
  'reasoning',
  'answer',
  'grounding',
  'confidence',
  'memory-recall',
  'source-scope',
  'search-provenance',
  'lifecycle-notices',
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

/** What a stub renderer was handed, so the test can read through it. */
type Seen = { node: ConversationNode; ctx: ConversationNodeContext }

/** A registry whose every renderer returns the views it was given. */
function recordingRegistry() {
  const definitions = Object.fromEntries(
    ALL_KINDS.map((kind) => [
      kind,
      { kind, render: (node: ConversationNode, context: ConversationNodeContext) => ({ node, ctx: context }) },
    ]),
  )
  return createConversationNodeRegistry(
    definitions as unknown as Parameters<typeof createConversationNodeRegistry>[0],
  )
}

const entry = (stopReason: string, context = ctx()): ConversationNodeEntry => ({
  node: { kind: 'truncated', stopReason },
  context,
})

/** Renders one live row inside an owner, the way the transcript's `<For>` does; writes happen outside it. */
function liveRow(initial: ConversationNodeEntry) {
  const [current, setCurrent] = createSignal(initial)
  let seen!: Seen
  let dispose!: () => void
  createRoot((disposeRoot) => {
    dispose = disposeRoot
    seen = recordingRegistry().renderLive(current) as unknown as Seen
  })
  return { seen, dispose, replace: (next: ConversationNodeEntry) => { setCurrent(next); flush() } }
}

describe('ConversationNodeRegistry.renderLive', () => {
  it('hands the renderer views that follow the current entry', () => {
    const row = liveRow(entry('max_tokens'))
    expect(row.seen.node).toMatchObject({ kind: 'truncated', stopReason: 'max_tokens' })

    // The entry object is replaced, as every re-derivation does; the SAME views read the new one.
    row.replace(entry('content_filter', ctx({ threadId: 'thread-2' })))
    expect(row.seen.node).toMatchObject({ kind: 'truncated', stopReason: 'content_filter' })
    expect(row.seen.ctx.threadId).toBe('thread-2')
    row.dispose()
  })

  it('tells a reader of one property only when that property changed', () => {
    const [current, setCurrent] = createSignal(entry('max_tokens'))
    let seen!: Seen
    let dispose!: () => void
    let threadReads = 0
    let reasonReads = 0
    let threadId!: () => string | null
    let reason!: () => string
    createRoot((disposeRoot) => {
      dispose = disposeRoot
      seen = recordingRegistry().renderLive(current) as unknown as Seen
      threadId = createMemo(() => {
        threadReads += 1
        return seen.ctx.threadId
      })
      reason = createMemo(() => {
        reasonReads += 1
        return (seen.node as Extract<ConversationNode, { kind: 'truncated' }>).stopReason
      })
    })
    expect([threadId(), reason()]).toEqual(['thread-1', 'max_tokens'])

    // A fresh entry object, new node object, new context object: one field really changed.
    setCurrent(entry('content_filter'))
    flush()
    expect([threadId(), reason()]).toEqual(['thread-1', 'content_filter'])
    // This is what keeps a component that reads `ctx.citations` from rebuilding per token.
    expect(threadReads).toBe(1)
    expect(reasonReads).toBe(2)
    dispose()
  })

  it('supports the reads a renderer or a spread makes of a plain object', () => {
    const row = liveRow(entry('max_tokens'))
    expect('stopReason' in row.seen.node).toBe(true)
    expect('nothing' in row.seen.node).toBe(false)
    expect({ ...row.seen.node }).toEqual({ kind: 'truncated', stopReason: 'max_tokens' })
    expect(Object.keys(row.seen.node).sort()).toEqual(['kind', 'stopReason'])
    row.dispose()
  })

  it('renders outside any owner, reading the entry directly', () => {
    const seen = recordingRegistry().renderLive(() => entry('max_tokens')) as unknown as Seen
    expect(seen.node).toMatchObject({ stopReason: 'max_tokens' })
    expect(seen.ctx.threadId).toBe('thread-1')
  })

  it('renders nothing for a kind with no renderer, as render does', () => {
    const registry = createConversationNodeRegistry({} as unknown as Parameters<typeof createConversationNodeRegistry>[0])
    expect(registry.renderLive(() => entry('max_tokens'))).toBeNull()
  })
})
