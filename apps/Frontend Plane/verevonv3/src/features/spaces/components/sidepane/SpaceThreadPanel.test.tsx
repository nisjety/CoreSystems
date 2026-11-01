import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  getSpaceThreadTranscript: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceThreadPanel } = await import('./SpaceThreadPanel')

function renderPanel(threadId = 'thr_1', extra: Record<string, unknown> = {}) {
  const [id, setId] = createSignal(threadId)
  render(() => (
    <SpaceThreadPanel
      spaceRef="room-1"
      threadId={id()}
      thread={() => ({ thread_id: id(), space_id: 'room-1', updated_at: '2026-10-27T10:00:00Z' }) as never}
      roster={() => []}
      agentName="Lagerhjelpen"
      {...(extra as object)}
    />
  ))
  return { setId }
}

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
})

afterEach(() => cleanup())

describe('SpaceThreadPanel (WS01-S7a)', () => {
  it('shows a loading state before the read answers', async () => {
    spacesClient.getSpaceThreadTranscript.mockReturnValue(new Promise(() => undefined))
    renderPanel()
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()
  })

  it('renders ok turns read-only', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({
      turns: [
        { role: 'user', content: 'Hva er status?', authorSubjectId: 'user-1' },
        { role: 'assistant', content: 'Alt er klart.', agentName: 'Lagerhjelpen' },
      ],
    })
    renderPanel()
    expect(await screen.findByText('Hva er status?')).toBeTruthy()
    expect(screen.getByText('Alt er klart.')).toBeTruthy()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledWith('room-1', 'thr_1')
    // Read-only: no input, no reply control of its own.
    expect(screen.queryByRole('textbox')).toBeNull()
  })

  it('says how many replies the thread has beside its status', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
    renderPanel('thr_1', {
      thread: () => ({ thread_id: 'thr_1', space_id: 'room-1', latest_run_status: 'completed', reply_count: 6 }),
    })
    await screen.findByText('Hei')
    expect(document.querySelector('.verevon-space-thread-panel__replies')?.textContent).toBe('6 svar')
  })

  it('says "1 svar" for one reply and nothing for none', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
    renderPanel('thr_1', {
      thread: () => ({ thread_id: 'thr_1', space_id: 'room-1', reply_count: 1 }),
    })
    await screen.findByText('Hei')
    expect(document.querySelector('.verevon-space-thread-panel__replies')?.textContent).toBe('1 svar')
    cleanup()
    renderPanel('thr_1', {
      thread: () => ({ thread_id: 'thr_1', space_id: 'room-1', reply_count: 0 }),
    })
    await screen.findByText('Hei')
    expect(document.querySelector('.verevon-space-thread-panel__replies')).toBeNull()
  })

  // The timeline draws Verevon in the agent's colour; the pane used to draw the same turn in neutral black.
  it('draws an agent turn in the agent colour when the room agents are handed over', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({
      turns: [{ role: 'assistant', content: 'Alt er klart.', agentName: 'Lagerhjelpen' }],
    })
    renderPanel('thr_1', {
      agents: () => [{ agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active', identity_published: true, avatar_color: '#d85a30' }],
    })
    await screen.findByText('Alt er klart.')
    const avatar = document.querySelector('.verevon-agent-avatar') as HTMLElement
    expect(avatar.classList.contains('verevon-agent-avatar--neutral')).toBe(false)
    expect(avatar.style.background).toMatch(/#d85a30|rgb\(216, 90, 48\)/i)
  })

  it('says a thread with no readable turns has no content yet', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'system', content: 'x' }] })
    renderPanel()
    expect(await screen.findByText(/ikke noe lesbart innhold/)).toBeTruthy()
  })

  it('shows the restricted notice on 403, never a sign-in message', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('forbidden', 403, 'space_read_not_authorized'))
    renderPanel()
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
    expect(document.body.textContent ?? '').not.toMatch(/logg inn|sign in|log in/i)
  })

  it('says a typed 403 in the denial presenter’s words instead of the generic notice', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'space_thread_not_authorized', null, {
        reason: 'space_role_below_floor',
        resolvable_by: 'space_manager',
      }),
    )
    renderPanel()
    expect(await screen.findByText(/Rollen din i dette rommet er ikke høy nok til dette/)).toBeTruthy()
    expect(screen.queryByText(/Denne samtalen er ikke delt med deg/)).toBeNull()
  })

  it('uses the presenter’s generic line for a reason this build does not know', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'forbidden', null, { reason: 'a_newer_reason' }),
    )
    renderPanel()
    expect(await screen.findByText('Dette er ikke tillatt i rommet akkurat nå.')).toBeTruthy()
  })

  it('keeps the generic notice for a 403 that carries no typed reason', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'forbidden', null, { other: 1 }),
    )
    renderPanel()
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
  })

  it('shows the not-available state on 404', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('missing', 404, 'thread_not_found'))
    renderPanel()
    expect(await screen.findByText(/Samtalen er ikke tilgjengelig/)).toBeTruthy()
  })

  it('shows an alert with a retry on a server error, and the retry reads again', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValueOnce(new ApiError('boom', 500, null))
    renderPanel()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Kunne ikke hente samtalen')
    spacesClient.getSpaceThreadTranscript.mockResolvedValueOnce({
      turns: [{ role: 'user', content: 'Nå virker det' }],
    })
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(await screen.findByText('Nå virker det')).toBeTruthy()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(2)
  })

  it('treats a network failure (not an ApiError) as the retryable error state', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(new TypeError('Failed to fetch'))
    renderPanel()
    expect(await screen.findByRole('alert')).toBeTruthy()
  })

  it('hides the previous thread while a different one loads', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValueOnce({
      turns: [{ role: 'user', content: 'Første samtale' }],
    })
    const { setId } = renderPanel('thr_1')
    expect(await screen.findByText('Første samtale')).toBeTruthy()

    spacesClient.getSpaceThreadTranscript.mockReturnValueOnce(new Promise(() => undefined))
    setId('thr_2')
    await waitFor(() => expect(screen.queryByText('Første samtale')).toBeNull())
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenLastCalledWith('room-1', 'thr_2')
  })

  describe('the reply footer slot', () => {
    it('draws the footer under an available thread', async () => {
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
      renderPanel('thr_1', { footer: <div data-testid="pane-footer" /> })
      await screen.findByText('Hei')
      expect(screen.getByTestId('pane-footer')).toBeTruthy()
    })

    // A restricted thread is still a thread the member can post in (the room's composer decides),
    // so the footer is drawn for it; only a thread that is gone has nothing to reply to.
    it('draws the footer for a restricted thread too', async () => {
      spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('forbidden', 403, 'space_read_not_authorized'))
      renderPanel('thr_1', { footer: <div data-testid="pane-footer" /> })
      await screen.findByText(/Denne samtalen er ikke delt med deg/)
      expect(screen.getByTestId('pane-footer')).toBeTruthy()
    })

    it('does not draw the footer for a thread that is not available to this member', async () => {
      spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('missing', 404, null))
      renderPanel('thr_1', { footer: <div data-testid="pane-footer" /> })
      await screen.findByText(/Samtalen er ikke tilgjengelig/)
      expect(screen.queryByTestId('pane-footer')).toBeNull()
    })

    it('leaves the panel read-only when no footer is passed', async () => {
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
      renderPanel()
      await screen.findByText('Hei')
      expect(document.querySelector('.verevon-space-thread-panel__composer')).toBeNull()
    })
  })
})
