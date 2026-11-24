import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

const spacesClient = vi.hoisted(() => ({
  getSpacePrivatePanel: vi.fn(),
  openSpacePrivatePanel: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))

const chatClient = vi.hoisted(() => ({
  getChatThreadTranscript: vi.fn(),
  streamChat: vi.fn(),
}))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  ...chatClient,
}))

const { SpacePrivatePanel } = await import('./SpacePrivatePanel')

const ROOM = 'room-1'
const THREAD = 'thr_owner_1'

/** The owner's pointer for the room: a thread once one is open, none before the first open. */
const pointer = (threadId: string | null) => ({
  spaceRef: ROOM,
  threadId,
  agentSubjectId: threadId ? 'agent-1' : null,
})

/** The snapshot the chat transcript route returns for the owner's thread. `turns` are passed through raw. */
const transcript = (turns: unknown[]) => ({
  threadId: THREAD,
  turns,
  taskSteps: [],
  updatedAt: '2026-11-18T09:00:00.000Z',
})

function renderPanel(options: { canShare?: boolean } = {}) {
  const [canShare, setCanShare] = createSignal(options.canShare ?? false)
  const onShare = vi.fn()
  render(() => <SpacePrivatePanel spaceRef={ROOM} canShare={() => canShare()} onShare={onShare} />)
  return { onShare, setCanShare }
}

const ZDR_TEXT = /Legitimasjon med Zero Data Retention kan ikke opprette/
const openButton = () => screen.queryByRole('button', { name: 'Åpne privat assistent' })

beforeEach(() => {
  spacesClient.getSpacePrivatePanel.mockReset()
  spacesClient.openSpacePrivatePanel.mockReset()
  chatClient.getChatThreadTranscript.mockReset()
  chatClient.streamChat.mockReset()
})

afterEach(() => {
  cleanup()
})

