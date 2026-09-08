import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { publishSpaceThreads, resetLiveWorkForTests } from '../lib/space-live-work'

const chatClient = vi.hoisted(() => ({
  streamChat: vi.fn(),
  cancelInvocation: vi.fn(),
}))
vi.mock('@/shared/api/chat-client', () => chatClient)
const spacesClient = vi.hoisted(() => ({ updateSpaceThreadPresentation: vi.fn() }))
vi.mock('@/shared/api/spaces-client', () => spacesClient)
const skillsClient = vi.hoisted(() => ({ listAvailableSkills: vi.fn(), MAX_PICKED_SKILLS: 4 }))
vi.mock('@/shared/api/skills-client', () => skillsClient)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceRoomComposer } = await import('./SpaceRoomComposer')

type Handlers = {
  onConnected?: (event: { requestId?: string; threadId?: string }) => void
  onTitle?: (event: { title: string }) => void
  onMessage?: (event: { content: string }) => void
  onDone?: (event: Record<string, never>) => void
  onStopped?: (event: Record<string, never>) => void
  onError?: (event: { code: string; message: string }) => void
}

/**
 * A stream the test drives by hand: `streamChat` hands back its handlers and a
 * promise the test settles, exactly as the transport would.
 */
function scriptedStream() {
  let handlers: Handlers | undefined
  let signal: AbortSignal | undefined
  let finish: () => void = () => {}
  chatClient.streamChat.mockImplementation(
    (_request: unknown, h: Handlers, s?: AbortSignal) =>
      new Promise<void>((resolve) => {
        handlers = h
        signal = s
        finish = resolve
      }),
  )
  return {
    handlers: () => handlers as Handlers,
    signal: () => signal,
    finish: () => finish(),
  }
}

const renderComposer = (threads: readonly unknown[] = []) =>
  render(() => (
    <SpaceRoomComposer
      spaceRef="room-1"
      roster={() => []}
      agents={() => []}
      threads={() => threads as never}
    />
  ))

async function send(text: string) {
  const box = screen.getByRole('textbox')
  fireEvent.input(box, { target: { value: text } })
  // Solid 2 defers the `setText` until a flush; submitting in the same tick
  // would read an empty draft and return before ever calling the transport.
  flush()
  fireEvent.submit(box.closest('form') as HTMLFormElement)
  await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
}

beforeEach(() => {
  chatClient.streamChat.mockReset()
  chatClient.cancelInvocation.mockReset()
  chatClient.cancelInvocation.mockResolvedValue(undefined)
  spacesClient.updateSpaceThreadPresentation.mockReset()
  spacesClient.updateSpaceThreadPresentation.mockResolvedValue({ thread_id: 't-new' })
  resetLiveWorkForTests()
})

afterEach(() => cleanup())

