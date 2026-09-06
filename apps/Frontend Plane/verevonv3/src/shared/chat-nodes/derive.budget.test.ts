/**
 * The streaming budget: definition-of-finished point 11, made falsifiable.
 *
 * The implementation plan says "Solid's fine-grained rendering meets
 * performance targets under heavy streaming", and phase 0's exit gate asks for
 * reproducible baseline measurements. Neither existed: there was no target, no
 * stress profile, and no measurement anywhere in the chat feature, so point 11
 * could only ever be asserted. The profile is now written down in
 * VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md section 21; this file measures
 * the hot path against it.
 *
 * WHY THIS FUNCTION. `ChatMessages` wraps `deriveConversationNodes` in a
 * `createMemo` per message. Only the streaming turn's memo invalidates on a
 * delta, but it invalidates on EVERY delta — so this one function runs at the
 * token rate, over a turn that keeps growing, and it rebuilds the whole node
 * list each time (image previews, filters, ordering). It is the one place in
 * the chat path where per-token work is proportional to accumulated turn size,
 * which is the shape that turns a long answer into a stalled tab.
 *
 * HOW THE ASSERTIONS ARE BUILT. Wall-clock budgets in unit tests are famously
 * flaky, so the absolute budget here is deliberately loose — roughly an order
 * of magnitude above what this path costs on a developer machine. It is not
 * tuned to catch a 20% slowdown, and it should not be: it exists to catch an
 * ALGORITHMIC regression, the accidental O(n^2) that makes a 4x longer answer
 * 16x more expensive. The scaling test is the real guard, because a ratio
 * survives a slow CI box where an absolute millisecond count does not.
 */

import { describe, expect, it } from 'vitest'
import { deriveConversationNodes } from './derive'
import type { ChatTurn } from './types'

/**
 * The stress profile's "heavy" turn, from section 21: a long grounded answer
 * with tool calls, citations, artifacts and files all present at once. Every
 * optional branch in the derivation is populated, so nothing is measured on a
 * fast path a real heavy turn would not take.
 */
function heavyTurn(paragraphs: number): ChatTurn {
  const paragraph =
    'Omsetningen i tredje kvartal endte på 4,2 millioner kroner, opp 12 prosent ' +
    'fra samme kvartal i fjor. Veksten kom hovedsakelig fra nye kunder i ' +
    'offentlig sektor, og marginen holdt seg stabil.\n\n'
  return {
    id: 'stress-turn',
    role: 'assistant',
    content: paragraph.repeat(paragraphs),
    createdAt: '2026-09-06T09:00:00.000Z',
    streaming: true,
    status: 'waiting',
    tools: [],
    attachments: [],
    reasoning: 'Vurderer kildene mot spørsmålet.',
    confidence: 0.82,
    citations: Array.from({ length: 12 }, (_, index) => ({
      id: `c${index}`,
      title: `Kvartalsrapport ${index}`,
      url: `https://example.invalid/rapport-${index}`,
      snippet: 'Omsetning og margin per segment.',
    })),
    toolCalls: Array.from({ length: 8 }, (_, index) => ({
      id: `t${index}`,
      name: 'knowledge_search',
      status: 'done' as const,
      arguments: '{"query":"omsetning q3"}',
      result: 'Fant 4 dokumenter.',
    })),
    artifacts: Array.from({ length: 4 }, (_, index) => ({
      id: `a${index}`,
      kind: 'markdown',
      title: `Sammendrag ${index}`,
      content: '## Sammendrag\n\nOmsetning opp 12 prosent.',
      version: 1,
    })),
    files: Array.from({ length: 4 }, (_, index) => ({
      id: `f${index}`,
      name: `vedlegg-${index}.txt`,
      mime: 'text/plain',
      size: 2048,
      url: 'data:text/plain;base64,T21zZXRuaW5n',
    })),
  } as ChatTurn
}

/**
 * Median of `runs` timings, not the mean: one scheduler hiccup or GC pause
 * skews a mean badly at these durations, and the median is what a reader would
 * call "how long it takes".
 */
function medianMs(work: () => void, runs: number): number {
  const samples: number[] = []
  for (let i = 0; i < runs; i += 1) {
    const started = performance.now()
    work()
    samples.push(performance.now() - started)
  }
  samples.sort((left, right) => left - right)
  return samples[Math.floor(samples.length / 2)] ?? 0
}

