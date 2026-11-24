import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createWakeBoard, type WakeEntry, type WakeSequencer, type WakeState } from '../lib/wake-sequencer'

const { WakeStatus } = await import('./WakeStatus')

afterEach(() => cleanup())

function wake(agentSubjectId: string, agentName: string, ordinal: number, state: WakeState, code?: string): WakeEntry {
  return {
    messageId: 'msg-1',
    threadId: 'thread-1',
    spaceRef: 'space_1',
    content: 'Hei @Ada',
    agentSubjectId,
    agentName,
    ordinal,
    clientWakeId: `cw-${agentSubjectId}`,
    state,
    ...(code ? { code } : {}),
  }
}

function renderStatus(entries: readonly WakeEntry[], messageId: string | undefined = 'msg-1') {
  const board = createWakeBoard()
  board.set('msg-1', entries)
  const sequencer = {
    start: vi.fn(),
    wakeNow: vi.fn(),
    retry: vi.fn(),
    interrupt: vi.fn(),
    onSettled: vi.fn(() => () => undefined),
  }
  render(() => (
    <WakeStatus messageId={messageId} board={board} sequencer={sequencer as unknown as WakeSequencer} />
  ))
  return { sequencer }
}

const line = (text: string) => screen.getByText(text)

describe('WakeStatus (WS08-S6)', () => {
  it('draws nothing for a post that wakes no agent', () => {
    const { container } = render(() => <WakeStatus messageId={undefined} board={createWakeBoard()} />)
    expect(container.querySelector('ul')).toBeNull()
  })

  it('says, for each addressed agent, where it stands, in words', () => {
    renderStatus([
      wake('agent-ada', 'Ada', 1, 'queued'),
      wake('agent-bo', 'Bo', 2, 'waiting'),
      wake('agent-cy', 'Cy', 3, 'answering'),
      wake('agent-di', 'Di', 4, 'done'),
    ])
    expect(line('Ada er neste')).toBeTruthy()
    expect(line('Bo venter: en agent jobber allerede i samtalen')).toBeTruthy()
    expect(line('Cy svarer …')).toBeTruthy()
    expect(line('Di har svart')).toBeTruthy()
    expect(document.querySelector('[data-state="answering"]')).not.toBeNull()
    expect(document.querySelector('[data-state="done"]')).not.toBeNull()
  })

  it('is a polite live region named for the agents mentioned', () => {
    renderStatus([wake('agent-ada', 'Ada', 1, 'queued')])
    const region = screen.getByRole('status', { name: 'Agenter som er nevnt' })
    expect(region.getAttribute('aria-live')).toBe('polite')
  })

  it('a refused agent says why in words, and is offered no retry', () => {
    renderStatus([wake('agent-ada', 'Ada', 1, 'refused', 'agent_respond_to_denied')])
    expect(line('Ada: Denne agenten svarer ikke på meldinger fra deg i dette rommet. En leder bestemmer hvem som kan tilkalle den.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Vekk nå' })).toBeNull()
  })

  it('a failed wake with a code that retrying can fix gets Retry, which runs that agent again', () => {
    const { sequencer } = renderStatus([wake('agent-ada', 'Ada', 1, 'failed', 'session_unavailable')])
    expect(line('Ada: Samtaletjenesten er utilgjengelig akkurat nå. Prøv igjen om litt.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(sequencer.retry).toHaveBeenCalledWith('msg-1', 'agent-ada')
  })

  it('a run that was already busy in the thread is also retryable', () => {
    renderStatus([wake('agent-ada', 'Ada', 1, 'failed', 'thread_run_active')])
    expect(screen.getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
  })

  it('a failed wake with a code that retrying cannot fix gets no Retry', () => {
    renderStatus([wake('agent-ada', 'Ada', 1, 'failed', 'wake_owner_required')])
    expect(line('Ada: Bare den som startet denne samtalen kan tilkalle en agent i den. Meldingen er sendt uten svar fra agenten.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })

  it('an interrupted wake says it has not started, and Wake now starts the interrupted ones', () => {
    const { sequencer } = renderStatus([wake('agent-ada', 'Ada', 1, 'interrupted')])
    expect(line('Ada er ikke startet')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Vekk nå' }))
    expect(sequencer.wakeNow).toHaveBeenCalledWith('msg-1')
  })

  it('Wake now is offered only while something is interrupted', () => {
    renderStatus([wake('agent-ada', 'Ada', 1, 'done'), wake('agent-bo', 'Bo', 2, 'answering')])
    expect(screen.queryByRole('button', { name: 'Vekk nå' })).toBeNull()
  })
})
