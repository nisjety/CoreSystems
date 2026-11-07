// @vitest-environment jsdom

import { render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunEventHandlers } from '@/shared/api/run-console-client'

const hoisted = vi.hoisted(() => ({
  subscriptions: [] as Array<{ handlers: Record<string, unknown>; runId: string }>,
  runDetail: null as { status: string } | null,
  /** Per-run detail for the children a reloaded panel reads (EP-2 S9); falls back to `runDetail`. */
  runDetails: {} as Record<string, { status: string }>,
  lineageEdges: [] as Array<{
    parentRunId: string
    childRunId: string
    goalSummary: string
    batchId: string
    status: string
  }>,
  lineageThreads: [] as string[],
}))

vi.mock('@/shared/api/run-console-client', () => ({
  streamRunEvents: (runId: string, handlers: Record<string, unknown>) => {
    hoisted.subscriptions.push({ handlers, runId })
    // Never settles: the real stream stays open for the life of the run.
    return new Promise<void>(() => {})
  },
}))

vi.mock('@/shared/api/runs-client', () => ({
  getRun: async (runId: string) => hoisted.runDetails[runId] ?? hoisted.runDetail,
}))

vi.mock('@/shared/api/orchestration-client', () => ({
  getLineageEdges: async (threadId: string) => {
    hoisted.lineageThreads.push(threadId)
    return hoisted.lineageEdges
  },
}))

import { ChatWideResearchPanel } from './ChatWideResearchPanel'

function handlers(index = hoisted.subscriptions.length - 1): RunEventHandlers {
  return hoisted.subscriptions[index]?.handlers as RunEventHandlers
}

