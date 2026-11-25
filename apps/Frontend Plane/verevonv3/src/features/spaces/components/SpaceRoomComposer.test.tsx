import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, For, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { reloadGuardCount } from '@/app/chunk-recovery'
import { ApiError } from '@/shared/api/http'
import { hasUnsavedWork, hasUnsavedWorkProbes } from '@/shared/build/unsaved-work'
import { translateApiError } from '@/shared/i18n/errors'
import { CATALOGUE_TIER_WAIT_MS } from '@/features/dashboard/home/submit-privacy-tier'
import type { SpaceRunLiveHandlers } from '@/shared/api/space-run-live-client'
import { publishSpaceThreads, resetLiveWorkForTests } from '../lib/space-live-work'
import { spaceRunStore } from '../lib/space-run-store'
import { createSpaceSendJournal, type SpaceSendJournal } from '../lib/space-send-journal'
import { applyServerRetention, __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { HOME_DRAFT_DEBOUNCE_MS } from '@/features/dashboard/home/home-draft'
import { clearSpaceRoomFeatures, publishSpaceRoomFeatures } from '../lib/space-room-features'
import { spaceWakeBoard } from '../lib/wake-sequencer'
import { setCommandPaletteFeature, setMessageSearchOpener } from '@/features/command/command-registry'

const chatClient = vi.hoisted(() => ({
  streamChat: vi.fn(),
  cancelInvocation: vi.fn(),
  listModels: vi.fn(),
  // WS09-S2: a steer into the running turn.
  queueInvocationInput: vi.fn(),
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
  // The user id keys the room's drafts (WS15-S2); the org keys the rest.
  getSession: () => ({ user: { id: 'user-kari' }, activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))
const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  updateSpaceThreadPresentation: vi.fn(),
  // WS01-S6 human posts.
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
  SPACE_POST_MAX_CHARS_DEFAULT: 8000,
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
// WS14-S9: the durable upload of a turn's chosen text files. The cases read the call; nothing leaves.
const spaceFilesClient = vi.hoisted(() => ({ uploadSpaceFiles: vi.fn() }))
vi.mock('@/shared/api/space-files-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/space-files-client')>()),
  ...spaceFilesClient,
}))
// WS09-S2: the live stream of the run a send-when-ready waits behind; the test drives its frames.
const runLive = vi.hoisted(() => ({ streamSpaceRunLive: vi.fn() }))
vi.mock('@/shared/api/space-run-live-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/space-run-live-client')>(),
  ...runLive,
}))
const skillsClient = vi.hoisted(() => ({ listAvailableSkills: vi.fn(), MAX_PICKED_SKILLS: 4 }))
vi.mock('@/shared/api/skills-client', () => skillsClient)
// WS13-S7: `/read` runs the shared mark-read action; the action itself is faked, the composer's part is tested.
const paletteProviders = vi.hoisted(() => ({ runPaletteAction: vi.fn() }))
vi.mock('@/features/command/palette-providers', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/features/command/palette-providers')>(),
  ...paletteProviders,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
  // The mid-run strip (WS09-S2). A stand-in that shows each entry's words, state and note: the chat's
  // own strip is tested in its own file, and these tests check what the composer hands it.
  QueuedInputStrip: (props: { entries: readonly { id: string; content: string; state: string; note?: string }[] }) => (
    <ul aria-label="Meldinger sendt underveis">
      <For each={props.entries}>
        {(entry) => (
          <li data-state={entry.state}>
            <span>{entry.content}</span>
            {entry.note ? <span>{entry.note}</span> : null}
          </li>
        )}
      </For>
    </ul>
  ),
}))

const { SpaceRoomComposer } = await import('./SpaceRoomComposer')

type Handlers = {
  onRetrievalNotice?: (event: { code: string; wordingKey: string }) => void
  onConnected?: (event: { requestId?: string; threadId?: string }) => void
  onTitle?: (event: { title: string }) => void
  onMessage?: (event: { content: string }) => void
  onDone?: (event: Record<string, never>) => void
  onStopped?: (event: Record<string, never>) => void
  onError?: (event: { code: string; message: string; feature?: string }) => void
  onQueuedInput?: (event: { messages: string[]; ids?: string[] }) => void
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
  chatClient.queueInvocationInput.mockReset()
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
  runLive.streamSpaceRunLive.mockReset()
  runLive.streamSpaceRunLive.mockImplementation(() => new Promise<void>(() => {}))
})

