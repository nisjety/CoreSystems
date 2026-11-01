import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import { translateApiError } from '@/shared/i18n/errors'
import { CATALOGUE_TIER_WAIT_MS } from '@/features/dashboard/home/submit-privacy-tier'
import { publishSpaceThreads, resetLiveWorkForTests } from '../lib/space-live-work'
import { createSpaceSendJournal, type SpaceSendJournal } from '../lib/space-send-journal'

const chatClient = vi.hoisted(() => ({
  streamChat: vi.fn(),
  cancelInvocation: vi.fn(),
  listModels: vi.fn(),
}))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chat-client')>(),
  ...chatClient,
}))
const subscriptionsClient = vi.hoisted(() => ({ listChatGptSubscriptions: vi.fn() }))
vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>(),
  ...subscriptionsClient,
}))
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))
const spacesClient = vi.hoisted(() => ({
  updateSpaceThreadPresentation: vi.fn(),
  // WS01-S6 human posts.
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
  SPACE_POST_MAX_CHARS_DEFAULT: 8000,
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)
const skillsClient = vi.hoisted(() => ({ listAvailableSkills: vi.fn(), MAX_PICKED_SKILLS: 4 }))
vi.mock('@/shared/api/skills-client', () => skillsClient)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceRoomComposer } = await import('./SpaceRoomComposer')

type Handlers = {
  onRetrievalNotice?: (event: { code: string; wordingKey: string }) => void
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
  // DEPTH-SPA-2: SpaceConnectivityBanner (WS01-S8) writes a signal while it mounts, which Solid 2
  // refuses inside a component (REACTIVE_WRITE_IN_OWNED_SCOPE) and halts the reactive system for the
  // rest of the file. The banner has its own switch; these tests are about the send path, so keep it off.
  vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'false')
  chatClient.streamChat.mockReset()
  chatClient.cancelInvocation.mockReset()
  chatClient.cancelInvocation.mockResolvedValue(undefined)
  chatClient.listModels.mockReset()
  chatClient.listModels.mockResolvedValue([])
  subscriptionsClient.listChatGptSubscriptions.mockReset()
  subscriptionsClient.listChatGptSubscriptions.mockResolvedValue([])
  spacesClient.updateSpaceThreadPresentation.mockReset()
  spacesClient.createSpacePost.mockReset()
  spacesClient.replyInSpaceThread.mockReset()
  spacesClient.updateSpaceThreadPresentation.mockResolvedValue({ thread_id: 't-new' })
  resetLiveWorkForTests()
})

afterEach(() => {
  vi.unstubAllEnvs()
  cleanup()
  window.localStorage.clear()
})

describe('SpaceRoomComposer — declined retrieval note (D1-242)', () => {
  it('says why the answer does not use the knowledge, in the room words', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Hva selger vi?')
    stream.handlers().onConnected?.({ requestId: 'req-7' })
    stream.handlers().onRetrievalNotice?.({ code: 'retrieval_declined_entitlement_off', wordingKey: 'retrieval.declined.entitlement_off' })
    flush()
    stream.handlers().onMessage?.({ content: 'Jeg vet ikke.' })
    flush()

    const note = await screen.findByText(/har ikke slått på kunnskapssøk/)
    expect(note.getAttribute('role')).toBe('status')
  })

  it('shows no note for a reason outside the set, or when nothing was declined', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Hva selger vi?')
    stream.handlers().onRetrievalNotice?.({ code: 'retrieval_declined_from_the_future', wordingKey: 'x' })
    flush()
    stream.handlers().onMessage?.({ content: 'Svar.' })
    flush()

    expect(await screen.findByText('Svar.')).toBeTruthy()
    expect(screen.queryByText(/Svaret bruker ikke organisasjonens kunnskap/)).toBeNull()
  })
})

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

describe('SpaceRoomComposer — subscription model routing', () => {
  it('sends a plain room turn through the selected active subscription connection', async () => {
    chatClient.listModels.mockResolvedValue([{
      id: 'gpt-5.6-luna',
      name: 'GPT 5.6 Luna Subscription',
      provider: 'openai-codex-subscription',
    }])
    subscriptionsClient.listChatGptSubscriptions.mockResolvedValue([{
      id: 'conn-subscription-1',
      providerKey: 'openai-codex-subscription',
      status: 'active',
    }])
    const stream = scriptedStream()
    renderComposer()

    fireEvent.click(await screen.findByRole('button', { name: /choose ai model|velg ai-modell/i }))
    fireEvent.click(await screen.findByRole('button', { name: /gpt 5\.6 luna subscription/i }))
    await send('Use the room subscription')

    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      model: 'gpt-5.6-luna',
      provider: 'openai-codex-subscription',
      subscriptionConnectionId: 'conn-subscription-1',
      spaceRef: 'room-1',
    })
    stream.finish()
  })
})

