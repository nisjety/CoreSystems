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

const SENT = '2026-11-07T10:00:00.000Z'
const FOUR_MIN_12_S_LATER = '2026-11-07T10:04:12.000Z'

const assistant = (id: string, overrides: Partial<FoldTurn> = {}): FoldTurn => ({
  id,
  role: 'assistant',
  createdAt: SENT,
  settledAt: undefined,
  streaming: false,
  status: undefined,
  reasoning: undefined,
  toolCalls: undefined,
  files: undefined,
  pendingApprovals: undefined,
  planMode: undefined,
  grantedRung: undefined,
  planRejected: undefined,
  ...overrides,
})

const user = (id: string, createdAt = SENT): FoldTurn => ({ ...assistant(id, { createdAt }), role: 'user' })

const calls = (count: number) =>
  Array.from({ length: count }, (_, index) => ({ id: `call_${index}`, name: 'web_search' }))

const file = (name: string, mime: string) => ({
  id: name,
  name,
  mime,
  size: 1,
  url: `https://files.example.test/${name}`,
})

const APPROVAL = [{ id: 'approval_1', kind: 'APPROVAL_KIND_TOOL_CALL' }] as unknown as FoldTurn['pendingApprovals']

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
    expect(isSettled(assistant('a'), { activeTurnId: 'a' })).toBe(false)
    expect(isSettled(assistant('a'), { activeTurnId: 'b' })).toBe(true)
  })

  it('never settles the turn of a parked run, which may resume', () => {
    expect(isSettled(assistant('a'), { parkedTurnId: 'a' })).toBe(false)
    expect(isSettled(assistant('a'), { parkedTurnId: 'b' })).toBe(true)
  })

  it('never settles a user turn', () => {
    expect(isSettled(user('u'))).toBe(false)
  })

  it('keeps a turn unsettled while an approval it waits on is open, and settles it once decided', () => {
    expect(isSettled(assistant('a', { pendingApprovals: APPROVAL }))).toBe(false)
    expect(isSettled(assistant('a', { pendingApprovals: [] }))).toBe(true)
  })

  it('keeps a plan turn unsettled until the plan is granted or rejected', () => {
    expect(isSettled(assistant('a', { planMode: true }))).toBe(false)
    expect(isSettled(assistant('a', { planMode: true, grantedRung: 'workspace_write' }))).toBe(true)
    expect(isSettled(assistant('a', { planMode: true, planRejected: true }))).toBe(true)
  })

  it('settles a turn that was not run in plan mode, whatever its plan fields say', () => {
    expect(isSettled(assistant('a', { planMode: false }))).toBe(true)
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

  it('never gives a row to the turn a run is still on, or to a parked turn', () => {
    expect(foldPlan([assistant('a', { toolCalls: calls(1) })], { activeTurnId: 'a' }).size).toBe(0)
    expect(foldPlan([assistant('a', { toolCalls: calls(1) })], { parkedTurnId: 'a' }).size).toBe(0)
  })

  it('gives no row to a streaming or waiting turn', () => {
    const plan = foldPlan([
      assistant('a', { toolCalls: calls(1), streaming: true }),
      assistant('b', { toolCalls: calls(1), status: 'waiting' }),
    ])
    expect(plan.size).toBe(0)
  })

  it('gives no row to a turn that holds an approval or an ungranted plan, so its decision stays whole', () => {
    const plan = foldPlan([
      assistant('a', { toolCalls: calls(2), pendingApprovals: APPROVAL }),
      assistant('b', { toolCalls: calls(2), planMode: true }),
    ])
    expect(plan.size).toBe(0)
  })

  it('gives no row to a turn that errored, so its trail and its error stay whole', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(3), status: 'error' })])
    expect(plan.size).toBe(0)
  })

  it('still gives a row to a turn the person stopped, whose trail is the work done before the stop', () => {
    const plan = foldPlan([assistant('a', { toolCalls: calls(2), status: 'stopped' })])
    expect(plan.get('a')?.steps).toBe(2)
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

  describe('duration: from the triggering user message to the terminal frame', () => {
    it('is the span from the user message to the turn\'s terminal frame', () => {
      const plan = foldPlan([
        user('u1'),
        assistant('a', { toolCalls: calls(1), settledAt: FOUR_MIN_12_S_LATER }),
      ])
      expect(plan.get('a')?.durationMs).toBe(252_000)
    })

    it('gives no duration to a restored turn, which has no terminal frame on this client', () => {
      const plan = foldPlan([user('u1'), assistant('a', { toolCalls: calls(2) })])
      expect(plan.get('a')?.durationMs).toBeUndefined()
      expect(plan.get('a')?.steps).toBe(2)
    })

    it('gives no duration when the turn has no triggering user message in view', () => {
      const plan = foldPlan([assistant('a', { toolCalls: calls(1), settledAt: FOUR_MIN_12_S_LATER })])
      expect(plan.get('a')?.durationMs).toBeUndefined()
    })

    it('counts from the nearest user message before the answer', () => {
      const plan = foldPlan([
        user('u1', '2026-11-07T10:00:00.000Z'),
        assistant('a1', { toolCalls: calls(1), settledAt: '2026-11-07T10:01:00.000Z' }),
        user('u2', '2026-11-07T10:05:00.000Z'),
        assistant('a2', { toolCalls: calls(1), settledAt: '2026-11-07T10:06:30.000Z' }),
      ])
      expect(plan.get('a1')?.durationMs).toBe(60_000)
      expect(plan.get('a2')?.durationMs).toBe(90_000)
    })

    it('gives no duration for a terminal frame that is earlier than its message, rather than a negative span', () => {
      const plan = foldPlan([
        user('u1', '2026-11-07T10:00:00.000Z'),
        assistant('a', { toolCalls: calls(1), settledAt: '2026-11-07T09:59:00.000Z' }),
      ])
      expect(plan.get('a')?.durationMs).toBeUndefined()
    })

    it('gives no duration for a time that does not parse', () => {
      const plan = foldPlan([
        user('u1', 'not-a-time'),
        assistant('a', { toolCalls: calls(1), settledAt: FOUR_MIN_12_S_LATER }),
        user('u2'),
        assistant('b', { toolCalls: calls(1), settledAt: 'not-a-time' }),
      ])
      expect(plan.get('a')?.durationMs).toBeUndefined()
      expect(plan.get('b')?.durationMs).toBeUndefined()
    })
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

  it('keeps the last settled turn open while a later turn is still unsettled', () => {
    const plan = foldPlan([
      assistant('a1', { toolCalls: calls(1) }),
      user('u1'),
      assistant('a2', { toolCalls: calls(1), pendingApprovals: APPROVAL }),
      user('u2'),
    ])
    expect(plan.has('a2')).toBe(false)
    expect(plan.get('a1')?.defaultFolded).toBe(false)
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