afterEach(() => {
  vi.unstubAllEnvs()
  cleanup()
  spaceRunStore.dispose()
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

  // WS09-S8: a `stopped` frame this browser did not ask for is a room manager's stop, and the copy says so.
  it('reads a stop this browser did not ask for as stopped by a room manager', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-42' })
    flush()
    stream.handlers().onStopped?.({})
    flush()
    stream.finish()

    expect(await screen.findByText('Kjøringen ble stoppet av en rombehandler. Rommet ser dette som stoppet.')).toBeTruthy()
    expect(screen.queryByText(/Stoppet av deg/)).toBeNull()
    expect(chatClient.cancelInvocation).not.toHaveBeenCalled()
  })

  it('reads a stop this browser made as stopped by you once the server has recorded it', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-42' })
    flush()

    screen.getByRole('button', { name: 'Stopp' }).click()

    expect(await screen.findByText('Stoppet av deg. Rommet ser dette som stoppet.')).toBeTruthy()
    expect(screen.queryByText(/rombehandler/)).toBeNull()
    expect(chatClient.cancelInvocation).toHaveBeenCalledWith('req-42')
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

  // WS09-S6: with the room's live view on, the run strip replaces the sentence once a run shows its work.
  it('with agent_work.live on, the run strip replaces the working sentence once a run shows its work', async () => {
    const frames: { onFrame?: SpaceRunLiveHandlers['onFrame'] } = {}
    runLive.streamSpaceRunLive.mockImplementation((_spaceRef: string, runId: string, handlers: SpaceRunLiveHandlers) => {
      if (runId === 'run-live-7') frames.onFrame = handlers.onFrame
      return new Promise<void>(() => {})
    })
    publishSpaceThreads('room-1', [
      { thread_id: 't9', space_id: 'room-1', title: 'Innkjøp', latest_run_id: 'run-live-7', latest_run_status: 'running' },
    ])
    flush()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        agentWork={() => ({ live: true, text: false, steer_owner: false, stop_manager: false }) as never}
      />
    ))
    await waitFor(() => expect(frames.onFrame).toBeDefined())
    frames.onFrame?.({ type: 'step', step: { id: 'step-1', class: 'web', state: 'running' } }, undefined)
    flush()

    expect(await screen.findByText('Søker på nettet', {}, { timeout: 3000 })).toBeTruthy()
    expect(document.querySelector('.verevon-run-strip')).not.toBeNull()
    expect(screen.queryByText(/Verevon jobber i/)).toBeNull()
  })

  it('with agent_work.live off, the plain working sentence stays and no strip is drawn', () => {
    publishSpaceThreads('room-1', [
      { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_id: 'run-9', latest_run_status: 'running' },
    ])
    flush()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        agentWork={() => ({ live: false, text: false, steer_owner: false, stop_manager: false }) as never}
      />
    ))
    expect(screen.getByText(/Verevon jobber i «Innkjøp»/)).toBeTruthy()
    expect(document.querySelector('.verevon-run-strip')).toBeNull()
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
    // The platform commands (WS13-S7) have their own describe; here the list is the skills alone.
    setCommandPaletteFeature(false)
    skillsClient.listAvailableSkills.mockReset()
    skillsClient.listAvailableSkills.mockResolvedValue(catalogue)
  })

  afterEach(() => {
    setCommandPaletteFeature(undefined)
  })

  const textarea = () => screen.getByPlaceholderText(/velger en ferdighet/) as HTMLTextAreaElement

  it('the picker is named for skills alone while the platform commands are off', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()

    expect(await screen.findByRole('listbox', { name: 'Velg en ferdighet' })).toBeTruthy()
  })

  it('fetches the catalogue on the first "/", not on mount, and badges each skill with its real scope', async () => {
    renderComposer()
    expect(skillsClient.listAvailableSkills).not.toHaveBeenCalled()

    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    expect(skillsClient.listAvailableSkills).toHaveBeenCalledTimes(1)

    // The skills follow once their catalogue has loaded (the commands are off in this describe).
    await screen.findByRole('option', { name: /HMS-runde/ })
    const labels = screen.getAllByRole('option').map((option) => option.textContent ?? '')
    expect(labels).toEqual(
      expect.arrayContaining(['HMS-rundeOrganisasjon', 'InnkjøpsrutineOrganisasjon', 'Min sjekklistePersonlig']),
    )
    // No "this room" badge exists to give: the registry has no Space scope. The platform command hints
    // legitimately say "rommet", so only the skill rows are checked.
    const skillRows = screen
      .getAllByRole('option')
      .filter((option) => /HMS-runde|Innkjøpsrutine|sjekkliste/.test(option.textContent ?? ''))
    expect(skillRows).toHaveLength(3)
    for (const row of skillRows) expect(row.textContent).not.toMatch(/rommet/i)
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
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true')
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
    expect(screen.queryByRole('option', { name: /Innkjøpsrutine|HMS-runde|sjekkliste/ })).toBeNull()
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

describe('SpaceRoomComposer — a zero-retention refusal names its feature', () => {
  // The refusal names the feature by its key; the room words it, in the reader's language.
  it('says which feature the refusal is about', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('Søk etter noe')

    stream.handlers().onError?.({
      code: 'zdr_feature_not_supported',
      message: 'This is not supported with zero data retention yet: web search and browsing.',
      feature: 'web_search',
    })

    await waitFor(() => expect(document.body.textContent).toMatch(/websøk og nettlesing|web search and browsing/))
    expect(document.body.textContent).not.toMatch(/this feature/)
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
    canAsk?: boolean
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
        canAsk={props.canAsk === undefined ? undefined : () => props.canAsk === true}
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

    // WS12-S3: a direct message is people only, so the page passes canAsk false.
    it('offers Send only, with no mode switch, when canAsk is false (a direct message)', async () => {
      renderPosts({ canAsk: false })
      expect(screen.queryByRole('radio', { name: 'Spør Verevon' })).toBeNull()
      expect(screen.queryByRole('button', { name: 'Spør Verevon' })).toBeNull()
      expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
      type('Hei')
      submit()
      await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
      expect(chatClient.streamChat).not.toHaveBeenCalled()
    })

    // WS12-S3: a default of ask must not bring Ask back in a direct message.
    it('a default of ask cannot bring Ask back when canAsk is false', () => {
      renderPosts({ canAsk: false, defaultSendMode: 'ask' })
      expect(screen.queryByRole('button', { name: 'Spør Verevon' })).toBeNull()
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
      expect(screen.queryByRole('button', { name: 'Send' })).toBeNull()
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

describe('SpaceRoomComposer — owner mid-run steer (WS09-S2)', () => {
  const STEER_ON = { live: false, text: false, steer_owner: true, stop_manager: false }

  // A reply into thread t1, the room's own composer: where a member steers into a turn or waits for one.
  const renderInThread = (agentWork?: unknown) =>
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        replyTarget={() => ({ threadId: 't1', title: 'Innkjøp' })}
        agentWork={() => agentWork as never}
      />
    ))

  // Types and submits. Synchronous on purpose: the caller waits for whatever the send does next.
  function typeAndSubmit(value: string): void {
    const box = screen.getByRole('textbox')
    fireEvent.input(box, { target: { value } })
    flush()
    fireEvent.submit(box.closest('form') as HTMLFormElement)
  }

  // The state of the strip entry that shows these words.
  const stripState = (words: string) => screen.getByText(words).closest('[data-state]')?.getAttribute('data-state')

  // The member's own turn has started and its stream has announced the run and the thread.
  async function ownTurnStreaming(stream: ReturnType<typeof scriptedStream>): Promise<void> {
    await send('Rydd lageret')
    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't1' })
    flush()
  }

  // Steer (WS09-S2): the member's own turn stays open to a message, and the gateway takes it.
  it('keeps the box open during the member\'s own turn when the room allows steering', async () => {
    const stream = scriptedStream()
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(false)
    expect(screen.getByText(/Agenten jobber\. Det du skriver nå leveres/)).toBeTruthy()
  })

  it('disables the box during the own turn when the room does not allow steering, as before', async () => {
    const stream = scriptedStream()
    renderInThread()
    await ownTurnStreaming(stream)

    expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(true)
    expect(screen.queryByText(/Agenten jobber\. Det du skriver/)).toBeNull()
  })

  it('hands a message written mid-run to the running turn, aimed at its thread and room, and clears the box once accepted', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({ outcome: 'queued', pending: 1, persisted: true })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Bruk gul ledning')

    await waitFor(() => expect(chatClient.queueInvocationInput).toHaveBeenCalledWith(
      'req-1',
      'Bruk gul ledning',
      expect.objectContaining({ threadId: 't1', spaceRef: 'room-1', clientMessageId: expect.any(String) }),
    ))
    await waitFor(() => expect(stripState('Bruk gul ledning')).toBe('pending'))
    await waitFor(() => expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe(''))
    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
  })

  it('keeps the words in the box and says why when the gateway refuses the steer', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({
      outcome: 'refused',
      reason: 'too_many',
      message: 'Tre meldinger venter allerede.',
    })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Ny instruks')

    await waitFor(() => expect(stripState('Ny instruks')).toBe('refused'))
    expect(screen.getByText('Tre meldinger venter allerede.')).toBeTruthy()
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Ny instruks')
  })

  it('refuses a steer typed before the stream announced its run, with no gateway call', async () => {
    scriptedStream()
    renderInThread(STEER_ON)
    await send('Rydd lageret')

    typeAndSubmit('For tidlig')

    await waitFor(() => expect(stripState('For tidlig')).toBe('refused'))
    expect(screen.getByText(/har ikke startet ennå/)).toBeTruthy()
    expect(chatClient.queueInvocationInput).not.toHaveBeenCalled()
    expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('For tidlig')
  })

  it('shows a steered message as delivered once the agent takes it, and sends nothing again when the run finishes', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({ outcome: 'queued', pending: 1, persisted: true })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Bruk gul ledning')
    await waitFor(() => expect(chatClient.queueInvocationInput).toHaveBeenCalled())
    const clientMessageId = chatClient.queueInvocationInput.mock.calls[0]?.[2]?.clientMessageId as string

    stream.handlers().onQueuedInput?.({ messages: ['Bruk gul ledning'], ids: [clientMessageId] })
    flush()
    stream.handlers().onDone?.({})
    flush()
    stream.finish()

    await waitFor(() => expect(stripState('Bruk gul ledning')).toBe('delivered'))
    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
  })

  it('sends a steered message the run never read as an ordinary turn once the run finishes cleanly', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({ outcome: 'run_ended' })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Legg til kolonne')
    await waitFor(() => expect(stripState('Legg til kolonne')).toBe('resent'))

    stream.handlers().onDone?.({})
    flush()
    stream.finish()

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(2))
    expect(chatClient.streamChat.mock.calls[1]?.[0]).toMatchObject({ content: 'Legg til kolonne', threadId: 't1' })
  })

  it('says a steered message the run never read was not sent again when the run stops', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({ outcome: 'run_ended' })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Legg til kolonne')
    await waitFor(() => expect(stripState('Legg til kolonne')).toBe('resent'))

    stream.handlers().onStopped?.({})
    flush()
    stream.finish()

    await waitFor(() => expect(stripState('Legg til kolonne')).toBe('refused'))
    expect(screen.getByText(/ble ikke sendt på nytt/)).toBeTruthy()
    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
  })

  // The real client turns the gateway's 429 into a refusal that keeps the words; only the transport is stubbed.
  it('a 429 from the gateway shows its note as the refusal and keeps the words in the box', async () => {
    const stream = scriptedStream()
    const actual = await vi.importActual<typeof import('@/shared/api/chat-client')>('@/shared/api/chat-client')
    chatClient.queueInvocationInput.mockImplementation(actual.queueInvocationInput)
    vi.stubGlobal('fetch', vi.fn(async () => new Response(
      JSON.stringify({ error: { code: 'too_many_pending', message: 'Tre meldinger venter allerede.' } }),
      { status: 429, headers: { 'Content-Type': 'application/json' } },
    )))
    try {
      renderInThread(STEER_ON)
      await ownTurnStreaming(stream)

      typeAndSubmit('Ny instruks')

      await waitFor(() => expect(stripState('Ny instruks')).toBe('refused'))
      expect(screen.getByText('Tre meldinger venter allerede.')).toBeTruthy()
      expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Ny instruks')
      expect(chatClient.queueInvocationInput).toHaveBeenCalledTimes(1)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('says a steered message the run never read was not sent again when the run fails', async () => {
    const stream = scriptedStream()
    chatClient.queueInvocationInput.mockResolvedValue({ outcome: 'run_ended' })
    renderInThread(STEER_ON)
    await ownTurnStreaming(stream)

    typeAndSubmit('Legg til kolonne')
    await waitFor(() => expect(stripState('Legg til kolonne')).toBe('resent'))

    stream.handlers().onError?.({ code: 'upstream_error', message: 'Tjenesten svarte ikke' })
    stream.finish()

    await waitFor(() => expect(stripState('Legg til kolonne')).toBe('refused'))
    expect(screen.getByText(/ble ikke sendt på nytt/)).toBeTruthy()
    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
  })

  // Send when ready (WS09-S2): a busy thread holds the message here instead of refusing it.
  // With the live view off no done event can arrive, so the room's projection stands in for it.
  it('keeps a message for a thread another member\'s agent is working in, and sends it once that work is done', async () => {
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'running' }])
    flush()
    renderInThread(STEER_ON)

    typeAndSubmit('Oppdater lista')

    expect(await screen.findByText(/Agenten jobber allerede i denne samtalen/)).toBeTruthy()
    expect(screen.getByText(/Venter på at agenten blir ferdig/)).toBeTruthy()
    expect(chatClient.streamChat).not.toHaveBeenCalled()

    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'completed' }])
    flush()

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ content: 'Oppdater lista', threadId: 't1' })
  })

  it('holds one message while another waits: a further send keeps its words in the box', async () => {
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' }])
    flush()
    renderInThread(STEER_ON)
    typeAndSubmit('Første')
    expect(await screen.findByText(/Venter på at agenten blir ferdig: «Første»/)).toBeTruthy()

    const box = screen.getByRole('textbox') as HTMLTextAreaElement
    fireEvent.input(box, { target: { value: 'Andre' } })
    flush()
    fireEvent.keyDown(box, { key: 'Enter' })
    flush()
    expect(box.value).toBe('Andre')
    expect(screen.getByText(/Venter på at agenten blir ferdig: «Første»/)).toBeTruthy()

    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'completed' }])
    flush()

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ content: 'Første' })
    expect(box.value).toBe('Andre')
  })

  it('cancelling a waiting message gives its words back to the box and sends nothing', async () => {
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' }])
    flush()
    renderInThread(STEER_ON)
    typeAndSubmit('Oppdater lista')
    await screen.findByText(/Venter på at agenten blir ferdig/)

    screen.getByRole('button', { name: 'Avbryt' }).click()
    flush()
    await waitFor(() => expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Oppdater lista'))

    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'completed' }])
    flush()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('gives a send-when-ready its words back when the room refuses it because another run had the thread', async () => {
    const stream = scriptedStream()
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' }])
    flush()
    renderInThread(STEER_ON)
    typeAndSubmit('Oppdater lista')
    await screen.findByText(/Venter på at agenten blir ferdig/)

    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'completed' }])
    flush()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    stream.handlers().onError?.({ code: 'thread_run_active', message: 'Another reply is running.' })
    flush()
    stream.finish()

    await waitFor(() => expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Oppdater lista'))
    expect(chatClient.streamChat).toHaveBeenCalledTimes(1)
  })

  // With the room's live view on, the run the message waits behind has a stream of its own, and its
  // `finished` frame is the done event the message waits for. An idle projection is not.
  it('sends a waiting message on the run\'s done event, not when the projection first shows the thread idle', async () => {
    let onFrame: SpaceRunLiveHandlers['onFrame'] = () => {}
    runLive.streamSpaceRunLive.mockImplementation((_spaceRef: string, _runId: string, handlers: SpaceRunLiveHandlers) => {
      onFrame = handlers.onFrame
      return new Promise<void>(() => {})
    })
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_id: 'run-9', latest_run_status: 'running' }])
    flush()
    renderInThread({ ...STEER_ON, live: true })

    typeAndSubmit('Oppdater lista')
    await screen.findByText(/Venter på at agenten blir ferdig/)
    await waitFor(() => expect(runLive.streamSpaceRunLive).toHaveBeenCalledWith('room-1', 'run-9', expect.anything(), expect.anything()))

    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_id: 'run-9', latest_run_status: 'completed' }])
    flush()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(chatClient.streamChat).not.toHaveBeenCalled()
    expect(screen.getByText(/Venter på at agenten blir ferdig/)).toBeTruthy()

    onFrame({ type: 'finished', outcome: 'completed', reason: null }, undefined)
    flush()

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ content: 'Oppdater lista', threadId: 't1' })
  })

  it('cancelling a waiting message gives back its skill and its agent mention, not only its words', async () => {
    skillsClient.listAvailableSkills.mockReset()
    skillsClient.listAvailableSkills.mockResolvedValue([{ id: 'sk-hms', name: 'HMS-runde', description: '', enabled: true, scope: 'org' }])
    scriptedStream()
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
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' }])
    flush()
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => [agent] as never}
        threads={() => []}
        replyTarget={() => ({ threadId: 't1', title: 'Innkjøp' })}
        agentWork={() => STEER_ON as never}
      />
    ))
    const box = screen.getByRole('textbox') as HTMLTextAreaElement
    fireEvent.input(box, { target: { value: '/hms' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /HMS-runde/ }))
    flush()
    fireEvent.input(box, { target: { value: '@Drif' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
    flush()
    fireEvent.input(box, { target: { value: '@Driftsassistent se på lista' } })
    flush()
    fireEvent.submit(box.closest('form') as HTMLFormElement)
    await screen.findByText(/Venter på at agenten blir ferdig/)
    expect(screen.queryByRole('list', { name: 'Valgte ferdigheter' })).toBeNull()

    screen.getByRole('button', { name: 'Avbryt' }).click()
    flush()
    await waitFor(() => expect(box.value).toBe('@Driftsassistent se på lista'))
    expect(screen.getByRole('list', { name: 'Valgte ferdigheter' }).textContent).toContain('HMS-runde')

    // The thread is free now: the message goes out as it was first asked, with the picks given back.
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'completed' }])
    flush()
    fireEvent.submit(box.closest('form') as HTMLFormElement)
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      content: '@Driftsassistent se på lista',
      mentionedAgentRef: 'agent-1',
      actions: [{ id: 'sk-hms', name: 'HMS-runde', kind: 'skill' }],
    })
  })
})