// A room turn for a catalogue model carries the model's privacy tier, resolved
// exactly as the main composer resolves it (`submit-privacy-tier.ts`). Without
// it the turn reached the gateway with no `min_privacy_tier`: unconstrained.
describe('SpaceRoomComposer — a catalogue model’s privacy tier rides the turn', () => {
  const euModel = { id: 'm-eu', name: 'EU Model', provider: 'eu-host', privacyTier: 'eu_resident' as const }
  const plainModel = { id: 'm-plain', name: 'Plain Model', provider: 'plain-host' }

  /** The selection a previous page load stored, with or without its tier. */
  function storeSelection(privacyTier?: string, model: { id: string; name: string; provider: string } = euModel) {
    window.localStorage.setItem('verevon.ai-model-selection.v1:org-1', JSON.stringify({
      model: model.id,
      label: model.name,
      provider: model.provider,
      ...(privacyTier ? { privacyTier } : {}),
    }))
  }

  /** Every pending microtask has run: a held send would have gone out by now. */
  const settle = () => new Promise((resolve) => { setTimeout(resolve, 0) })

  function storedSelection(): Record<string, unknown> {
    return JSON.parse(window.localStorage.getItem('verevon.ai-model-selection.v1:org-1') ?? '{}') as Record<string, unknown>
  }

  /** Submit without waiting for the transport: the send may be held. */
  function submit(text: string) {
    const box = screen.getByRole('textbox')
    fireEvent.input(box, { target: { value: text } })
    flush()
    fireEvent.submit(box.closest('form') as HTMLFormElement)
  }

  const sentRequest = () => chatClient.streamChat.mock.calls[0]?.[0] as Record<string, unknown>

  afterEach(() => {
    vi.useRealTimers()
  })

  describe('before the catalogue has loaded', () => {
    it('holds the turn for the catalogue and sends the model’s own tier', async () => {
      storeSelection()
      let loadCatalogue!: (models: unknown[]) => void
      chatClient.listModels.mockReturnValue(new Promise((resolve) => { loadCatalogue = resolve }))
      const stream = scriptedStream()
      renderComposer()

      submit('Hei')
      await settle()
      expect(chatClient.streamChat).not.toHaveBeenCalled()

      loadCatalogue([euModel])
      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'eu_resident', content: 'Hei' })
      // One request, shared by the picker and the send.
      expect(chatClient.listModels).toHaveBeenCalledTimes(1)
      stream.finish()
    })

    it('sends the last-known tier once the bounded wait runs out', async () => {
      storeSelection('zdr_contractual')
      chatClient.listModels.mockReturnValue(new Promise(() => {}))
      const stream = scriptedStream()
      renderComposer()

      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      submit('Hei')
      await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS - 1)
      expect(chatClient.streamChat).not.toHaveBeenCalled()
      await vi.advanceTimersByTimeAsync(1)
      vi.useRealTimers()

      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'zdr_contractual' })
      stream.finish()
    })

    it('fails closed to the strictest tier when nothing is known about the model', async () => {
      storeSelection()
      chatClient.listModels.mockReturnValue(new Promise(() => {}))
      const stream = scriptedStream()
      renderComposer()

      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
      submit('Hei')
      await vi.advanceTimersByTimeAsync(CATALOGUE_TIER_WAIT_MS)
      vi.useRealTimers()

      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'sovereign' })
      stream.finish()
    })

    it('sends nothing when Stop lands while the turn waits for the catalogue', async () => {
      storeSelection()
      let loadCatalogue!: (models: unknown[]) => void
      chatClient.listModels.mockReturnValue(new Promise((resolve) => { loadCatalogue = resolve }))
      scriptedStream()
      renderComposer()

      submit('Hei')
      ;(await screen.findByRole('button', { name: 'Stopp' })).click()
      loadCatalogue([euModel])

      expect(await screen.findByText(/Avbrutt før svaret startet/)).toBeTruthy()
      await settle()
      expect(chatClient.streamChat).not.toHaveBeenCalled()
    })

    it('sends an intent mode at once and with no tier, as before', async () => {
      // Default selection: Verevon Balance, resolved server-side.
      chatClient.listModels.mockReturnValue(new Promise(() => {}))
      const stream = scriptedStream()
      renderComposer()

      await send('Hei')
      expect(sentRequest().minPrivacyTier).toBeUndefined()
      stream.finish()
    })
  })

  describe('after the catalogue has loaded', () => {
    it('sends the listed tier, even over a different last-known one', async () => {
      storeSelection('global')
      chatClient.listModels.mockResolvedValue([euModel, plainModel])
      const stream = scriptedStream()
      renderComposer()
      // The picker writes the listed tier once the catalogue has loaded.
      await waitFor(() => expect(storedSelection().privacyTier).toBe('eu_resident'))

      await send('Hei')
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'eu_resident' })
      expect(chatClient.listModels).toHaveBeenCalledTimes(1)
      stream.finish()
    })

    it('sends no tier for a listed model that claims none, as before', async () => {
      storeSelection(undefined, plainModel)
      chatClient.listModels.mockResolvedValue([euModel, plainModel])
      const stream = scriptedStream()
      renderComposer()
      await waitFor(() => expect(storedSelection().privacyTier).toBe('unspecified'))

      await send('Hei')
      expect(sentRequest()).toMatchObject({ model: 'm-plain', content: 'Hei' })
      expect(sentRequest().minPrivacyTier).toBeUndefined()
      stream.finish()
    })

    it('stores the listed tier as the last-known one, for a later slow catalogue', async () => {
      storeSelection()
      chatClient.listModels.mockResolvedValue([euModel, plainModel])
      renderComposer()

      await waitFor(() => expect(storedSelection()).toMatchObject({ model: 'm-eu', privacyTier: 'eu_resident' }))
      // Picking a model stores its tier too, "claims none" included.
      fireEvent.click(await screen.findByRole('button', { name: /velg ai-modell/i }))
      fireEvent.click(await screen.findByRole('button', { name: /plain model/i }))
      expect(storedSelection()).toMatchObject({ model: 'm-plain', privacyTier: 'unspecified' })
    })
  })

  describe('when the catalogue request fails', () => {
    it('sends the last-known tier without waiting', async () => {
      storeSelection('global')
      chatClient.listModels.mockRejectedValue(new Error('Catalog unavailable'))
      const stream = scriptedStream()
      renderComposer()

      // waitFor gives up after 1 s, well inside the 3 s bound: no wait happened.
      await send('Hei')
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'global' })
      stream.finish()
    })

    it('fails closed when nothing is known about the model', async () => {
      storeSelection()
      chatClient.listModels.mockRejectedValue(new Error('Catalog unavailable'))
      const stream = scriptedStream()
      renderComposer()

      await send('Hei')
      expect(sentRequest()).toMatchObject({ model: 'm-eu', minPrivacyTier: 'sovereign' })
      stream.finish()
    })
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

  // WS03-S7: a typed Space denial error event shows the presenter's words, never event.message.
  it('shows the denial copy for a typed Space denial, not the event message', async () => {
    const stream = scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: 'Hei' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())

    const onError = stream.handlers().onError as unknown as (event: Record<string, unknown>) => void
    onError({
      code: 'forbidden',
      message: 'Server internal wording',
      details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
    })
    stream.finish()
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Rollen din i dette rommet er ikke høy nok til dette.')
    expect(alert.textContent).toContain('Be en romleder om hjelp.')
    expect(alert.textContent).not.toContain('Server internal wording')
  })

  it('treats an unknown typed reason as the generic denial, not the server text', async () => {
    const stream = scriptedStream()
    renderComposer()
    fireEvent.input(textarea(), { target: { value: 'Hei' } })
    flush()
    fireEvent.submit(textarea().closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalled())

    const onError = stream.handlers().onError as unknown as (event: Record<string, unknown>) => void
    onError({ code: 'forbidden', message: 'Server internal wording', details: { reason: 'space_future_reason' } })
    stream.finish()
    flush()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Dette er ikke tillatt i rommet akkurat nå.')
    expect(alert.textContent).not.toContain('Server internal wording')
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

// WS01-S6: human posts. Nothing in these tests calls a model: the transport is the mocked
// createSpacePost / replyInSpaceThread, and the Ask cases only check that streamChat is reached.
describe('SpaceRoomComposer — human posts (WS01-S6)', () => {
  const receipt = { threadId: 'thr_new', messageId: 'msg_1', sequence: 1, deduplicated: false }
  const agent = {
    subject_id: 'agent-1',
    role: 'editor',
    revision: 1,
    identity_published: true,
    name: 'Driftsassistent',
    status: 'active',
    definition_status: 'published',
    delivery_targets: [],
  }
  const skillCatalogue = [{ id: 'skill-1', name: 'Innkjøpsrutine', description: '', scope: 'org' }]

  type PostProps = {
    on?: boolean
    role?: string
    viewer?: string
    maxChars?: number
    replyTarget?: { threadId: string; title: string }
    threads?: readonly unknown[]
    agents?: readonly unknown[]
    defaultSendMode?: 'post' | 'ask'
  }

  // DEPTH-SPA-1: the post sender reads the journal straight after writing it, and Solid 2 shows a
  // write only after the next flush, so with the default journal the request never leaves the
  // composer (the entry sits at `sending`). The cases below hand the composer a journal that flushes
  // after each write so they test the composer's own decisions; the unflushed default is the one
  // skipped case at the end of the file.
  function flushingJournal(): SpaceSendJournal {
    const inner = createSpaceSendJournal()
    const settled = <A extends unknown[], R>(fn: (...args: A) => R) => (...args: A): R => {
      const result = fn(...args)
      flush()
      return result
    }
    return {
      ...inner,
      begin: settled(inner.begin),
      update: settled(inner.update),
      remove: settled(inner.remove),
      reconcile: settled(inner.reconcile),
    }
  }

  const renderPosts = (props: PostProps = {}) =>
    render(() => (
      <SpaceRoomComposer
        journal={flushingJournal()}
        spaceRef="room-1"
        roster={() => []}
        agents={() => (props.agents ?? []) as never}
        threads={() => (props.threads ?? []) as never}
        humanPostsEnabled={() => props.on !== false}
        viewerRole={() => props.role ?? 'editor'}
        viewerSubjectId={() => props.viewer ?? 'user-me'}
        postMaxChars={props.maxChars === undefined ? undefined : () => props.maxChars}
        replyTarget={props.replyTarget ? () => props.replyTarget : undefined}
        defaultSendMode={props.defaultSendMode}
      />
    ))

  const box = () => screen.getByRole('textbox') as HTMLTextAreaElement
  const type = (value: string) => {
    fireEvent.input(box(), { target: { value } })
    flush()
  }
  const submit = () => fireEvent.submit(box().closest('form') as HTMLFormElement)

  beforeEach(() => {
    skillsClient.listAvailableSkills.mockReset()
    skillsClient.listAvailableSkills.mockResolvedValue(skillCatalogue)
    spacesClient.createSpacePost.mockResolvedValue(receipt)
    spacesClient.replyInSpaceThread.mockResolvedValue({ ...receipt, threadId: 'thr_7' })
  })

  describe('with features.human_posts off', () => {
    it('is the single ask composer it always was: no mode switch, Send streams, nothing is posted', async () => {
      scriptedStream()
      renderPosts({ on: false })
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
      await send('Rydd lageret')
      expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
      expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    })

    it('ignores a viewer role: the flag decides whether roles matter at all', () => {
      renderPosts({ on: false, role: 'viewer' })
      expect((box()).disabled).toBe(false)
      expect(screen.queryByText(/lesetilgang/)).toBeNull()
    })
  })

  describe('with it on', () => {
    it('offers Send and Ask as a switch, defaulting to Send', () => {
      renderPosts()
      const radios = screen.getAllByRole('radio')
      expect(radios.map((radio) => radio.textContent)).toEqual(['Send til rommet', 'Spør Verevon'])
      expect(screen.getByRole('radio', { name: 'Send til rommet' }).getAttribute('aria-checked')).toBe('true')
      expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
    })

    it('honours defaultSendMode ask', () => {
      renderPosts({ defaultSendMode: 'ask' })
      expect(screen.getByRole('radio', { name: 'Spør Verevon' }).getAttribute('aria-checked')).toBe('true')
      expect(screen.getByRole('button', { name: 'Spør Verevon' })).toBeTruthy()
    })

    it('posts through createSpacePost with a client send id and never calls streamChat', async () => {
      renderPosts()
      type('  Hei alle  ')
      submit()
      await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
      const [spaceRef, input] = spacesClient.createSpacePost.mock.calls[0] as [string, { content: string; clientSendId: string }]
      expect(spaceRef).toBe('room-1')
      expect(input.content).toBe('Hei alle')
      expect(input.clientSendId).toMatch(/^[A-Za-z0-9_-]{16,64}$/)
      expect(chatClient.streamChat).not.toHaveBeenCalled()
      expect(spacesClient.replyInSpaceThread).not.toHaveBeenCalled()
    })

    it('clears the text only on acceptance', async () => {
      let accept: (value: typeof receipt) => void = () => undefined
      spacesClient.createSpacePost.mockImplementationOnce(() => new Promise((resolve) => { accept = resolve }))
      renderPosts()
      type('Hei alle')
      submit()
      await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalled())
      // The request is out and unanswered: the words are still in the box.
      expect(box().value).toBe('Hei alle')
      accept(receipt)
      await waitFor(() => expect(box().value).toBe(''))
    })

    it('replies through replyInSpaceThread when a reply target is set', async () => {
      renderPosts({ replyTarget: { threadId: 'thr_7', title: 'Innkjøp' } })
      type('Enig')
      submit()
      await waitFor(() => expect(spacesClient.replyInSpaceThread).toHaveBeenCalledTimes(1))
      const [spaceRef, threadId, input] = spacesClient.replyInSpaceThread.mock.calls[0] as [string, string, { content: string }]
      expect([spaceRef, threadId, input.content]).toEqual(['room-1', 'thr_7', 'Enig'])
      expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
      expect(chatClient.streamChat).not.toHaveBeenCalled()
    })

    it('streams when the Ask switch is chosen, and does not post', async () => {
      scriptedStream()
      renderPosts()
      fireEvent.click(screen.getByRole('radio', { name: 'Spør Verevon' }))
      flush()
      expect(screen.getByRole('button', { name: 'Spør Verevon' })).toBeTruthy()
      type('Hva er status?')
      submit()
      await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
      expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    })
  })

  describe('what forces Ask', () => {
    it('a picked skill forces Ask and hides the switch', async () => {
      renderPosts()
      type('Se på dette /inn')
      await screen.findByRole('option', { name: /Innkjøpsrutine/ })
      fireEvent.keyDown(box(), { key: 'Enter' })
      flush()
      expect(screen.getByRole('list', { name: 'Valgte ferdigheter' })).toBeTruthy()
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Spør Verevon' })).toBeTruthy()
    })

    it('a mentioned agent forces Ask and hides the switch', async () => {
      renderPosts({ agents: [agent] })
      type('@Drif')
      fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
      flush()
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Spør Verevon' })).toBeTruthy()
    })
  })

  describe('a reply in someone else’s thread', () => {
    const threads = [{ thread_id: 'thr_7', space_id: 'room-1', owner_subject_id: 'user-other' }]

    it('offers no Ask: there is nothing to switch', () => {
      renderPosts({ replyTarget: { threadId: 'thr_7', title: 'Innkjøp' }, threads })
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
    })

    it('offers Ask again in a thread the member started', () => {
      renderPosts({
        replyTarget: { threadId: 'thr_7', title: 'Innkjøp' },
        threads: [{ thread_id: 'thr_7', space_id: 'room-1', owner_subject_id: 'user-me' }],
      })
      expect(screen.getAllByRole('radio')).toHaveLength(2)
    })

    it('says so when an agent mention would have started the agent there', async () => {
      renderPosts({ replyTarget: { threadId: 'thr_7', title: 'Innkjøp' }, threads, agents: [agent] })
      type('@Drif')
      fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
      flush()
      expect(screen.getByText(/Du kan ikke starte agenten i en annens samtale/)).toBeTruthy()
      // Nothing is offered, so the button is the (disabled) ask label.
      expect((screen.getByRole('button', { name: 'Spør Verevon' }) as HTMLButtonElement).disabled).toBe(true)
    })
  })

  describe('a viewer', () => {
    it('gets a disabled box and the reason, and cannot send', () => {
      renderPosts({ role: 'viewer' })
      expect(box().disabled).toBe(true)
      expect(screen.getByText(/lesetilgang i dette rommet/)).toBeTruthy()
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect((screen.getByRole('button', { name: /Send|Spør Verevon/ }) as HTMLButtonElement).disabled).toBe(true)
    })

    it('treats a role the SPA does not know as read-only too', () => {
      renderPosts({ role: undefined as never })
      // `undefined` falls back to editor in the harness; pass an unknown role explicitly.
      cleanup()
      renderPosts({ role: 'guest' })
      expect(box().disabled).toBe(true)
    })
  })

  describe('failures', () => {
    it('keeps the text on failure and offers Try again and Discard', async () => {
      spacesClient.createSpacePost.mockRejectedValueOnce(new ApiError('down', 503, 'session_unavailable'))
      renderPosts()
      type('Hei alle')
      submit()
      const alert = await screen.findByRole('alert')
      expect(alert.textContent).toContain('Meldingen ble ikke sendt')
      expect(within(alert).getByRole('button', { name: 'Prøv igjen' })).toBeTruthy()
      expect(within(alert).getByRole('button', { name: 'Forkast' })).toBeTruthy()
      expect(box().value).toBe('Hei alle')
    })

    it('Try again repeats the same send id and clears the text once accepted', async () => {
      spacesClient.createSpacePost.mockRejectedValueOnce(new TypeError('Failed to fetch'))
      renderPosts()
      type('Hei alle')
      submit()
      fireEvent.click(await screen.findByRole('button', { name: 'Prøv igjen' }))
      await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(2))
      const ids = spacesClient.createSpacePost.mock.calls.map((call) => (call[1] as { clientSendId: string }).clientSendId)
      expect(ids[1]).toBe(ids[0])
      await waitFor(() => expect(box().value).toBe(''))
    })

    it('offers no Try again for a refusal that cannot succeed, only Discard', async () => {
      spacesClient.createSpacePost.mockRejectedValueOnce(new ApiError('no', 403, 'space_post_not_authorized'))
      renderPosts()
      type('Hei alle')
      submit()
      const alert = await screen.findByRole('alert')
      expect(within(alert).queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
      fireEvent.click(within(alert).getByRole('button', { name: 'Forkast' }))
      await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
      // Discard keeps the words in the box and sends nothing more.
      expect(box().value).toBe('Hei alle')
      expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1)
    })

    it('shows the zero-retention catalogue text for the typed 412 refusal and removes Send', async () => {
      spacesClient.createSpacePost.mockRejectedValueOnce(new ApiError('zdr', 412, 'zdr_durable_mutation_forbidden'))
      renderPosts()
      type('Hei alle')
      submit()
      await waitFor(() => expect(screen.queryByRole('radio', { name: 'Send til rommet' })).toBeNull())
      // The catalogue's own words for the refusal, in a status notice above the box.
      const catalogueText = translateApiError({ code: 'zdr_durable_mutation_forbidden' }, (no) => no)
      expect(catalogueText).not.toBe('')
      expect(screen.getByRole('status', { name: '' }).textContent ?? '').toContain(catalogueText)
      expect(screen.queryByRole('radiogroup')).toBeNull()
      expect(screen.getByRole('button', { name: 'Spør Verevon' })).toBeTruthy()
    })
  })

  it('DEPTH-SPA-1: with the default journal a post leaves the composer without an explicit flush', async () => {
    cleanup()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        humanPostsEnabled={() => true}
        viewerRole={() => 'editor'}
      />
    ))
    type('Hei alle')
    submit()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
  })

  describe('the length bound', () => {
    it('blocks Send over the limit with the counter, and sends nothing', () => {
      renderPosts({ maxChars: 10 })
      type('x'.repeat(11))
      const counter = screen.getByRole('alert')
      expect(counter.textContent).toContain('11 / 10')
      expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
      expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    })

    it('shows a quiet counter near the limit and allows Send at the limit', () => {
      renderPosts({ maxChars: 10 })
      type('x'.repeat(9))
      expect(screen.queryByRole('alert')).toBeNull()
      expect(screen.getByText('9 / 10')).toBeTruthy()
      expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(false)
    })

    it('counts code points, not UTF-16 units', () => {
      renderPosts({ maxChars: 4 })
      type('😀😀😀😀')
      expect(screen.queryByRole('alert')).toBeNull()
      expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(false)
    })
  })
})

