import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { ApiError } from '@/shared/api/http'

import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../../lib/space-room-features'

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceThreadTranscript: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
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

afterEach(() => {
  cleanup()
  __resetRetentionForTests()
  clearSpaceRoomFeatures()
})

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
    expect(document.querySelector('.verevon-space-thread-panel__turns')).toBeNull()
  })

  it('says a typed 403 in the denial presenter’s words instead of the generic notice', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'space_thread_not_authorized', {
        details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
      }),
    )
    renderPanel()
    expect(await screen.findByText(/Rollen din i dette rommet er ikke høy nok til dette/)).toBeTruthy()
    expect(screen.queryByText(/Denne samtalen er ikke delt med deg/)).toBeNull()
  })

  it('uses the presenter’s generic line for a reason this build does not know', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'forbidden', { details: { reason: 'a_newer_reason' } }),
    )
    renderPanel()
    expect(await screen.findByText('Dette er ikke tillatt i rommet akkurat nå.')).toBeTruthy()
  })

  it('keeps the generic notice for a 403 that carries no typed reason', async () => {
    spacesClient.getSpaceThreadTranscript.mockRejectedValue(
      new ApiError('forbidden', 403, 'forbidden', { details: { other: 1 } }),
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
      expect(document.querySelector('.verevon-space-thread-panel__turns')).toBeNull()
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

  // Shared reads off for this Space is the same rule the timeline keeps (its `transcriptEnabled` gate):
  // no transcript call, the room's restricted notice, and no turn text, including turns read earlier.
  describe('shared reads (the timeline rule)', () => {
    it('makes no transcript call and shows no turn text while shared reads are off', async () => {
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        turns: [{ role: 'user', content: 'Hemmelig innhold', authorSubjectId: 'user-1' }],
      })
      renderPanel('thr_1', { sharedReadAvailable: () => false })
      expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
      // Let any scheduled read run before asserting that none was made.
      await new Promise((resolve) => setTimeout(resolve, 0))
      flush()
      expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()
      expect(screen.queryByText('Hemmelig innhold')).toBeNull()
      expect(document.querySelector('.verevon-space-thread-panel__turns')).toBeNull()
      expect(screen.queryByText(/Henter samtalen/)).toBeNull()
    })

    // The control for the case above: the same thread, with shared reads on, is read and drawn.
    it('reads the thread and draws its turns while shared reads are on', async () => {
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        turns: [{ role: 'user', content: 'Hemmelig innhold', authorSubjectId: 'user-1' }],
      })
      renderPanel('thr_1', { sharedReadAvailable: () => true })
      expect(await screen.findByText('Hemmelig innhold')).toBeTruthy()
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
      expect(screen.queryByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeNull()
    })

    it('hides the turns it read earlier once shared reads go off, and asks for nothing more', async () => {
      const [shared, setShared] = createSignal(true)
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        turns: [{ role: 'user', content: 'Tidligere innhold' }],
      })
      renderPanel('thr_1', { sharedReadAvailable: shared })
      expect(await screen.findByText('Tidligere innhold')).toBeTruthy()

      setShared(false)
      flush()
      expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
      expect(screen.queryByText('Tidligere innhold')).toBeNull()
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
    })

    it('reads the thread once when shared reads come back on', async () => {
      const [shared, setShared] = createSignal(false)
      spacesClient.getSpaceThreadTranscript.mockResolvedValue({
        turns: [{ role: 'user', content: 'Igjen synlig' }],
      })
      renderPanel('thr_1', { sharedReadAvailable: shared })
      await new Promise((resolve) => setTimeout(resolve, 0))
      flush()
      expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()

      setShared(true)
      flush()
      expect(await screen.findByText('Igjen synlig')).toBeTruthy()
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledWith('room-1', 'thr_1')
    })

    it('a refused re-read hides the turns it showed before', async () => {
      const [updated, setUpdated] = createSignal('2026-10-27T10:00:00Z')
      spacesClient.getSpaceThreadTranscript.mockResolvedValueOnce({
        turns: [{ role: 'user', content: 'Før avslaget' }],
      })
      renderPanel('thr_1', {
        thread: () => ({ thread_id: 'thr_1', space_id: 'room-1', updated_at: updated() }),
      })
      expect(await screen.findByText('Før avslaget')).toBeTruthy()

      spacesClient.getSpaceThreadTranscript.mockRejectedValueOnce(
        new ApiError('forbidden', 403, 'space_read_not_authorized'),
      )
      setUpdated('2026-10-27T11:00:00Z')
      flush()
      expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
      expect(screen.queryByText('Før avslaget')).toBeNull()
      expect(document.querySelector('.verevon-space-thread-panel__turns')).toBeNull()
      expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(2)
    })
  })
})