// ---------------------------------------------------------------------------
// W3 P1-D: per-turn attachments, keep-in-room, drafts and the personal memory choice.
// The shared helpers come first; each owed item has its own describe below.
// ---------------------------------------------------------------------------

/**
 * A chip's object URL. jsdom has none, so each fixture file is given a data URL carrying the same
 * bytes; the composer and the real `fetch` read it the way they would a blob URL.
 */
const chipUrls = new Map<Blob, string>()

function fixtureFile(name: string, body: string, type: string): File {
  const file = new File([body], name, { type })
  chipUrls.set(file, `data:${type};base64,${btoa(body)}`)
  return file
}

/**
 * Gives the chips their object URLs, and the composer's reads of those URLs a body, for one case. The
 * setup file replaces `fetch` with an empty-JSON stub, so the reads of this case's data URLs are served
 * here. The function it returns puts everything back.
 */
function stubObjectUrls(): () => void {
  const url = URL as unknown as { createObjectURL?: (blob: Blob) => string }
  const original = url.createObjectURL
  url.createObjectURL = (blob: Blob) => chipUrls.get(blob) ?? 'data:application/octet-stream;base64,'
  vi.stubGlobal('fetch', async (input: string | URL) => {
    const match = /^data:([^;,]*)(;base64)?,(.*)$/s.exec(String(input))
    if (!match) return new Response('', { status: 404 })
    const body = match[2] ? atob(match[3] ?? '') : decodeURIComponent(match[3] ?? '')
    return new Response(body, { status: 200, headers: { 'Content-Type': match[1] || 'text/plain' } })
  })
  return () => {
    url.createObjectURL = original
    vi.unstubAllGlobals()
  }
}