describe('SpaceRoomComposer — interruptible turns', () => {
  // Item 1b. Until this existed, a member who sent a message could only wait.
  it('offers Stop while the reply streams and hands an abort signal to the transport', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')

    expect(await screen.findByRole('button', { name: 'Stopp' })).toBeTruthy()
    expect(stream.signal()).toBeInstanceOf(AbortSignal)
    expect(stream.signal()?.aborted).toBe(false)
  })

  // The half that matters to everyone else in the room: aborting the read
  // alone leaves the run generating (Model Plane detaches-and-finishes a
  // departed client). The cancel by requestId is what records `cancelled`.
  it('records the stop by cancelling the invocation the stream announced', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-42' })
    flush()
    stream.handlers().onMessage?.({ content: 'Begynner…' })
    flush()

    screen.getByRole('button', { name: 'Stopp' }).click()

    await waitFor(() => expect(chatClient.cancelInvocation).toHaveBeenCalledWith('req-42'))
    expect(stream.signal()?.aborted).toBe(true)
    expect(await screen.findByText(/Rommet ser dette som stoppet/)).toBeTruthy()
    // Not an error, and not a finished answer.
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Stopp' })).toBeNull()
  })

  // No requestId means nothing to cancel with. Saying "stopped" here would
  // claim a recorded stop the server never saw.
  it('is honest when Stop lands before the stream announced itself', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')

    screen.getByRole('button', { name: 'Stopp' }).click()

    expect(await screen.findByText(/Avbrutt før svaret startet/)).toBeTruthy()
    expect(screen.getByText(/kan fortsatt fullføre på tjenersiden/)).toBeTruthy()
    expect(chatClient.cancelInvocation).not.toHaveBeenCalled()
    expect(stream.signal()?.aborted).toBe(true)
  })

  // A cancel the gateway refused must not be dressed up as a recorded stop.
  it('does not claim a recorded stop when the cancel call fails', async () => {
    const stream = scriptedStream()
    chatClient.cancelInvocation.mockRejectedValue(new Error('no active stream'))
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-42' })
    flush()

    screen.getByRole('button', { name: 'Stopp' }).click()

    await waitFor(() => expect(chatClient.cancelInvocation).toHaveBeenCalled())
    // Stays in the "stopped here, recording…" wording; never flips to recorded.
    expect(await screen.findByText(/Stoppet her/)).toBeTruthy()
    expect(screen.queryByText(/Rommet ser dette som stoppet/)).toBeNull()
  })

  // The server emits `stopped` and may still close the stream normally after
  // it. A trailing `done` used to turn a halted answer into a finished one.
  it('keeps a server-side stop even when a done frame follows it', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-42' })
    flush()
    stream.handlers().onStopped?.({})
    flush()
    stream.handlers().onDone?.({})
    flush()
    stream.finish()

    expect(await screen.findByText(/Rommet ser dette som stoppet/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Stopp' })).toBeNull()
  })

  it('treats its own abort as a stop, not as a transport error', async () => {
    chatClient.streamChat.mockImplementation(
      (_request: unknown, _h: Handlers, s?: AbortSignal) =>
        new Promise<void>((_resolve, reject) => {
          s?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))
        }),
    )
    renderComposer()
    await send('Rydd lageret')
    screen.getByRole('button', { name: 'Stopp' }).click()

    expect(await screen.findByText(/Avbrutt før svaret startet/)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('SpaceRoomComposer — the room is visibly working', () => {
  // Slack's "typing" affordance, for agents: a member about to type sees that
  // another member's agent is mid-reply, from the server's projection.
  it('says which conversation another member’s agent is working in', async () => {
    publishSpaceThreads('room-1', [
      { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'running' },
      { thread_id: 't2', space_id: 'room-1', title: 'Ferdig', latest_run_status: 'completed' },
    ])
    flush()
    renderComposer()

    expect(await screen.findByText(/Verevon jobber i «Innkjøp»/)).toBeTruthy()
  })

  it('counts when several conversations are working', async () => {
    publishSpaceThreads('room-1', [
      { thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' },
      { thread_id: 't2', space_id: 'room-1', latest_run_status: 'queued' },
    ])
    flush()
    renderComposer()

    expect(await screen.findByText(/jobber i 2 samtaler/)).toBeTruthy()
  })

  // The sender's own turn has its own live exchange above; repeating it in the
  // "others are working" line would count one reply twice.
  it('does not report the browser’s own reply as someone else’s work', async () => {
    const stream = scriptedStream()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        replyTarget={() => ({ threadId: 't1', title: 'Innkjøp' })}
      />
    ))
    await send('Fortsett')
    stream.handlers().onConnected?.({ requestId: 'req-1' })
    flush()
    // The poll now reports the very thread this browser is streaming into.
    publishSpaceThreads('room-1', [
      { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'running' },
    ])
    flush()

    await waitFor(() => expect(screen.getByRole('button', { name: 'Stopp' })).toBeTruthy())
    expect(screen.queryByText(/Verevon jobber i/)).toBeNull()
  })

  it('shows nothing when the room is quiet', () => {
    publishSpaceThreads('room-1', [
      { thread_id: 't1', space_id: 'room-1', latest_run_status: 'completed' },
    ])
    flush()
    renderComposer()
    expect(screen.queryByText(/Verevon jobber/)).toBeNull()
  })
})

describe('SpaceRoomComposer — the generated title reaches the room', () => {
  // Model Plane emits the title and does not store it; Chat keeps it in a
  // browser-local snapshot. A room needs every member to see the same name, so
  // the composer that opened the thread writes it back server-side.
  it('persists the generated title for a thread this browser opened', async () => {
    const stream = scriptedStream()
    const settled = vi.fn()
    render(() => (
      <SpaceRoomComposer spaceRef="room-1" roster={() => []} agents={() => []} threads={() => []} onExchangeSettled={settled} />
    ))
    await send('Hvordan bestiller vi pumper?')
    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-new' })
    flush()
    stream.handlers().onTitle?.({ title: 'Bestilling av pumper' })
    flush()

    await waitFor(() =>
      expect(spacesClient.updateSpaceThreadPresentation).toHaveBeenCalledWith('room-1', 't-new', {
        title: 'Bestilling av pumper',
      }),
    )
    // The room re-reads so every member's list shows the new name.
    await waitFor(() => expect(settled).toHaveBeenCalled())
  })

  // A reply continues someone's thread. It gets no title event (not a first
  // exchange) and the replier is not the owner — and even if a stray title
  // arrived, it must not be written onto a thread this browser did not open.
  it('never writes a title onto a thread it merely replied into', async () => {
    const stream = scriptedStream()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        replyTarget={() => ({ threadId: 't-existing', title: 'Innkjøp' })}
      />
    ))
    await send('Fortsett')
    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-existing' })
    flush()
    stream.handlers().onTitle?.({ title: 'Skulle ikke skje' })
    flush()
    await new Promise((resolve) => setTimeout(resolve, 20))

    expect(spacesClient.updateSpaceThreadPresentation).not.toHaveBeenCalled()
  })

  // A title is a label. A turn must never fail or shout over one.
  it('swallows a failed title write without disturbing the reply', async () => {
    const stream = scriptedStream()
    spacesClient.updateSpaceThreadPresentation.mockRejectedValue(new Error('owner only'))
    renderComposer()
    await send('Hei')
    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-new' })
    flush()
    stream.handlers().onTitle?.({ title: 'Hilsen' })
    flush()
    stream.handlers().onMessage?.({ content: 'Hei tilbake' })
    flush()

    await waitFor(() => expect(spacesClient.updateSpaceThreadPresentation).toHaveBeenCalled())
    expect(await screen.findByText('Hei tilbake')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('SpaceRoomComposer — `/` picks a skill (later tier)', () => {
  const catalogue = [
    { id: 'sk-innkjop', name: 'Innkjøpsrutine', description: 'Slik kjøper vi inn.', enabled: true, scope: 'org' },
    { id: 'sk-min', name: 'Min sjekkliste', description: 'Bare min.', enabled: true, scope: 'user' },
    { id: 'sk-hms', name: 'HMS-runde', description: '', enabled: true, scope: 'org' },
  ]

  beforeEach(() => {
    skillsClient.listAvailableSkills.mockReset()
    skillsClient.listAvailableSkills.mockResolvedValue(catalogue)
  })

  const textarea = () => screen.getByPlaceholderText(/velger en ferdighet/) as HTMLTextAreaElement

  it('fetches the catalogue on the first "/", not on mount, and badges each skill with its real scope', async () => {
    renderComposer()
    expect(skillsClient.listAvailableSkills).not.toHaveBeenCalled()

    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    expect(skillsClient.listAvailableSkills).toHaveBeenCalledTimes(1)

    const options = await screen.findAllByRole('option')
    expect(options.map((option) => option.textContent)).toEqual([
      'HMS-rundeOrganisasjon',
      'InnkjøpsrutineOrganisasjon',
      'Min sjekklistePersonlig',
    ])
    // No "this room" badge exists to give: the registry has no Space scope.
    expect(screen.queryByText(/rommet/i)).toBeNull()
  })

  it('filters by name, picks with the keyboard, and turns the pick into a chip instead of text', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: 'Se på dette /inn' } })
    flush()
    const option = await screen.findByRole('option', { name: /Innkjøpsrutine/ })
    expect(option.getAttribute('aria-selected')).toBe('true')

    fireEvent.keyDown(textarea(), { key: 'Enter' })
    flush()

    expect(textarea().value).toBe('Se på dette ')
    const chips = screen.getByRole('list', { name: 'Valgte ferdigheter' })
    expect(chips.textContent).toContain('Innkjøpsrutine')
    expect(chips.textContent).toContain('Organisasjon')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByText(/legges til som veiledning/)).toBeTruthy()
  })

  it('moves the highlight with the arrows and Escape closes without picking', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    await screen.findAllByRole('option')

    fireEvent.keyDown(textarea(), { key: 'ArrowDown' })
    flush()
    expect(screen.getByRole('option', { name: /Innkjøpsrutine/ }).getAttribute('aria-selected')).toBe('true')
    expect(textarea().getAttribute('aria-activedescendant')).toMatch(/-1$/)

    fireEvent.keyDown(textarea(), { key: 'Escape' })
    flush()
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.queryByRole('list', { name: 'Valgte ferdigheter' })).toBeNull()
    expect(textarea().value).toBe('/')
  })

  it('sends picked skills as actions of kind skill and clears them after the turn', async () => {
    const stream = scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/min' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /Min sjekkliste/ }))
    flush()
    fireEvent.input(textarea(), { target: { value: 'Gå gjennom lageret' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    flush()

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
    const request = chatClient.streamChat.mock.calls[0]?.[0] as { actions?: unknown[] }
    expect(request.actions).toEqual([{ id: 'sk-min', name: 'Min sjekkliste', kind: 'skill' }])
    expect(screen.queryByRole('list', { name: 'Valgte ferdigheter' })).toBeNull()
    stream.handlers().onDone?.({})
    stream.finish()
  })

  it('offers a remove control per chip and sends nothing when the chip is removed', async () => {
    scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/hms' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /HMS-runde/ }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Fjern ferdigheten HMS-runde' }))
    flush()
    expect(screen.queryByRole('list', { name: 'Valgte ferdigheter' })).toBeNull()

    fireEvent.input(textarea(), { target: { value: 'Hei' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
    const request = chatClient.streamChat.mock.calls[0]?.[0] as { actions?: unknown[] }
    expect(request.actions).toBeUndefined()
  })

  it('says so when the catalogue cannot be loaded, instead of an empty list that reads as "none"', async () => {
    skillsClient.listAvailableSkills.mockRejectedValue(new Error('down'))
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    expect(await screen.findByText(/kunne ikke hentes/)).toBeTruthy()
    expect(screen.queryByRole('option')).toBeNull()
  })

  it('does not list a skill twice once it is picked', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/hms' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /HMS-runde/ }))
    flush()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    const options = await screen.findAllByRole('option')
    expect(options.some((option) => /HMS-runde/.test(option.textContent ?? ''))).toBe(false)
  })
})