describe('SpacePrivatePanel (WS17-S6)', () => {
  it('shows the catalogue text and no open button when the pointer is refused with 412 ZDR', async () => {
    spacesClient.getSpacePrivatePanel.mockRejectedValue(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
    renderPanel()
    expect(await screen.findByText(ZDR_TEXT)).toBeTruthy()
    expect(openButton()).toBeNull()
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
    expect(spacesClient.openSpacePrivatePanel).not.toHaveBeenCalled()
    expect(chatClient.getChatThreadTranscript).not.toHaveBeenCalled()
    expect(window.localStorage.length).toBe(0)
  })

  it('shows the catalogue text and drops the open button when opening is refused with 412 ZDR', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(null))
    spacesClient.openSpacePrivatePanel.mockRejectedValue(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
    renderPanel()
    fireEvent.click(await screen.findByRole('button', { name: 'Åpne privat assistent' }))
    flush()
    expect(await screen.findByText(ZDR_TEXT)).toBeTruthy()
    await waitFor(() => expect(openButton()).toBeNull())
    expect(spacesClient.openSpacePrivatePanel).toHaveBeenCalledTimes(1)
    expect(spacesClient.openSpacePrivatePanel).toHaveBeenCalledWith(ROOM)
    expect(window.localStorage.length).toBe(0)
  })

  it('offers the open button when no thread is open yet, and opening shows the owner thread', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValueOnce(pointer(null)).mockResolvedValue(pointer(THREAD))
    spacesClient.openSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(transcript([{ role: 'user', content: 'Hei Kari' }]))
    renderPanel()
    expect(await screen.findByText('Den private samtalen for dette rommet er ikke åpnet ennå.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Åpne privat assistent' }))
    flush()
    expect(await screen.findByText('Hei Kari')).toBeTruthy()
    expect(spacesClient.openSpacePrivatePanel).toHaveBeenCalledWith(ROOM)
    expect(chatClient.getChatThreadTranscript).toHaveBeenCalledWith(THREAD)
    expect(openButton()).toBeNull()
  })

  it('shows the turns of an open thread and no open button', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(
      transcript([
        { role: 'user', content: 'Hei Kari' },
        { role: 'assistant', content: 'Hei Ola' },
      ]),
    )
    renderPanel()
    expect(await screen.findByText('Hei Kari')).toBeTruthy()
    expect(screen.getByText('Hei Ola')).toBeTruthy()
    expect(openButton()).toBeNull()
    expect(chatClient.getChatThreadTranscript).toHaveBeenCalledWith(THREAD)
  })

  it('shows only the user and assistant turns that have text', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(
      transcript([
        { role: 'system', content: 'Interne instruksjoner for assistenten' },
        { role: 'user', content: 'Hei Kari' },
        { role: 'tool', content: 'Verktøyresultat' },
        { role: 'assistant', content: '   ' },
        { role: 'assistant', content: 'Hei Ola' },
        { role: 'user' },
      ]),
    )
    renderPanel()
    const list = await screen.findByRole('list', { name: 'Den private samtalen' })
    const turns = within(list).getAllByRole('listitem')
    expect(turns.map((turn) => turn.getAttribute('data-role'))).toEqual(['user', 'assistant'])
    const [userTurn, assistantTurn] = turns
    if (!userTurn || !assistantTurn) throw new Error('expected a user turn and an assistant turn')
    expect(within(userTurn).getByText('Du')).toBeTruthy()
    expect(within(userTurn).getByText('Hei Kari')).toBeTruthy()
    expect(within(assistantTurn).getByText('Assistent')).toBeTruthy()
    expect(within(assistantTurn).getByText('Hei Ola')).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('Interne instruksjoner')
    expect(document.body.textContent ?? '').not.toContain('Verktøyresultat')
  })

  // The pane only reads and shares: the conversation continues in Chat. Without a link there, a person who had opened the
  // private assistant had nowhere to write, and the thread has no title to find in the chat history.
  it('links the open thread to Chat, by its id, once a thread is open', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(transcript([{ role: 'user', content: 'Hei Kari' }]))
    renderPanel()
    const link = await screen.findByRole('link', { name: 'Fortsett i Chat' })
    expect(link.getAttribute('href')).toBe(`/chat?thread_id=${THREAD}`)
  })

  it('offers no link before the thread is opened, and the link comes with the open', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValueOnce(pointer(null)).mockResolvedValue(pointer(THREAD))
    spacesClient.openSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(transcript([]))
    renderPanel()
    expect(await screen.findByText('Den private samtalen for dette rommet er ikke åpnet ennå.')).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'Fortsett i Chat' })).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Åpne privat assistent' }))
    flush()
    expect((await screen.findByRole('link', { name: 'Fortsett i Chat' })).getAttribute('href')).toBe(`/chat?thread_id=${THREAD}`)
  })

  it('says an empty conversation continues in Chat, and does not promise a title the thread does not have', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(transcript([]))
    renderPanel()
    expect(await screen.findByText(/Skriv til assistenten nedenfor, eller fortsett i Chat/)).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('under navnet på rommet')
  })

  it('offers no link when the pointer cannot be read', async () => {
    spacesClient.getSpacePrivatePanel.mockRejectedValue(new ApiError('forbidden', 403, 'active_membership_required'))
    renderPanel()
    await screen.findByText(/Du er ikke lenger aktivt medlem/)
    expect(screen.queryByRole('link', { name: 'Fortsett i Chat' })).toBeNull()
  })

  it('offers Share to room on each turn only while canShare is true', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(
      transcript([
        { role: 'user', content: 'Hei Kari' },
        { role: 'assistant', content: 'Hei Ola' },
      ]),
    )
    const { onShare, setCanShare } = renderPanel({ canShare: false })
    expect(await screen.findByText('Hei Kari')).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Del i rommet/ })).toBeNull()

    setCanShare(true)
    flush()
    const shareButtons = await screen.findAllByRole('button', { name: /Del i rommet/ })
    expect(shareButtons).toHaveLength(2)
    const [, secondShare] = shareButtons
    if (!secondShare) throw new Error('expected a share button on the second turn')
    fireEvent.click(secondShare)
    flush()
    expect(onShare).toHaveBeenCalledTimes(1)
    expect(onShare).toHaveBeenCalledWith('Hei Ola')

    setCanShare(false)
    flush()
    await waitFor(() => expect(screen.queryByRole('button', { name: /Del i rommet/ })).toBeNull())
  })

  it('a pointer 403 shows its typed copy, and the retry is still offered', async () => {
    spacesClient.getSpacePrivatePanel.mockRejectedValue(new ApiError('forbidden', 403, 'active_membership_required'))
    renderPanel()
    expect(await screen.findByText(/Du er ikke lenger aktivt medlem/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
  })

  it('a pointer 403 that carries the ZDR code shows the catalogue text and no retry', async () => {
    spacesClient.getSpacePrivatePanel.mockRejectedValue(new ApiError('forbidden', 403, 'zdr_durable_mutation_forbidden'))
    renderPanel()
    expect(await screen.findByText(ZDR_TEXT)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
    expect(openButton()).toBeNull()
  })
})

// D21e: the panel's own composer. A turn goes over the private-turn path (the owner's chat stream on the panel thread,
// conversation scope, no room), so the reply stays private to the member.
describe('SpacePrivatePanel composer (D21e)', () => {
  const composerBox = () => screen.queryByLabelText('Melding til assistenten (bare du ser den)') as HTMLTextAreaElement | null

  it('offers no composer before the thread is open, nor under a ZDR refusal', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(null))
    renderPanel()
    expect(await screen.findByText('Den private samtalen for dette rommet er ikke åpnet ennå.')).toBeTruthy()
    expect(composerBox()).toBeNull()
    cleanup()

    spacesClient.getSpacePrivatePanel.mockRejectedValue(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
    renderPanel()
    expect(await screen.findByText(ZDR_TEXT)).toBeTruthy()
    expect(composerBox()).toBeNull()
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('sends on the panel thread with conversation scope and no room, shows the reply, then rereads the thread', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript
      .mockResolvedValueOnce(transcript([]))
      .mockResolvedValue(transcript([{ role: 'user', content: 'Hva sa vi?' }, { role: 'assistant', content: 'Dette.' }]))
    let finish: () => void = () => {}
    chatClient.streamChat.mockImplementation(
      (_request: unknown, handlers: { onMessage?: (event: { content: string }) => void }) =>
        new Promise<void>((resolve) => {
          handlers.onMessage?.({ content: 'Det' })
          finish = resolve
        }),
    )
    renderPanel()
    await waitFor(() => expect(composerBox()).toBeTruthy())
    const box = composerBox() as HTMLTextAreaElement
    fireEvent.input(box, { target: { value: '  Hva sa vi?  ' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    flush()

    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
    const [request] = chatClient.streamChat.mock.calls[0] as [Record<string, unknown>]
    expect(request).toEqual({ content: 'Hva sa vi?', threadId: THREAD, sourceScope: 'conversation', locale: 'no' })
    expect(request).not.toHaveProperty('spaceRef')
    expect(await screen.findByText('Hva sa vi?')).toBeTruthy()
    expect(await screen.findByText('Det')).toBeTruthy()

    finish()
    expect(await screen.findByText('Dette.')).toBeTruthy()
    expect(chatClient.getChatThreadTranscript).toHaveBeenCalledTimes(2)
    expect(composerBox()?.value).toBe('')
  })

  it('keeps the text and shows the typed error when the send is refused', async () => {
    spacesClient.getSpacePrivatePanel.mockResolvedValue(pointer(THREAD))
    chatClient.getChatThreadTranscript.mockResolvedValue(transcript([]))
    chatClient.streamChat.mockImplementation(
      async (_request: unknown, handlers: { onError?: (event: { code: string; message: string }) => void }) => {
        handlers.onError?.({ code: 'upstream_unavailable', message: 'raw upstream text' })
      },
    )
    renderPanel()
    await waitFor(() => expect(composerBox()).toBeTruthy())
    fireEvent.input(composerBox() as HTMLTextAreaElement, { target: { value: 'Hei' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Send' }))
    flush()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(document.body.textContent ?? '').not.toContain('raw upstream text')
    await waitFor(() => expect(composerBox()?.value).toBe('Hei'))
  })
})