/** The room's flags, published the way the page publishes them after a context read. */
function publishRoomFeatures(features: Record<string, unknown>): void {
  publishSpaceRoomFeatures(features)
  flush()
}

const DRAFT_KEY_PREFIX = 'verevon.home-draft.v1'

const roomBox = () => screen.getByRole('textbox') as HTMLTextAreaElement

function typeInRoomBox(value: string): void {
  fireEvent.input(roomBox(), { target: { value } })
  flush()
}

function submitRoomBox(): void {
  fireEvent.submit(roomBox().closest('form') as HTMLFormElement)
  flush()
}

/** Picks files with the attach button's own file input. */
function pickRoomFiles(files: readonly File[]): void {
  fireEvent.change(screen.getByLabelText('Legg til filer'), { target: { files: [...files] } })
  flush()
}

function pasteIntoRoomBox(files: readonly File[]): void {
  fireEvent.paste(roomBox(), { clipboardData: { files: [...files], getData: () => '', types: ['Files'] } })
  flush()
}

function dropOnRoomForm(files: readonly File[]): void {
  fireEvent.drop(roomBox().closest('form') as HTMLFormElement, { dataTransfer: { files: [...files], types: ['Files'] } })
  flush()
}

type RoomOptions = {
  readonly spaceRef?: string
  readonly humanPosts?: boolean
  readonly viewerRole?: string
  readonly replyTarget?: { readonly threadId: string; readonly title: string }
  readonly journal?: SpaceSendJournal
}

function renderRoom(options: RoomOptions = {}) {
  return render(() => (
    <SpaceRoomComposer
      spaceRef={options.spaceRef ?? 'room-1'}
      roster={() => []}
      agents={() => []}
      threads={() => []}
      humanPostsEnabled={options.humanPosts ? () => true : undefined}
      viewerRole={() => options.viewerRole ?? 'editor'}
      replyTarget={options.replyTarget ? () => options.replyTarget : undefined}
      journal={options.journal}
    />
  ))
}

/** Every key and value either browser store holds, as text, so a case can search them for the words. */
function storedEntries(): string[] {
  const entries: string[] = []
  for (const store of [window.localStorage, window.sessionStorage]) {
    for (let index = 0; index < store.length; index += 1) {
      const key = store.key(index) ?? ''
      entries.push(`${key}=${store.getItem(key) ?? ''}`)
    }
  }
  return entries
}

/** The text of each draft this tab holds in session storage. */
function storedDraftTexts(): string[] {
  const texts: string[] = []
  const store = window.sessionStorage
  for (let index = 0; index < store.length; index += 1) {
    const key = store.key(index) ?? ''
    if (!key.startsWith(DRAFT_KEY_PREFIX)) continue
    const stored = JSON.parse(store.getItem(key) ?? '{}') as { text?: unknown }
    if (typeof stored.text === 'string') texts.push(stored.text)
  }
  return texts
}

/**
 * Types, then lets the draft's write-after-typing run. The timer is set under fake time, so the write
 * happens inside the advance and the case needs no real wait.
 */
async function typeAndSettleDraft(value: string): Promise<void> {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  try {
    typeInRoomBox(value)
    await vi.advanceTimersByTimeAsync(HOME_DRAFT_DEBOUNCE_MS)
  } finally {
    vi.useRealTimers()
  }
}

/** A journal that flushes after each write, as the post sender needs (see the human-post cases above). */
function flushingPostJournal(): SpaceSendJournal {
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

describe('SpaceRoomComposer — turn attachments (WS14-S8)', () => {
  let restoreObjectUrls: () => void = () => {}
  const zdrNotice = 'Dette støttes ikke med Zero Data Retention ennå'
  const notes = () => fixtureFile('Notat.txt', 'Lagerliste: 12 paller', 'text/plain')
  const photo = () => fixtureFile('bilde.png', 'PNG', 'image/png')

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_TURN_ATTACHMENTS', '')
    vi.stubEnv('VITE_SPACE_ROOM_FILES', '')
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
    vi.stubEnv('VITE_CHAT_VISION_AS_PARTS_ENABLED', '')
    restoreObjectUrls = stubObjectUrls()
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    __resetRetentionForTests()
    window.sessionStorage.clear()
    restoreObjectUrls()
  })

  it('has no attach button, and ignores paste and drop, while turn_attachments is off', () => {
    renderRoom()
    expect(screen.queryByRole('button', { name: 'Legg ved fil' })).toBeNull()
    pasteIntoRoomBox([photo()])
    dropOnRoomForm([notes()])
    expect(screen.queryByRole('list', { name: 'Vedlegg' })).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('offers the attach button, paste and drop in an Ask turn while turn_attachments is on', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom()
    expect(screen.getByRole('button', { name: 'Legg ved fil' })).toBeTruthy()
    pasteIntoRoomBox([photo()])
    expect(screen.getByText('bilde.png')).toBeTruthy()
    dropOnRoomForm([notes()])
    expect(screen.getByText('Notat.txt')).toBeTruthy()
  })

  it('offers none of them in a Send turn, where the box posts words to the room', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom({ humanPosts: true })
    expect(screen.getByRole('radio', { name: 'Send til rommet' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.queryByRole('button', { name: 'Legg ved fil' })).toBeNull()
    pasteIntoRoomBox([photo()])
    dropOnRoomForm([notes()])
    expect(screen.queryByRole('list', { name: 'Vedlegg' })).toBeNull()
  })

  it('removes a chip from the box and keeps the words', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom()
    pickRoomFiles([notes()])
    typeInRoomBox('Oppsummer')
    fireEvent.click(screen.getByRole('button', { name: 'Fjern Notat.txt' }))
    flush()
    expect(screen.queryByRole('button', { name: 'Fjern Notat.txt' })).toBeNull()
    expect(roomBox().value).toBe('Oppsummer')
  })

  it('sends a document\'s text fenced into the message, not as an attachment', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    const stream = scriptedStream()
    renderRoom()
    pickRoomFiles([notes()])
    typeInRoomBox('Oppsummer')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    const request = chatClient.streamChat.mock.calls[0]?.[0] as { content: string; attachments?: unknown }
    expect(request.content).toBe(
      'Oppsummer\n\n--- VEDLEGG: Notat.txt ---\nLagerliste: 12 paller\n--- SLUTT PÅ VEDLEGG: Notat.txt ---',
    )
    expect(request.attachments).toBeUndefined()
    stream.finish()
  })

  it('keeps the chips until the room has accepted the turn, then clears them', async () => {
    publishRoomFeatures({ composer_drafts: true, room_files: { turn_attachments: true } })
    const stream = scriptedStream()
    renderRoom()
    pickRoomFiles([notes()])
    typeInRoomBox('Oppsummer')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    // The sent message's own bubble lists its files too, so the composer's chip is named by its remove control.
    expect(screen.getByRole('button', { name: 'Fjern Notat.txt' })).toBeTruthy()
    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-1' })
    flush()
    expect(screen.queryByRole('button', { name: 'Fjern Notat.txt' })).toBeNull()
    stream.finish()
  })

  it('refuses an image under zero data retention, visibly, and no send starts', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    applyServerRetention(true)
    renderRoom()
    pickRoomFiles([photo()])
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain(zdrNotice)
    expect(alert.textContent).toContain('fil- og bildevedlegg')
    expect(screen.queryByText('bilde.png')).toBeNull()
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('refuses a picked image at Send once the workspace turns zero retention on, and sends nothing', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom()
    pickRoomFiles([photo()])
    applyServerRetention(true)
    flush()
    typeInRoomBox('Se på bildet')
    submitRoomBox()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain(zdrNotice)
    expect(alert.textContent).toContain('Utkastet er beholdt')
    expect(chatClient.streamChat).not.toHaveBeenCalled()
    expect(screen.getByText('bilde.png')).toBeTruthy()
    expect(roomBox().value).toBe('Se på bildet')
  })

  it('shows the notice, and sends nothing, when a Send turn is posted with chips in the box', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom({ humanPosts: true })
    fireEvent.click(screen.getByRole('radio', { name: 'Spør Verevon' }))
    flush()
    pickRoomFiles([notes()])
    fireEvent.click(screen.getByRole('radio', { name: 'Send til rommet' }))
    flush()
    typeInRoomBox('Oppsummer')
    submitRoomBox()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Vedlegg følger bare med når du spør Verevon')
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    expect(spacesClient.replyInSpaceThread).not.toHaveBeenCalled()
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })
})