describe('SpaceThreadPanel and the workspace posture (WS01-S7a)', () => {
  it('takes the turns off the screen when the workspace turns zero retention on, then reads again', async () => {
    spacesClient.getSpaceThreadTranscript
      .mockResolvedValueOnce({ turns: [{ role: 'user', content: 'Lagerstatus i dag' }] })
      .mockReturnValueOnce(new Promise(() => undefined))
    renderPanel()
    expect(await screen.findByText('Lagerstatus i dag')).toBeTruthy()

    applyServerRetention(true)
    flush()

    expect(screen.queryByText('Lagerstatus i dag')).toBeNull()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(2)
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()
  })

  it('leaves the turns alone when the posture is allowed or stays the same', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Lagerstatus i dag' }] })
    renderPanel()
    expect(await screen.findByText('Lagerstatus i dag')).toBeTruthy()

    applyServerRetention(false)
    flush()

    expect(screen.getByText('Lagerstatus i dag')).toBeTruthy()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
  })
})

// `?m=` (WS13-S4a): a message the pane brings into view. The page passes the id; the pane decides whether the
// transcript the viewer may read holds it, and says so neutrally when it does not.
describe('SpaceThreadPanel reaches a message named by ?m= (WS13-S4a)', () => {
  const turns = [
    { role: 'user', content: 'Første melding', authorSubjectId: 'user-1', messageId: 'msg_1' },
    { role: 'assistant', content: 'Svar med status', agentName: 'Lagerhjelpen', messageId: 'msg_2' },
  ]
  const scrollIntoView = vi.fn()

  beforeEach(() => {
    scrollIntoView.mockClear()
    Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, value: scrollIntoView })
  })

  afterEach(() => {
    delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView
  })

  it('scrolls to, focuses and outlines the named turn, and announces whose message it is', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns })
    renderPanel('thr_1', { messageId: () => 'msg_2' })

    expect(await screen.findByText('Svar med status')).toBeTruthy()
    const target = document.querySelector('[data-message-id="msg_2"]') as HTMLElement
    await waitFor(() => expect(target.getAttribute('data-highlight')).toBe('true'))
    expect(scrollIntoView).toHaveBeenCalledWith(expect.objectContaining({ block: 'center' }))
    expect(scrollIntoView.mock.contexts[0]).toBe(target)
    expect(document.activeElement).toBe(target)
    // The other turn is neither outlined nor the target.
    expect(document.querySelector('[data-message-id="msg_1"]')?.hasAttribute('data-highlight')).toBe(false)
    const status = document.querySelector('[data-entry-status="found"]')
    expect(status?.textContent).toBe('Viser melding fra Lagerhjelpen')
  })

  it('takes the outline off after three seconds', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns })
    renderPanel('thr_1', { messageId: () => 'msg_2' })
    await screen.findByText('Svar med status')
    const target = document.querySelector('[data-message-id="msg_2"]') as HTMLElement
    await waitFor(() => expect(target.getAttribute('data-highlight')).toBe('true'))
    await waitFor(() => expect(target.hasAttribute('data-highlight')).toBe(false), { timeout: 4000 })
  })

  it('says the message is not available to the viewer, neutrally, when the transcript does not hold it', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns })
    renderPanel('thr_1', { messageId: () => 'msg_hidden_42' })

    const status = await waitFor(() => {
      const node = document.querySelector('[data-entry-status="missing"]')
      expect(node).toBeTruthy()
      return node as HTMLElement
    })
    expect(status.getAttribute('role')).toBe('status')
    expect(status.textContent).toBe('Denne meldingen er ikke tilgjengelig for deg.')
    // The id from the link is never echoed back, and no turn is outlined or scrolled to.
    expect(document.body.textContent ?? '').not.toContain('msg_hidden_42')
    expect(document.querySelector('[data-highlight]')).toBeNull()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })

  it('says the same neutral thing when shared reads are off, and makes no transcript call', async () => {
    renderPanel('thr_1', { messageId: () => 'msg_2', sharedReadAvailable: () => false })
    await waitFor(() => expect(document.querySelector('[data-entry-status="missing"]')).toBeTruthy())
    expect(document.querySelector('[data-entry-status="missing"]')?.textContent).toBe(
      'Denne meldingen er ikke tilgjengelig for deg.',
    )
    expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()
  })

  it('does not call a message missing while the transcript is still loading', async () => {
    spacesClient.getSpaceThreadTranscript.mockReturnValue(new Promise(() => undefined))
    renderPanel('thr_1', { messageId: () => 'msg_2' })
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()
    expect(document.querySelector('[data-entry-status="missing"]')).toBeNull()
  })

  it('draws no entry status at all without a message id', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns })
    renderPanel()
    await screen.findByText('Svar med status')
    expect(document.querySelector('[data-entry-status]')).toBeNull()
    expect(scrollIntoView).not.toHaveBeenCalled()
  })
})

describe('SpaceThreadPanel: the thread reminder control (WS11-S11)', () => {
  it('draws Remind me under an available thread only while the room reports features.reminders on', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({
      turns: [{ role: 'user', content: 'Husk levering', authorSubjectId: 'user-1' }],
    })
    renderPanel()
    expect(await screen.findByText('Husk levering')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Påminn meg' })).toBeNull()

    publishSpaceRoomFeatures({ reminders: true })
    await waitFor(() => expect(screen.getByRole('button', { name: 'Påminn meg' })).toBeTruthy())
    // The menu is drawn; nothing is read for it until it is opened.
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
  })
})
