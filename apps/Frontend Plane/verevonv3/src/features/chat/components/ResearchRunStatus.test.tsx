// @vitest-environment jsdom

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// The chip's Stop claims a stop only when the gateway says it reached the run's live stream. A stop that did not reach
// it (`stopRequested` false) reads the run again instead, so a run that has ended shows its end, not a stale 'running'.

const RUN_ID = 'rsr_01ARZ3NDEKTSV4RRFFQ69G5FAV'

const hoisted = vi.hoisted(() => ({
  executeAction: vi.fn(),
  getResearchRun: vi.fn(),
}))

vi.mock('@/shared/actions/action-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/actions/action-client')>()
  return { ...actual, executeAction: hoisted.executeAction }
})

vi.mock('@/shared/api/research-runs-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/research-runs-client')>()
  return { ...actual, getResearchRun: hoisted.getResearchRun }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({
    status: 'authenticated',
    user: { id: 'user-1', role: 'member' },
    activeOrg: { id: 'org-1' },
  }),
}))

import { ResearchRunStatus } from './ResearchRunStatus'

function readRun(status: 'running' | 'completed') {
  return { id: RUN_ID, threadId: null, status, stopReason: null, pagesRead: 4, sourcesFound: 2, spentUsd: 0.12 }
}

function chip(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[data-testid="research-run-status"]')
}

/** The chip's Stop button, found by its place in the chip because its text follows the person's language. */
function stopButton(): HTMLButtonElement {
  const button = chip()?.querySelector<HTMLButtonElement>('button')
  if (!button) throw new Error('the running chip has no Stop button')
  return button
}

/** Lets the stop, its read-back and the ended read settle: each awaited step runs in a later macrotask. */
async function settle(): Promise<void> {
  for (let step = 0; step < 5; step += 1) {
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
  flush()
}

function renderRunningChip(): void {
  render(() => <ResearchRunStatus run={{ id: RUN_ID, status: 'running' }} />)
}

beforeEach(() => {
  hoisted.executeAction.mockReset()
  hoisted.getResearchRun.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('ResearchRunStatus stop', () => {
  it('disables Stop as requested only when the gateway says the stop reached the live stream', async () => {
    hoisted.executeAction.mockResolvedValueOnce({ result: { id: RUN_ID, stopRequested: true } })
    renderRunningChip()

    fireEvent.click(stopButton())
    await settle()

    expect(hoisted.executeAction).toHaveBeenCalledWith('chat.research_run.cancel', expect.anything(), {
      researchRunId: RUN_ID,
    })
    expect(stopButton().disabled).toBe(true)
    expect(hoisted.getResearchRun).not.toHaveBeenCalled()
  })

  it('does not claim a stop the stream did not get, and shows the run as the gateway now reads it', async () => {
    hoisted.executeAction.mockResolvedValueOnce({ result: { id: RUN_ID, stopRequested: false } })
    hoisted.getResearchRun.mockResolvedValue(readRun('completed'))
    renderRunningChip()

    fireEvent.click(stopButton())
    await settle()

    expect(hoisted.getResearchRun).toHaveBeenCalledWith(RUN_ID)
    expect(chip()?.getAttribute('data-status')).toBe('completed')
    expect(chip()?.querySelector('button')).toBeNull()
  })

  it('keeps Stop available when the run is still running after a stop the stream did not get', async () => {
    hoisted.executeAction.mockResolvedValueOnce({ result: { id: RUN_ID, stopRequested: false } })
    hoisted.getResearchRun.mockResolvedValue(readRun('running'))
    renderRunningChip()

    fireEvent.click(stopButton())
    await settle()

    expect(hoisted.getResearchRun).toHaveBeenCalledWith(RUN_ID)
    expect(chip()?.getAttribute('data-status')).toBe('running')
    expect(stopButton().disabled).toBe(false)
  })
})