describe('SpaceRoomComposer — keep text files in the room (WS14-S9)', () => {
  let restoreObjectUrls: () => void = () => {}
  const notes = () => fixtureFile('Notat.txt', 'Lagerliste: 12 paller', 'text/plain')
  const list = () => fixtureFile('Liste.csv', 'Paller;12', 'text/csv')
  const keepChoice = () => screen.getByRole('checkbox', { name: /Behold tekstfiler i rommet/ }) as HTMLInputElement

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_TURN_ATTACHMENTS', '')
    vi.stubEnv('VITE_SPACE_ROOM_FILES', '')
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
    vi.stubEnv('VITE_CHAT_VISION_AS_PARTS_ENABLED', '')
    restoreObjectUrls = stubObjectUrls()
    spaceFilesClient.uploadSpaceFiles.mockReset()
    spaceFilesClient.uploadSpaceFiles.mockResolvedValue({ uploadId: 'up-1', deduplicated: false, files: [] })
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    __resetRetentionForTests()
    window.sessionStorage.clear()
    restoreObjectUrls()
  })

  it('is offered with the durable flag and a writer role, unchecked', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes()])
    expect(keepChoice().checked).toBe(false)
  })

  it('is not offered without the durable flag', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true } })
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes()])
    expect(screen.getByText('Notat.txt')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Behold tekstfiler i rommet/ })).toBeNull()
  })

  it('is not offered to a reader, who cannot add files to the room', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    renderRoom({ viewerRole: 'viewer' })
    pickRoomFiles([notes()])
    expect(screen.getByText('Notat.txt')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Behold tekstfiler i rommet/ })).toBeNull()
  })

  it('is not offered under zero data retention, and a switch while the box is open takes it away', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes()])
    expect(keepChoice()).toBeTruthy()
    applyServerRetention(true)
    flush()
    expect(screen.getByText('Notat.txt')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Behold tekstfiler i rommet/ })).toBeNull()
  })

  it('is not offered when the workspace is already zero retention as the room opens', () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    applyServerRetention(true)
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes()])
    expect(screen.getByText('Notat.txt')).toBeTruthy()
    expect(screen.queryByRole('checkbox', { name: /Behold tekstfiler i rommet/ })).toBeNull()
  })

  it('uploads exactly the chip files it was chosen for, beside the send', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    const stream = scriptedStream()
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes(), list()])
    fireEvent.click(keepChoice())
    flush()
    typeInRoomBox('Oppsummer')
    submitRoomBox()
    await waitFor(() => expect(spaceFilesClient.uploadSpaceFiles).toHaveBeenCalledTimes(1))
    const [spaceRef, kept] = spaceFilesClient.uploadSpaceFiles.mock.calls[0] as [string, File[]]
    expect(spaceRef).toBe('room-1')
    expect(kept.map((file) => file.name)).toEqual(['Notat.txt', 'Liste.csv'])
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    stream.finish()
  })

  it('is unchecked again after the send, so the next turn uploads nothing unless it is chosen again', async () => {
    publishRoomFeatures({ room_files: { turn_attachments: true, durable: true } })
    const first = scriptedStream()
    renderRoom({ viewerRole: 'editor' })
    pickRoomFiles([notes()])
    fireEvent.click(keepChoice())
    flush()
    typeInRoomBox('Første')
    submitRoomBox()
    await waitFor(() => expect(spaceFilesClient.uploadSpaceFiles).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    first.handlers().onDone?.({})
    flush()
    first.finish()
    await waitFor(() => expect(roomBox().disabled).toBe(false))

    const second = scriptedStream()
    pickRoomFiles([list()])
    expect(keepChoice().checked).toBe(false)
    typeInRoomBox('Andre')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(2))
    expect(spaceFilesClient.uploadSpaceFiles).toHaveBeenCalledTimes(1)
    second.finish()
  })
})

describe('SpaceRoomComposer — drafts (WS15-S2)', () => {
  const receipt = { threadId: 'thr_new', messageId: 'msg_1', sequence: 1, deduplicated: false }

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
    publishRoomFeatures({ composer_drafts: true })
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    __resetRetentionForTests()
    window.sessionStorage.clear()
    vi.useRealTimers()
  })

  it('restores a draft in the room and reply target it was typed in, and in no other', async () => {
    renderRoom()
    await typeAndSettleDraft('Utkast i rommet')
    cleanup()

    renderRoom()
    expect(roomBox().value).toBe('Utkast i rommet')
    cleanup()

    renderRoom({ spaceRef: 'room-2' })
    expect(roomBox().value).toBe('')
    cleanup()

    const reply = { threadId: 't-1', title: 'Innkjøp' }
    renderRoom({ replyTarget: reply })
    expect(roomBox().value).toBe('')
    await typeAndSettleDraft('Svar i tråden')
    cleanup()

    renderRoom()
    expect(roomBox().value).toBe('Utkast i rommet')
    cleanup()

    renderRoom({ replyTarget: reply })
    expect(roomBox().value).toBe('Svar i tråden')
  })

  it('an ask send keeps its words until the room has the turn, then clears them and the stored draft', async () => {
    const stream = scriptedStream()
    renderRoom()
    await typeAndSettleDraft('Rydd lageret')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(roomBox().value).toBe('Rydd lageret')
    expect(storedDraftTexts()).toEqual(['Rydd lageret'])

    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-1' })
    flush()
    expect(roomBox().value).toBe('')
    expect(storedDraftTexts()).toEqual([])
    stream.finish()
  })

  it('an ask send that fails before the room has the turn keeps its words and the stored draft', async () => {
    const stream = scriptedStream()
    renderRoom()
    await typeAndSettleDraft('Rydd lageret')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    stream.handlers().onError?.({ code: 'upstream_error', message: 'Tjenesten svarte ikke' })
    stream.finish()
    flush()
    await screen.findByRole('alert')
    expect(roomBox().value).toBe('Rydd lageret')
    expect(storedDraftTexts()).toEqual(['Rydd lageret'])
  })

  it('an ask send stopped before the room has the turn keeps its words and the stored draft', async () => {
    const stream = scriptedStream()
    renderRoom()
    await typeAndSettleDraft('Rydd lageret')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    fireEvent.click(screen.getByRole('button', { name: 'Stopp' }))
    flush()
    expect(await screen.findByText(/Avbrutt før svaret startet/)).toBeTruthy()
    stream.finish()
    flush()
    expect(roomBox().value).toBe('Rydd lageret')
    expect(storedDraftTexts()).toEqual(['Rydd lageret'])
  })

  it('a post the room accepts clears the box and the stored draft', async () => {
    spacesClient.createSpacePost.mockResolvedValueOnce(receipt)
    renderRoom({ humanPosts: true, journal: flushingPostJournal() })
    await typeAndSettleDraft('Hei alle')
    expect(storedDraftTexts()).toEqual(['Hei alle'])
    submitRoomBox()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(roomBox().value).toBe(''))
    expect(storedDraftTexts()).toEqual([])
  })

  it('with the drafts flag off the box is cleared at Send, as before, and nothing is stored', async () => {
    publishRoomFeatures({})
    const stream = scriptedStream()
    renderRoom()
    await typeAndSettleDraft('Rydd lageret')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(roomBox().value).toBe('')
    expect(storedDraftTexts()).toEqual([])
    stream.finish()
  })

  it('under a zero-retention workspace nothing is written to browser storage, and no older draft comes back', async () => {
    renderRoom()
    await typeAndSettleDraft('Gammelt utkast')
    expect(storedDraftTexts()).toEqual(['Gammelt utkast'])
    cleanup()

    applyServerRetention(true)
    expect(storedDraftTexts()).toEqual([])

    renderRoom()
    expect(roomBox().value).toBe('')
    await typeAndSettleDraft('Hemmelig utkast')
    expect(roomBox().value).toBe('Hemmelig utkast')
    expect(storedEntries().filter((entry) => entry.includes('Hemmelig') || entry.startsWith(DRAFT_KEY_PREFIX))).toEqual([])
    cleanup()

    renderRoom()
    expect(roomBox().value).toBe('')
  })
})

