import { describe, expect, it } from 'vitest'
import {
  foldPlan,
  isSettled,
  isSettledFoldEnabled,
  settledFoldSummary,
  splitForFold,
  type FoldTurn,
} from './settled-fold'
import type { ConversationNodeKind } from '@/shared/chat-nodes/types'

const assistant = (id: string, overrides: Partial<FoldTurn> = {}): FoldTurn => ({
  id,
  role: 'assistant',
  streaming: false,
  status: undefined,
  reasoning: undefined,
  toolCalls: undefined,
  files: undefined,
  timing: undefined,
  ...overrides,
})

const user = (id: string): FoldTurn => ({ ...assistant(id), role: 'user' })

const calls = (count: number) =>
  Array.from({ length: count }, (_, index) => ({ id: `call_${index}`, name: 'web_search' }))

const file = (name: string, mime: string) => ({
  id: name,
  name,
  mime,
  size: 1,
  url: `https://files.example.test/${name}`,
})

const SPAN = { serverMs: 252_000, providerMs: 200_000, orchestrationMs: 52_000 }

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

describe('isSettledFoldEnabled', () => {
  it('enables only on the literal true, as the other chat switches do', () => {
    expect(isSettledFoldEnabled('true')).toBe(true)
    for (const raw of ['TRUE', '1', 'yes', '', undefined]) {
      expect(isSettledFoldEnabled(raw)).toBe(false)
    }
  })
})

describe('isSettled', () => {
  it('settles a finished, errored or stopped assistant turn', () => {
    expect(isSettled(assistant('a'))).toBe(true)
    expect(isSettled(assistant('a', { status: 'error' }))).toBe(true)
    expect(isSettled(assistant('a', { status: 'stopped' }))).toBe(true)
  })

  it('does not settle a streaming or waiting turn', () => {
    expect(isSettled(assistant('a', { streaming: true }))).toBe(false)
    expect(isSettled(assistant('a', { status: 'waiting' }))).toBe(false)
  })

  it('never settles the turn a run is still on', () => {
    expect(isSettled(assistant('a'), 'a')).toBe(false)
    expect(isSettled(assistant('a'), 'b')).toBe(true)
  })

  it('never settles a user turn', () => {
    expect(isSettled(user('u'))).toBe(false)
  })
})

describe('foldPlan', () => {
  it('gives a settled turn with tool steps a row with its step count', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(3) })])
    expect(plan.get('a')).toEqual({ defaultFolded: false, steps: 3, files: 0 })
  })

  it('gives a settled turn with reasoning and no tool step a row', () => {
    const plan = foldPlan([assistant('a', { reasoning: 'Sjekker tallene først.' })])
    expect(plan.get('a')).toEqual({ defaultFolded: false, steps: 0, files: 0 })
  })

  it('gives no row to a settled turn that ran nothing to fold', () => {
    expect(foldPlan([assistant('a')]).has('a')).toBe(false)
  })

  it('never gives a row to the turn a run is still on', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(1) })], { activeTurnId: 'a' })
    expect(plan.size).toBe(0)
  })

  it('gives no row to a streaming or waiting turn', () => {
    const plan = foldPlan([
      assistant('a', { toolCalls: calls(1), streaming: true }),
      assistant('b', { toolCalls: calls(1), status: 'waiting' }),
    ])
    expect(plan.size).toBe(0)
  })

  it('leaves generated images out of the file count', () => {
    const plan = foldPlan([
      assistant('a', {
        toolCalls: calls(1),
        files: [file('chart.png', 'image/png'), file('report.pdf', 'application/pdf')],
      }),
    ])
    expect(plan.get('a')?.files).toBe(1)
  })

  it('takes the duration from the gateway span the turn reported', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(1), timing: SPAN })])
    expect(plan.get('a')?.durationMs).toBe(252_000)
  })

  it('keeps the duration on a restored turn, which carries only the timing totals', () => {
    const restored = assistant('a', { toolCalls: calls(2), timing: { ...SPAN } })
    expect(foldPlan([restored]).get('a')?.durationMs).toBe(252_000)
  })

  it('gives no duration when the turn reported none, rather than estimating one', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(4) })])
    expect(plan.get('a')).toEqual({ defaultFolded: false, steps: 4, files: 0 })
    expect(plan.get('a')?.durationMs).toBeUndefined()
  })

  it('gives no duration for a span that is not a finite, non-negative number', () => {
    const invalid = [Number.NaN, -1, Number.POSITIVE_INFINITY]
    for (const serverMs of invalid) {
      const plan = foldPlan([assistant('a', { toolCalls: calls(1), timing: { ...SPAN, serverMs } })])
      expect(plan.get('a')?.durationMs).toBeUndefined()
    }
  })

  it('folds the settled turns older than the last two, and leaves the last settled turn open', () => {
    const plan = foldPlan([
      assistant('a1', { toolCalls: calls(1) }),
      user('u1'),
      assistant('a2', { toolCalls: calls(1) }),
      user('u2'),
      assistant('a3', { toolCalls: calls(1) }),
      user('u3'),
    ])
    expect(plan.get('a1')?.defaultFolded).toBe(true)
    expect(plan.get('a2')?.defaultFolded).toBe(true)
    expect(plan.get('a3')?.defaultFolded).toBe(false)
  })

  it('keeps the last settled turn open even when it is older than the last two turns', () => {
    const plan = foldPlan([
      assistant('a1', { toolCalls: calls(1) }),
      user('u1'),
      assistant('a2', { toolCalls: calls(1) }),
      user('u2'),
      user('u3'),
    ])
    expect(plan.get('a1')?.defaultFolded).toBe(true)
    expect(plan.get('a2')?.defaultFolded).toBe(false)
  })
})