async function tick(times = 1): Promise<void> {
  for (let step = 0; step < times; step += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
  flush()
}

beforeEach(() => {
  hoisted.subscriptions.length = 0
  hoisted.runDetail = null
  hoisted.runDetails = {}
  hoisted.lineageEdges = []
  hoisted.lineageThreads.length = 0
})

describe('ChatWideResearchPanel', () => {
  it('renders nothing while the run has no id, and never subscribes', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId={null} />)
    await tick(3)

    expect(container.querySelector('.verevon-wide-research')).toBeNull()
    expect(hoisted.subscriptions).toHaveLength(0)
  })

  it('renders one card per item once ≥2 SubagentAttachedEvents share a batchId', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'look up carrier docs',
      role: 'researcher',
    })
    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.1',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'look up pricing docs',
      role: 'researcher',
    })
    flush()
    await tick()

    const cards = container.querySelectorAll('.verevon-wide-research__card')
    expect(cards).toHaveLength(2)
    expect(container.textContent).toContain('look up carrier docs')
    expect(container.textContent).toContain('look up pricing docs')
    // Both start running.
    for (const card of cards) {
      expect(card.getAttribute('data-status')).toBe('running')
    }
  })

  it('does not switch in for a single item — a lone subagent.task delegation carries no batchId', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    // A standalone `subagent.task` attach never carries a batchId at all.
    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_sub-1',
      goalSummary: '',
      role: 'generic',
    })
    flush()
    await tick()

    expect(container.querySelector('.verevon-wide-research')).toBeNull()
  })

  it('does not switch in when only one item of a batch ever registers (the rest refused)', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'only survivor',
      role: 'researcher',
    })
    flush()
    await tick()

    expect(container.querySelector('.verevon-wide-research')).toBeNull()
  })

  it('a card transitions running -> done when its matching SubagentStoppedEvent arrives', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item A',
    })
    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.1',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item B',
    })
    flush()
    await tick()

    handlers().onSubagentStopped?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      status: 'completed',
    })
    flush()
    await tick()

    const cards = [...container.querySelectorAll('.verevon-wide-research__card')]
    const done = cards.find((card) => card.textContent?.includes('item A'))
    const stillRunning = cards.find((card) => card.textContent?.includes('item B'))
    expect(done?.getAttribute('data-status')).toBe('done')
    expect(stillRunning?.getAttribute('data-status')).toBe('running')
  })

  it('keeps untouched cards mounted when another item changes state or a new one joins', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    const attach = (childRunId: string, goalSummary: string) => handlers().onSubagentAttached?.({
      childRunId,
      batchId: 'run_1:tool_1_fan-1',
      goalSummary,
    })
    attach('run_1.tool_1_fan-1.0', 'item A')
    attach('run_1.tool_1_fan-1.1', 'item B')
    flush()
    await tick()

    const cardFor = (text: string) => [...container.querySelectorAll('.verevon-wide-research__card')]
      .find((card) => card.textContent?.includes(text))
    const batch = container.querySelector('.verevon-wide-research__batch')
    const cardB = cardFor('item B')
    expect(batch).not.toBeNull()
    expect(cardB).toBeDefined()

    // One item finishes...
    handlers().onSubagentStopped?.({ childRunId: 'run_1.tool_1_fan-1.0', status: 'completed' })
    flush()
    await tick()
    expect(cardFor('item A')?.getAttribute('data-status')).toBe('done')
    // ...and a third joins the batch.
    attach('run_1.tool_1_fan-1.2', 'item C')
    flush()
    await tick()

    expect(cardFor('item C')).toBeDefined()
    expect(container.querySelector('.verevon-wide-research__batch-head')?.textContent).toContain('3 deloppgaver')
    // Unkeyed, every event remounted the batch and all its cards: a running
    // card's spinner restarted on each one, dozens of times over a wide fan-out.
    expect(container.querySelector('.verevon-wide-research__batch')).toBe(batch)
    expect(cardFor('item B')).toBe(cardB)
  })

  it('a failed item is distinguished from a completed one', async () => {
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item A',
    })
    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.1',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item B',
    })
    flush()
    await tick()

    handlers().onSubagentStopped?.({ childRunId: 'run_1.tool_1_fan-1.0', status: 'failed' })
    flush()
    await tick()

    const cards = [...container.querySelectorAll('.verevon-wide-research__card')]
    const failed = cards.find((card) => card.textContent?.includes('item A'))
    expect(failed?.getAttribute('data-status')).toBe('failed')
  })

  it('a card that never receives a stop event shows "did not finish" once the run ends, never a perpetual spinner', async () => {
    hoisted.runDetail = null
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick()

    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.0',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item A',
    })
    handlers().onSubagentAttached?.({
      childRunId: 'run_1.tool_1_fan-1.1',
      batchId: 'run_1:tool_1_fan-1',
      goalSummary: 'item B',
    })
    flush()
    await tick()

    // The run ends (stream closes) with item B never having stopped.
    handlers().onSubagentStopped?.({ childRunId: 'run_1.tool_1_fan-1.0', status: 'completed' })
    handlers().onDone?.()
    flush()
    await tick()

    const cards = [...container.querySelectorAll('.verevon-wide-research__card')]
    const done = cards.find((card) => card.textContent?.includes('item A'))
    const unresolved = cards.find((card) => card.textContent?.includes('item B'))
    expect(done?.getAttribute('data-status')).toBe('done')
    expect(unresolved?.getAttribute('data-status')).toBe('unknown')
    expect(unresolved?.textContent).toContain('Fullførte ikke')
  })

  it('does not read the lineage without a threadId, so a live-only host starts empty as before', async () => {
    hoisted.lineageEdges = [
      { parentRunId: 'run_1', childRunId: 'c0', goalSummary: 'item A', batchId: 'b1', status: 'attached' },
      { parentRunId: 'run_1', childRunId: 'c1', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
    ]
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" />)
    await tick(3)

    expect(hoisted.lineageThreads).toHaveLength(0)
    expect(container.querySelector('.verevon-wide-research')).toBeNull()
  })

  it('restores the grid after a reload from the lineage, with each card status read from its child run', async () => {
    hoisted.runDetail = { status: 'completed' }
    hoisted.runDetails = {
      c0: { status: 'completed' },
      c1: { status: 'failed' },
      c2: { status: 'running' },
    }
    hoisted.lineageEdges = [
      { parentRunId: 'run_1', childRunId: 'c0', goalSummary: 'item A', batchId: 'b1', status: 'detached' },
      { parentRunId: 'run_1', childRunId: 'c1', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
      // A retry of item B started from a later run: it joins the batch it retried.
      { parentRunId: 'run_2', childRunId: 'c2', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
      // Another run's fan-out and a standalone delegation are not this run's grid.
      { parentRunId: 'run_9', childRunId: 'x0', goalSummary: 'other', batchId: 'b9', status: 'attached' },
      { parentRunId: 'run_1', childRunId: 'x1', goalSummary: 'lone task', batchId: '', status: 'attached' },
    ]
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" threadId="thread_1" />)
    await tick(4)

    expect(hoisted.lineageThreads).toEqual(['thread_1'])
    const cards = [...container.querySelectorAll('.verevon-wide-research__card')]
    expect(cards).toHaveLength(3)
    expect(container.textContent).not.toContain('other')
    expect(container.textContent).not.toContain('lone task')
    const status = (index: number) => cards[index]?.getAttribute('data-status')
    expect(status(0)).toBe('done')
    expect(status(1)).toBe('failed')
    // The parent run ended, so a child still shown running did not finish.
    expect(status(2)).toBe('unknown')
  })

  it('joins live stop events onto the cards it seeded from the lineage', async () => {
    hoisted.runDetails = { c0: { status: 'running' }, c1: { status: 'running' } }
    hoisted.lineageEdges = [
      { parentRunId: 'run_1', childRunId: 'c0', goalSummary: 'item A', batchId: 'b1', status: 'attached' },
      { parentRunId: 'run_1', childRunId: 'c1', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
    ]
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" threadId="thread_1" />)
    await tick(4)

    handlers().onSubagentStopped?.({ childRunId: 'c0', status: 'completed' })
    flush()
    await tick()

    const cards = [...container.querySelectorAll('.verevon-wide-research__card')]
    expect(cards).toHaveLength(2)
    expect(cards.find((card) => card.textContent?.includes('item A'))?.getAttribute('data-status')).toBe('done')
    expect(cards.find((card) => card.textContent?.includes('item B'))?.getAttribute('data-status')).toBe('running')
  })

  it('offers "Prøv igjen" only on an item without an answer, and only when the host can retry', async () => {
    hoisted.runDetails = { c0: { status: 'completed' }, c1: { status: 'failed' } }
    hoisted.lineageEdges = [
      { parentRunId: 'run_1', childRunId: 'c0', goalSummary: 'item A', batchId: 'b1', status: 'attached' },
      { parentRunId: 'run_1', childRunId: 'c1', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
    ]
    const onRetryItem = vi.fn(async () => {})
    const { container } = render(() => (
      <ChatWideResearchPanel runId="run_1" threadId="thread_1" onRetryItem={onRetryItem} />
    ))
    await tick(4)

    const cardFor = (text: string) => [...container.querySelectorAll('.verevon-wide-research__card')]
      .find((card) => card.textContent?.includes(text))
    expect(cardFor('item A')?.querySelector('.verevon-wide-research__retry')).toBeNull()
    const button = cardFor('item B')?.querySelector<HTMLButtonElement>('.verevon-wide-research__retry')
    expect(button?.textContent).toContain('Prøv igjen')

    button?.click()
    flush()
    await tick()

    expect(onRetryItem).toHaveBeenCalledTimes(1)
    expect(onRetryItem).toHaveBeenCalledWith({ childRunId: 'c1', batchId: 'b1', goalSummary: 'item B' })
  })

  it('offers no retry when the host does not wire one', async () => {
    hoisted.runDetails = { c0: { status: 'failed' }, c1: { status: 'failed' } }
    hoisted.lineageEdges = [
      { parentRunId: 'run_1', childRunId: 'c0', goalSummary: 'item A', batchId: 'b1', status: 'attached' },
      { parentRunId: 'run_1', childRunId: 'c1', goalSummary: 'item B', batchId: 'b1', status: 'attached' },
    ]
    const { container } = render(() => <ChatWideResearchPanel runId="run_1" threadId="thread_1" />)
    await tick(4)

    expect(container.querySelectorAll('.verevon-wide-research__card')).toHaveLength(2)
    expect(container.querySelector('.verevon-wide-research__retry')).toBeNull()
  })
})