describe('SpaceRoomComposer — personal memory for one room turn (WS17-S8)', () => {
  const MEMORY_LABEL = 'Bruk mitt personlige minne for dette svaret'
  // Personal memory is recalled into an existing room thread, so the choice is offered only on a reply.
  const REPLY = { threadId: 'thr_7', title: 'Innkjøp' } as const

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    window.sessionStorage.clear()
  })

  it('is offered only while features.personal_memory_turn is on', () => {
    renderRoom({ replyTarget: REPLY })
    expect(screen.queryByRole('checkbox', { name: MEMORY_LABEL })).toBeNull()
    cleanup()

    publishRoomFeatures({ personal_memory_turn: true })
    renderRoom({ replyTarget: REPLY })
    expect(screen.getByRole('checkbox', { name: MEMORY_LABEL })).toBeTruthy()
  })

  // A turn that starts a new thread has no room thread to recall into: the gateway drops the choice and answers
  // without personal memory, with nothing said. A box that does nothing must not be offered.
  it('is not offered while the turn would start a new thread, and no choice is ever sent for one', async () => {
    publishRoomFeatures({ personal_memory_turn: true })
    const stream = scriptedStream()
    renderRoom()
    expect(screen.queryByRole('checkbox', { name: MEMORY_LABEL })).toBeNull()

    typeInRoomBox('Hva fant vi sist?')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect((chatClient.streamChat.mock.calls[0]?.[0] as { threadId?: unknown }).threadId).toBeUndefined()
    expect((chatClient.streamChat.mock.calls[0]?.[0] as { usePersonalMemory?: unknown }).usePersonalMemory).toBeUndefined()
    stream.finish()
  })

  it('is offered for an Ask turn, not for a Send turn', () => {
    publishRoomFeatures({ personal_memory_turn: true })
    // A reply offers Ask only to the thread's starter, so the viewer is made the owner of the thread.
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        humanPostsEnabled={() => true}
        viewerRole={() => 'editor'}
        viewerSubjectId={() => 'user_1'}
        threadOwnerSubjectId={() => 'user_1'}
        replyTarget={() => REPLY}
      />
    ))
    expect(screen.queryByRole('checkbox', { name: MEMORY_LABEL })).toBeNull()
    fireEvent.click(screen.getByRole('radio', { name: 'Spør Verevon' }))
    flush()
    expect(screen.getByRole('checkbox', { name: MEMORY_LABEL })).toBeTruthy()
  })

  it('goes with the turn it was chosen for, and is unchecked again after the send', async () => {
    publishRoomFeatures({ personal_memory_turn: true })
    const first = scriptedStream()
    renderRoom({ replyTarget: REPLY })
    const memory = screen.getByRole('checkbox', { name: MEMORY_LABEL }) as HTMLInputElement
    fireEvent.click(memory)
    flush()
    expect(memory.checked).toBe(true)

    typeInRoomBox('Hva fant vi sist?')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ usePersonalMemory: true, threadId: 'thr_7' })
    expect(memory.checked).toBe(false)

    first.handlers().onDone?.({})
    flush()
    first.finish()
    await waitFor(() => expect(roomBox().disabled).toBe(false))

    const second = scriptedStream()
    typeInRoomBox('Og nå?')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(2))
    expect((chatClient.streamChat.mock.calls[1]?.[0] as { usePersonalMemory?: unknown }).usePersonalMemory).toBeUndefined()
    second.finish()
  })

  // The choice is for one thread's answer: moving the reply to another thread, or to none, takes it back.
  it('is unchecked again when the reply moves to another thread, and gone when the reply is cleared', () => {
    publishRoomFeatures({ personal_memory_turn: true })
    const [target, setTarget] = createSignal<{ threadId: string; title: string } | undefined>(REPLY)
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        viewerRole={() => 'editor'}
        replyTarget={target}
      />
    ))
    const memory = () => screen.getByRole('checkbox', { name: MEMORY_LABEL }) as HTMLInputElement
    fireEvent.click(memory())
    flush()
    expect(memory().checked).toBe(true)

    // The same thread's row refreshing (a new object, the same thread) leaves the choice alone.
    setTarget({ threadId: 'thr_7', title: 'Innkjøp (oppdatert)' })
    flush()
    expect(memory().checked).toBe(true)

    setTarget({ threadId: 'thr_8', title: 'Leveranse' })
    flush()
    expect(memory().checked).toBe(false)

    fireEvent.click(memory())
    flush()
    expect(memory().checked).toBe(true)
    setTarget(undefined)
    flush()
    expect(screen.queryByRole('checkbox', { name: MEMORY_LABEL })).toBeNull()
    setTarget(REPLY)
    flush()
    expect(memory().checked).toBe(false)
  })
})