describe('splitForFold', () => {
  it('folds only the process trail, and every other kind stays in view', () => {
    const folded = splitForFold(ALL_KINDS.map((kind) => ({ node: { kind } }))).folded.map((entry) => entry.node.kind)
    expect(folded).toEqual(['reasoning', 'tool-chips', 'steps'])
  })

  it('keeps the answer, decisions, receipts, notices and deliverables in view', () => {
    const { visible } = splitForFold(ALL_KINDS.map((kind) => ({ node: { kind } })))
    const kept = visible.map((entry) => entry.node.kind)
    for (const kind of [
      'answer',
      'approvals',
      'plan-approval',
      'source-scope',
      'lifecycle-notices',
      'truncated',
      'files',
      'artifacts',
      'image-previews',
    ] as const) {
      expect(kept).toContain(kind)
    }
  })

  it('keeps the order each side was derived in', () => {
    const entries = [
      { node: { kind: 'reasoning' as const } },
      { node: { kind: 'answer' as const } },
      { node: { kind: 'steps' as const } },
      { node: { kind: 'tool-chips' as const } },
      { node: { kind: 'files' as const } },
    ]
    const { folded, visible } = splitForFold(entries)
    expect(folded.map((entry) => entry.node.kind)).toEqual(['reasoning', 'steps', 'tool-chips'])
    expect(visible.map((entry) => entry.node.kind)).toEqual(['answer', 'files'])
  })
})

describe('settledFoldSummary', () => {
  const en = { tr: (_noText: string, enText: string) => enText }
  const no = { tr: (noText: string) => noText }

  it('names the time, the steps and the files in English', () => {
    expect(settledFoldSummary({ defaultFolded: true, steps: 9, files: 2, durationMs: 252_000 }, en)).toBe(
      'Worked for 4 min 12 s · 9 steps · 2 files',
    )
  })

  it('names the same things in Norwegian', () => {
    expect(settledFoldSummary({ defaultFolded: true, steps: 9, files: 2, durationMs: 252_000 }, no)).toBe(
      'Arbeidet i 4 min 12 s · 9 steg · 2 filer',
    )
  })

  it('shows step counts only when the turn reported no time', () => {
    expect(settledFoldSummary({ defaultFolded: false, steps: 1, files: 0 }, en)).toBe('Worked · 1 step')
  })

  it('leaves out a zero count', () => {
    expect(settledFoldSummary({ defaultFolded: false, steps: 0, files: 0, durationMs: 3_000 }, en)).toBe(
      'Worked for 3 s',
    )
  })

  it('uses the singular for one file', () => {
    expect(settledFoldSummary({ defaultFolded: false, steps: 0, files: 1, durationMs: 3_000 }, en)).toBe(
      'Worked for 3 s · 1 file',
    )
  })
})