/**
 * Section 21's tick rate: 40 deltas per second is a fast provider stream, and
 * the memo recomputes once per delta. One second of that is the unit of work
 * the budget is expressed in.
 */
const TICKS_PER_SECOND = 40

/**
 * 200ms of CPU for a second of streaming — a 20% duty cycle on one turn,
 * leaving the rest of the frame budget to Solid's own rendering. Loose on
 * purpose: see the header.
 */
const BUDGET_MS_PER_STREAMED_SECOND = 200

/**
 * Phase 0's exit gate asks for "reproducible baseline measurements", so the
 * measurements are printed rather than only compared. A budget test that
 * reports nothing tells the next reader whether it passed, not whether the
 * headroom is still there — and headroom quietly disappearing is the thing
 * worth noticing before it becomes a failure.
 */
function report(label: string, ms: number, budget: number): void {
  const headroom = budget / Math.max(ms, 0.001)
  // eslint-disable-next-line no-console
  console.log(
    `[streaming budget] ${label}: ${ms.toFixed(2)}ms (budget ${budget}ms, ${headroom.toFixed(0)}x headroom)`,
  )
}

describe('streaming budget: deriveConversationNodes', () => {
  it('sustains a second of fast streaming on a heavy turn well inside the budget', () => {
    const turn = heavyTurn(20)
    const elapsed = medianMs(() => {
      for (let tick = 0; tick < TICKS_PER_SECOND; tick += 1) {
        deriveConversationNodes(turn)
      }
    }, 7)
    report(`${TICKS_PER_SECOND} ticks on a heavy turn`, elapsed, BUDGET_MS_PER_STREAMED_SECOND)
    expect(elapsed).toBeLessThan(BUDGET_MS_PER_STREAMED_SECOND)
  })

  /**
   * The assertion that actually matters. A quadratic path — rescanning the
   * accumulated answer per citation, say, or rebuilding previews from the full
   * content on every file — shows up here and nowhere else. 4x the answer
   * length may cost at most 8x the time; anything worse is superlinear enough
   * to matter on a long research answer, and the headroom absorbs noise.
   */
  it('costs no more than linearly in answer length', () => {
    const small = heavyTurn(10)
    const large = heavyTurn(40)
    // One derivation measures 0.01ms, which is under `performance.now()`'s
    // useful resolution here — the first version of this test divided two
    // sub-resolution numbers and silently skipped the assertion it exists
    // for. Each sample is a BATCH, so both sides land well above the noise
    // floor and the ratio means something.
    const batch = 200
    const runs = 9
    const measure = (turn: ChatTurn) => medianMs(() => {
      for (let i = 0; i < batch; i += 1) deriveConversationNodes(turn)
    }, runs)
    const smallMs = measure(small)
    const largeMs = measure(large)

    const ratio = largeMs / smallMs
    report(`4x answer length over ${batch} derivations costs ${ratio.toFixed(1)}x`, largeMs, BUDGET_MS_PER_STREAMED_SECOND)
    // A batch this size must still be measurable, or the ratio is noise again.
    expect(smallMs).toBeGreaterThan(0.05)
    expect(ratio).toBeLessThan(8)
  })

  /**
   * A transcript's non-streaming turns each hold their own memo and must not be
   * re-derived when a new turn streams. This measures the reload case instead:
   * deriving a whole long thread once, as a mount does.
   */
  it('derives a 200-turn thread on mount within the budget', () => {
    const turns = Array.from({ length: 200 }, () => heavyTurn(4))
    const elapsed = medianMs(() => {
      for (const turn of turns) deriveConversationNodes(turn)
    }, 5)
    // 200 turns is the long-thread figure in section 21; a mount is allowed
    // five streamed seconds' worth of work because it happens once.
    report('200-turn thread mount', elapsed, BUDGET_MS_PER_STREAMED_SECOND * 5)
    expect(elapsed).toBeLessThan(BUDGET_MS_PER_STREAMED_SECOND * 5)
  })

  /**
   * A budget test that silently stopped exercising the heavy path would pass
   * forever. Pin the fixture's shape, so deleting a branch from the derivation
   * fails here instead of quietly making the measurement meaningless.
   */
  it('measures a turn that really does take every branch', () => {
    const kinds = deriveConversationNodes(heavyTurn(2)).map((node) => node.kind)
    expect(kinds).toContain('reasoning')
    expect(kinds).toContain('answer')
    expect(kinds).toContain('tool-chips')
    expect(new Set(kinds).size).toBeGreaterThan(3)
  })
})