// WS13-S7: platform slash commands. A command runs locally on Enter, clears its `/token`, sends nothing and
// reports through one status line. The shared mark-read action it runs is faked; the composer's part is tested.
describe('SpaceRoomComposer — platform commands (WS13-S7)', () => {
  const textarea = () => screen.getByPlaceholderText(/velger en ferdighet/) as HTMLTextAreaElement
  let disposeOpener: (() => void) | undefined

  beforeEach(() => {
    setCommandPaletteFeature(true)
    skillsClient.listAvailableSkills.mockReset()
    skillsClient.listAvailableSkills.mockResolvedValue([
      { id: 'sk-hms', name: 'HMS-runde', description: '', enabled: true, scope: 'org' },
    ])
    paletteProviders.runPaletteAction.mockReset()
    paletteProviders.runPaletteAction.mockResolvedValue('done')
  })

  afterEach(() => {
    cleanup()
    disposeOpener?.()
    disposeOpener = undefined
    setCommandPaletteFeature(undefined)
  })

  it('a command picked from the list runs locally: the box is cleared, nothing is sent, and one status line says so', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/read' } })
    flush()
    fireEvent.click(await screen.findByRole('option', { name: /Merk rommet som lest/ }))
    flush()

    const status = await screen.findByText('Rommet er merket som lest.')
    expect(status.closest('[role="status"]')).toBeTruthy()
    expect(paletteProviders.runPaletteAction).toHaveBeenCalledWith('spaces.mark_read', { spaceRef: 'room-1' })
    expect(textarea().value).toBe('')
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('/search with words opens message search with them on Enter, and posts nothing', async () => {
    const opener = vi.fn()
    disposeOpener = setMessageSearchOpener(opener)
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/search kvartals' } })
    flush()
    fireEvent.keyDown(textarea(), { key: 'Enter' })
    flush()

    await waitFor(() => expect(opener).toHaveBeenCalledWith('kvartals'))
    expect(textarea().value).toBe('')
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('a /word that names no command is sent as an ordinary message', async () => {
    const stream = scriptedStream()
    renderComposer()
    await send('/redo')
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({ content: '/redo' })
    expect(paletteProviders.runPaletteAction).not.toHaveBeenCalled()
    stream.finish()
  })

  it('the commands are listed above the skills, each with a Command badge', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()
    await screen.findByRole('option', { name: /HMS-runde/ })

    const options = screen.getAllByRole('option')
    const labels = options.map((option) => option.textContent ?? '')
    expect(labels[0]).toContain('/read')
    expect(within(options[0] as HTMLElement).getByText('Kommando')).toBeTruthy()
    const lastCommand = labels.reduce((last, label, index) => (label.includes('Kommando') ? index : last), -1)
    const firstSkill = labels.findIndex((label) => label.includes('HMS-runde'))
    expect(lastCommand).toBeGreaterThanOrEqual(0)
    expect(lastCommand).toBeLessThan(firstSkill)
  })

  it('the picker is named for skills and commands together, so a screen reader hears both', async () => {
    renderComposer()
    fireEvent.input(textarea(), { target: { value: '/' } })
    flush()

    expect(await screen.findByRole('listbox', { name: 'Velg en ferdighet eller kommando' })).toBeTruthy()
  })
})

// WS15-S3: markdown authoring in the room box, behind `features.composer_markdown`: a format bar, a
// Write/Preview toggle, a counter that always shows in a post, and the shortcuts. Off, the box is today's.
describe('SpaceRoomComposer — markdown authoring (WS15-S3)', () => {
  const receipt = { threadId: 'thr_md', messageId: 'msg_md_1', sequence: 1, deduplicated: false }
  const formatBar = () => screen.queryByRole('toolbar', { name: 'Formatering' })
  // The Preview state hides the textarea, so it is read by element rather than by role.
  const bodyBox = () => document.querySelector('textarea') as HTMLTextAreaElement

  function renderMarkdown(options: { humanPosts?: boolean; maxChars?: number } = {}) {
    return render(() => (
      <SpaceRoomComposer
        journal={flushingPostJournal()}
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        humanPostsEnabled={options.humanPosts ? () => true : undefined}
        viewerRole={() => 'editor'}
        postMaxChars={options.maxChars === undefined ? undefined : () => options.maxChars}
      />
    ))
  }

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_COMPOSER_MARKDOWN', '')
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
    spacesClient.createSpacePost.mockResolvedValue(receipt)
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    window.sessionStorage.clear()
  })

  it('with the flag off there is no format bar, the box is two rows, and the counter shows only past 80%', () => {
    publishRoomFeatures({})
    renderMarkdown({ humanPosts: true, maxChars: 10 })
    expect(formatBar()).toBeNull()
    expect(screen.queryByRole('button', { name: 'Forhåndsvis' })).toBeNull()
    expect(bodyBox().getAttribute('rows')).toBe('2')

    typeInRoomBox('x'.repeat(7))
    expect(screen.queryByText('7 / 10')).toBeNull()
    typeInRoomBox('x'.repeat(9))
    expect(screen.getByText('9 / 10')).toBeTruthy()
  })

  it('with the flag on the format bar offers bold, italic, code, link, list and quote', () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown()
    const bar = formatBar() as HTMLElement
    expect(bar).toBeTruthy()
    expect(Array.from(bar.querySelectorAll('[data-format]')).map((button) => button.getAttribute('data-format')))
      .toEqual(['bold', 'italic', 'code', 'link', 'bullet', 'quote'])
    expect(within(bar).getByRole('button', { name: 'Fet (Ctrl+B)' })).toBeTruthy()
    expect(within(bar).getByRole('button', { name: 'Lenke (Ctrl+K)' })).toBeTruthy()
  })

  it('Ctrl+B wraps the selected words in bold and keeps them in the box, and nothing is sent', () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown({ humanPosts: true })
    typeInRoomBox('hei alle')
    bodyBox().setSelectionRange(0, 3)
    fireEvent.keyDown(roomBox(), { key: 'b', ctrlKey: true })
    flush()

    expect(roomBox().value).toBe('**hei** alle')
    expect([bodyBox().selectionStart, bodyBox().selectionEnd]).toEqual([2, 5])
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('Preview renders the draft through the chat markdown and hides the box; Write brings the box back', () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown({ humanPosts: true })
    typeInRoomBox('Hei **alle**')
    fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis' }))
    flush()

    expect(bodyBox().hidden).toBe(true)
    const preview = screen.getByLabelText('Forhåndsvisning')
    expect(within(preview).getByText('Hei **alle**')).toBeTruthy()
    expect(formatBar()).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Skriv' }))
    flush()
    expect(bodyBox().hidden).toBe(false)
    expect(screen.queryByLabelText('Forhåndsvisning')).toBeNull()
    expect(roomBox().value).toBe('Hei **alle**')
  })

  it('in a post the counter always shows, and Send is disabled over the post limit', () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown({ humanPosts: true, maxChars: 10 })
    expect(screen.getByText('0 / 10')).toBeTruthy()

    typeInRoomBox('x'.repeat(11))
    expect(screen.getByRole('alert').textContent).toContain('11 / 10')
    expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
  })

  it('Enter sends the post, and Shift+Enter keeps its newline and sends nothing', async () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown({ humanPosts: true })
    typeInRoomBox('Hei alle')

    const shiftEnter = new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true, cancelable: true })
    roomBox().dispatchEvent(shiftEnter)
    flush()
    expect(shiftEnter.defaultPrevented).toBe(false)
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
    expect(roomBox().value).toBe('Hei alle')

    fireEvent.keyDown(roomBox(), { key: 'Enter' })
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(spacesClient.createSpacePost.mock.calls[0]?.[1]).toMatchObject({ content: 'Hei alle' })
  })

  it('emptying the box leaves Preview', () => {
    publishRoomFeatures({ composer_markdown: true })
    renderMarkdown({ humanPosts: true })
    typeInRoomBox('Hei')
    fireEvent.click(screen.getByRole('button', { name: 'Forhåndsvis' }))
    flush()
    expect(bodyBox().hidden).toBe(true)

    fireEvent.input(bodyBox(), { target: { value: '' } })
    flush()
    expect(bodyBox().hidden).toBe(false)
    expect(screen.queryByLabelText('Forhåndsvisning')).toBeNull()
  })
})