describe('SpaceRoomComposer — the spend hard stop is a refusal, not a cut-off reply', () => {
  const textarea = () => screen.getByPlaceholderText(/velger en ferdighet/) as HTMLTextAreaElement

  it('renders the spend hard stop in the room’s words with the way to the ceiling', async () => {
    const stream = scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: 'Tell lageret' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())

    stream.handlers().onError?.({ code: 'budget_exceeded', message: 'Budget exceeded' })
    stream.finish()
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Forbrukstaket er nådd')
    expect(alert.textContent).toContain('startet ikke')
    // Not "the reply stopped before it finished": nothing was replied.
    expect(alert.textContent).not.toContain('stoppet før det var ferdig')
    const link = screen.getByRole('link', { name: 'Åpne Forbrukstak' })
    expect(link.getAttribute('href')).toBe('/settings/quotas')
  })

  it('keeps the server’s message for a code the room has no words for', async () => {
    const stream = scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: 'Hei' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())

    stream.handlers().onError?.({ code: 'something_new', message: 'Upstream said no' })
    stream.finish()
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('stoppet før det var ferdig')
    expect(alert.textContent).toContain('Upstream said no')
    expect(screen.queryByRole('link', { name: 'Åpne Forbrukstak' })).toBeNull()
  })
})

describe('SpaceRoomComposer — the room\u2019s own typing line', () => {
  const textarea = () => screen.getByPlaceholderText(/velger en ferdighet/) as HTMLTextAreaElement

  it('renders what the page hands it, above the agent working line', async () => {
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        typingSentence={() => 'Kari skriver …'}
      />
    ))
    const line = await screen.findByRole('status')
    expect(line.textContent).toContain('Kari skriver …')
    expect(line.getAttribute('aria-live')).toBe('polite')
  })

  it('tells the page a member is writing, but not when the box is emptied', async () => {
    const typed = vi.fn()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        onTyping={typed}
      />
    ))
    fireEvent.input(textarea(), { target: { value: 'Hei' } })
    flush()
    expect(typed).toHaveBeenCalledTimes(1)

    fireEvent.input(textarea(), { target: { value: '   ' } })
    flush()
    expect(typed).toHaveBeenCalledTimes(1)
  })
})