describe('SpaceRoomComposer — reliable send on the agent path (WS01-S8)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', 'true')
  })

  it('offers Try again for a dropped connection and repeats the send under the same key', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onError?.({ code: 'connection_error', message: 'x' })
    stream.finish()
    const retry = await screen.findByRole('button', { name: 'Prøv igjen' })

    // The repeat goes out as the same send: same idempotency key, same session key.
    const first = chatClient.streamChat.mock.calls[0]?.[0] as { clientMessageId?: string; sessionKey?: string }
    expect(first.clientMessageId).toMatch(/^[0-9a-f]{32}$/)
    expect(first.sessionKey).toBe(`space-post-${first.clientMessageId}`)

    scriptedStream()
    fireEvent.click(retry)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(2))
    const second = chatClient.streamChat.mock.calls[1]?.[0] as { clientMessageId?: string; sessionKey?: string }
    expect(second.clientMessageId).toBe(first.clientMessageId)
    expect(second.sessionKey).toBe(first.sessionKey)
  })

  it('offers no Try again for an answer the server gave', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onError?.({ code: 'spend_hard_stop', message: 'x' })
    stream.finish()
    await screen.findByRole('alert')
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })

  it('says a send the server already had was received, and shows no error', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onError?.({ code: 'managed_run_already_started', message: 'x' })
    stream.finish()
    expect(await screen.findByText('Meldingen var allerede mottatt. Svaret vises i rommet.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })

  it('with the send key off a dropped connection is a plain error with no Try again', async () => {
    vi.stubEnv('VITE_SPACE_SEND_IDEMPOTENCY_API', 'false')
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    const request = chatClient.streamChat.mock.calls[0]?.[0] as { clientMessageId?: string; sessionKey?: string }
    expect(request.clientMessageId).toBeUndefined()
    expect(request.sessionKey).toBeUndefined()
    stream.handlers().onError?.({ code: 'connection_error', message: 'x' })
    stream.finish()
    await screen.findByRole('alert')
    expect(screen.queryByRole('button', { name: 'Prøv igjen' })).toBeNull()
  })
})

describe('SpaceRoomComposer — a fixed reply target (WS01-S7b)', () => {
  const renderWithTarget = (fixed: boolean | undefined) =>
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        replyTarget={() => ({ threadId: 'thr_7', title: 'Innkjøp' })}
        onClearReplyTarget={() => undefined}
        replyTargetFixed={fixed}
      />
    ))

  it('hides the cancel-reply button when the target is the surface itself (the thread pane)', () => {
    renderWithTarget(true)
    expect(screen.queryByRole('button', { name: 'Avbryt svar' })).toBeNull()
  })

  // The pane title names the thread, so the pane's box does not repeat it in a chip and does not talk
  // about "the room" (it is a reply in the conversation).
  it('in the thread pane the box talks about the conversation, with no "Svarer i" chip', () => {
    renderWithTarget(true)
    expect(screen.queryByText(/Svarer i/)).toBeNull()
    expect(screen.getByRole('textbox').getAttribute('placeholder')).toMatch(/^Svar i samtalen/)
    expect(screen.getByRole('textbox').getAttribute('placeholder')).not.toMatch(/rommet/)
  })

  it('with human posts on, the pane offers "Svar i samtalen" where the room says "Send til rommet"', () => {
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => [{ thread_id: 'thr_7', space_id: 'room-1', owner_subject_id: 'user-me' }] as never}
        humanPostsEnabled={() => true}
        viewerRole={() => 'editor'}
        viewerSubjectId={() => 'user-me'}
        replyTarget={() => ({ threadId: 'thr_7', title: 'Innkjøp' })}
        replyTargetFixed
      />
    ))
    expect(screen.getAllByRole('radio').map((radio) => radio.textContent)).toEqual(['Svar i samtalen', 'Spør Verevon'])
    expect(screen.getByRole('textbox').getAttribute('placeholder')).toMatch(/^Svar i samtalen\. Agenten svarer ikke/)
  })

  it('keeps the cancel-reply button for the room composer', () => {
    renderWithTarget(undefined)
    expect(screen.getByText(/Svarer i/)).toBeTruthy()
    expect(screen.getByRole('textbox').getAttribute('placeholder')).not.toMatch(/Svar i samtalen/)
    expect(screen.getByRole('button', { name: 'Avbryt svar' })).toBeTruthy()
    cleanup()
    renderWithTarget(false)
    expect(screen.getByRole('button', { name: 'Avbryt svar' })).toBeTruthy()
  })
})