// WS08-S6: structured mentions in the room box. A person picked from the list is recorded by id; an agent
// joins the address bar when addressed wake is on. A post carries the picks still in its words, and each agent
// the server recorded as expected is woken from the stored post, one at a time.
describe('SpaceRoomComposer — structured mentions and addressed wakes (WS08-S6)', () => {
  const kari = { subject_type: 'user', subject_id: 'user-kari', display_name: 'Kari Nordmann', role: 'editor' }
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
  const plainReceipt = { threadId: 'thr_m1', messageId: 'msg_m1', sequence: 1, deduplicated: false }
  // The wake runner is module-level and keeps each message's wakes, so every case names its own message.
  let messageCount = 0
  const freshMessageId = (prefix: string) => `${prefix}_${++messageCount}`
  const postedInput = (call = 0) =>
    spacesClient.createSpacePost.mock.calls[call]?.[1] as { content: string; mentions?: unknown }

  function renderMentions(agents: readonly unknown[] = [agent]) {
    return render(() => (
      <SpaceRoomComposer
        journal={flushingPostJournal()}
        spaceRef="room-1"
        roster={() => [kari] as never}
        agents={() => agents as never}
        threads={() => []}
        humanPostsEnabled={() => true}
        viewerRole={() => 'editor'}
        viewerSubjectId={() => 'user-me'}
      />
    ))
  }

  beforeEach(() => {
    vi.stubEnv('VITE_SPACE_COMPOSER_DRAFTS', '')
    spacesClient.createSpacePost.mockResolvedValue(plainReceipt)
  })

  afterEach(() => {
    cleanup()
    clearSpaceRoomFeatures()
    window.sessionStorage.clear()
  })

  it('a person picked from the list goes into the post as a mention by id, and never sets the agent mention', async () => {
    publishRoomFeatures({ mention_picker: true })
    renderMentions()
    typeInRoomBox('@Kar')
    fireEvent.click(await screen.findByRole('option', { name: /Kari Nordmann/ }))
    flush()
    expect(roomBox().value).toBe('@Kari Nordmann ')
    expect(screen.queryByText(/Vil invokere en agent/)).toBeNull()

    submitRoomBox()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(postedInput().content).toBe('@Kari Nordmann')
    expect(postedInput().mentions).toEqual({ people: [{ subjectId: 'user-kari', label: 'Kari Nordmann' }], agents: [] })
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('with the picker off, a person picked from the list is only text, and the post carries no mentions', async () => {
    publishRoomFeatures({ mention_picker: false })
    renderMentions()
    typeInRoomBox('@Kar')
    fireEvent.click(await screen.findByRole('option', { name: /Kari Nordmann/ }))
    flush()

    submitRoomBox()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(postedInput().content).toBe('@Kari Nordmann')
    expect(postedInput().mentions).toBeUndefined()
    expect(screen.queryByRole('list', { name: 'Adressert til' })).toBeNull()
    expect(screen.queryByText(/Vil invokere en agent/)).toBeNull()
  })

  it('with addressed wake on, an agent mention stays a Send: the post carries the agent, then the agent is woken from the stored post', async () => {
    publishRoomFeatures({ mention_picker: true, addressed_wake: true })
    const messageId = freshMessageId('msg_wake')
    spacesClient.createSpacePost.mockResolvedValueOnce({
      ...plainReceipt,
      messageId,
      mentions: { recorded: [{ subjectId: 'agent-1', kind: 'agent', wake: 'expected', ordinal: 0 }], rejected: [] },
    })
    renderMentions()
    typeInRoomBox('@Drif')
    fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
    flush()
    expect(screen.getByRole('list', { name: 'Adressert til' }).textContent).toContain('Driftsassistent')
    expect(screen.getByRole('radio', { name: 'Send til rommet' }).getAttribute('aria-checked')).toBe('true')
    expect(screen.getByRole('button', { name: 'Send' })).toBeTruthy()
    expect(screen.getByText(/Agentene svarer etterpå/)).toBeTruthy()

    submitRoomBox()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(postedInput().content).toBe('@Driftsassistent')
    expect(postedInput().mentions).toEqual({ people: [], agents: [{ subjectId: 'agent-1' }] })

    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))
    expect(chatClient.streamChat.mock.calls[0]?.[0]).toMatchObject({
      content: '@Driftsassistent',
      threadId: 'thr_m1',
      mentionedAgentRef: 'agent-1',
      wake: { messageId, clientWakeId: expect.any(String) },
    })
  })

  it('a mention the server did not record is named in the box with its reason', async () => {
    publishRoomFeatures({ mention_picker: true })
    spacesClient.createSpacePost.mockResolvedValueOnce({
      ...plainReceipt,
      mentions: { recorded: [], rejected: [{ subjectId: 'user-kari', code: 'person_not_in_room' }] },
    })
    renderMentions()
    typeInRoomBox('@Kar')
    fireEvent.click(await screen.findByRole('option', { name: /Kari Nordmann/ }))
    flush()

    submitRoomBox()
    const notice = await screen.findByText('Personen er ikke med i rommet, så omtalen ble ikke lagret.')
    expect(notice.getAttribute('role')).toBe('status')
  })

  it('an agent the server recorded as refused is not woken, and keeps the reason it was refused for', async () => {
    publishRoomFeatures({ mention_picker: true, addressed_wake: true })
    const messageId = freshMessageId('msg_refused')
    spacesClient.createSpacePost.mockResolvedValueOnce({
      ...plainReceipt,
      messageId,
      mentions: {
        recorded: [{ subjectId: 'agent-1', kind: 'agent', wake: 'refused', reason: 'respond_to_denied', ordinal: 0 }],
        rejected: [],
      },
    })
    renderMentions()
    typeInRoomBox('@Drif')
    fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
    flush()

    submitRoomBox()
    await waitFor(() => expect(spaceWakeBoard.forMessage(messageId)).toHaveLength(1))
    const [entry] = spaceWakeBoard.forMessage(messageId)
    expect(entry?.state).toBe('refused')
    expect(entry?.code).toBe('agent_respond_to_denied')
    expect(chatClient.streamChat).not.toHaveBeenCalled()
  })

  it('a locked agent goes with every post, marked locked, until it is unlocked', async () => {
    publishRoomFeatures({ mention_picker: true, addressed_wake: true })
    renderMentions()
    typeInRoomBox('@Drif')
    fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Lås Driftsassistent' }))
    flush()
    expect(screen.getByRole('button', { name: 'Lås opp Driftsassistent' }).getAttribute('aria-pressed')).toBe('true')

    typeInRoomBox('Se på lista')
    submitRoomBox()
    await waitFor(() => expect(spacesClient.createSpacePost).toHaveBeenCalledTimes(1))
    expect(postedInput().content).toBe('Se på lista')
    expect(postedInput().mentions).toEqual({ people: [], agents: [{ subjectId: 'agent-1', source: 'locked' }] })
    // The post is accepted and the words leave the box; the lock stays on the bar for the next post.
    await waitFor(() => expect(roomBox().value).toBe(''))
    expect(screen.getByRole('list', { name: 'Adressert til' }).textContent).toContain('Driftsassistent')
  })

  it('editing the words so the @name leaves them removes the typed agent from the address bar', async () => {
    publishRoomFeatures({ mention_picker: true, addressed_wake: true })
    renderMentions()
    typeInRoomBox('@Drif')
    fireEvent.click(await screen.findByRole('option', { name: /Driftsassistent/ }))
    flush()
    expect(screen.getByRole('list', { name: 'Adressert til' })).toBeTruthy()

    typeInRoomBox('Hei alle')
    expect(screen.queryByRole('list', { name: 'Adressert til' })).toBeNull()
    expect(spacesClient.createSpacePost).not.toHaveBeenCalled()
  })
})

// WS07-S3: the room box and the thread pane's reply box each register their own unsaved-work probe. Words typed in
// either one hold a reload, and a probe is released when its box leaves the page.
describe('SpaceRoomComposer — unsaved-work probe (WS07-S3)', () => {
  it('holds a reload only while words are typed in the room box', () => {
    renderRoom()
    expect(hasUnsavedWorkProbes()).toBe(true)
    expect(hasUnsavedWork()).toBe(false)
    typeInRoomBox('Et halvferdig svar')
    expect(hasUnsavedWork()).toBe(true)
    typeInRoomBox('')
    expect(hasUnsavedWork()).toBe(false)
  })

  it('the room box and the thread pane box each hold a probe, so one never hides the other', () => {
    const view = render(() => (
      <>
        <SpaceRoomComposer spaceRef="room-1" roster={() => []} agents={() => []} threads={() => []} />
        <SpaceRoomComposer
          spaceRef="room-1"
          roster={() => []}
          agents={() => []}
          threads={() => []}
          replyTarget={() => ({ threadId: 'thr_7', title: 'Innkjøp' })}
          onClearReplyTarget={() => undefined}
          replyTargetFixed
        />
      </>
    ))
    expect(reloadGuardCount()).toBe(2)

    // Words only in the pane's reply box are still unsaved work.
    const [, paneBox] = screen.getAllByRole('textbox') as HTMLTextAreaElement[]
    fireEvent.input(paneBox as HTMLTextAreaElement, { target: { value: 'Svar i tråden' } })
    flush()
    expect(hasUnsavedWork()).toBe(true)

    view.unmount()
    expect(reloadGuardCount()).toBe(0)
    expect(hasUnsavedWorkProbes()).toBe(false)
  })

  it('an attachment in an otherwise empty box holds a reload until the chip is removed', async () => {
    vi.stubEnv('VITE_SPACE_TURN_ATTACHMENTS', '')
    const restoreObjectUrls = stubObjectUrls()
    try {
      publishRoomFeatures({ room_files: { turn_attachments: true } })
      renderRoom()
      expect(hasUnsavedWork()).toBe(false)

      pickRoomFiles([fixtureFile('Notat.txt', 'Lagerliste: 12 paller', 'text/plain')])
      expect(roomBox().value).toBe('')
      expect(hasUnsavedWork()).toBe(true)

      fireEvent.click(screen.getByRole('button', { name: 'Fjern Notat.txt' }))
      flush()
      await waitFor(() => expect(hasUnsavedWork()).toBe(false))
    } finally {
      clearSpaceRoomFeatures()
      restoreObjectUrls()
    }
  })

  it('a send in flight holds a reload after the box is cleared, and lets go once the turn is done', async () => {
    const stream = scriptedStream()
    renderRoom()
    typeInRoomBox('Oppsummer uka')
    submitRoomBox()
    await waitFor(() => expect(chatClient.streamChat).toHaveBeenCalledTimes(1))

    // With drafts off the box is emptied at Send, so only the send itself is left to hold the reload.
    expect(roomBox().value).toBe('')
    expect(hasUnsavedWork()).toBe(true)

    stream.handlers().onConnected?.({ requestId: 'req-1', threadId: 't-1' })
    stream.handlers().onDone?.({})
    stream.finish()
    await waitFor(() => expect(hasUnsavedWork()).toBe(false))
  })

  it('a message queued behind another member’s run holds a reload while the box is empty', async () => {
    publishSpaceThreads('room-1', [{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'running' }])
    flush()
    // The room allows steering, as in the mid-run steer cases: that is where a send waits for a busy thread.
    render(() => (
      <SpaceRoomComposer
        spaceRef="room-1"
        roster={() => []}
        agents={() => []}
        threads={() => []}
        replyTarget={() => ({ threadId: 't1', title: 'Innkjøp' })}
        agentWork={() => ({ live: false, text: false, steer_owner: true, stop_manager: false }) as never}
      />
    ))
    typeInRoomBox('Oppdater lista')
    submitRoomBox()

    expect(await screen.findByText(/Venter på at agenten blir ferdig/)).toBeTruthy()
    expect(chatClient.streamChat).not.toHaveBeenCalled()
    expect(roomBox().value).toBe('')
    expect(hasUnsavedWork()).toBe(true)
  })
})
