// @vitest-environment jsdom

/**
 * "Les høyt" on governed text-to-speech.
 *
 * It used to call `speechSynthesis.speak` with the raw markdown and a fixed
 * nb-NO language. Chrome's and Edge's online voices send that text to the
 * browser vendor, outside Verevon's provider and audit controls. It now posts
 * the speakable text to the BFF speech route, pinned to Azure Speech, plays
 * the mp3 from a `blob:` URL, can be stopped, fails closed with a notice, and
 * is disabled in Temporary chat.
 */

import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { speechPlayer } from '@/features/voice/speech-playback'
import { I18nProvider } from '@/shared/i18n'
import { AssistantMessage } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const turn = (overrides: Partial<ChatTurn> = {}): ChatTurn => ({
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret er **ferdig** [1], se [kilden](https://example.com/kilde).',
  createdAt: '2026-09-24T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  ...overrides,
})

const props = (message: ChatTurn, temporaryChat = false) => ({
  copied: false,
  message,
  onBranch: vi.fn(),
  onCopy: vi.fn(),
  onFeedback: vi.fn(async () => true),
  onRegenerate: vi.fn(),
  onApprovalDecision: vi.fn(),
  onApprovePlan: vi.fn(),
  onViewSteps: vi.fn(),
  temporaryChat,
})

// The browser's own speech synthesis, as spies that throw: "Les høyt" must never reach it.
const webSpeech = {
  SpeechSynthesisUtterance: vi.fn(function SpeechSynthesisUtterance() {
    throw new Error('SpeechSynthesisUtterance must not be used')
  }),
  speak: vi.fn(() => {
    throw new Error('speechSynthesis.speak must not be used')
  }),
}

type Played = { element: HTMLMediaElement; src: string }
let played: Played[] = []
let objectUrls = 0
const originalUrl = { create: URL.createObjectURL, revoke: URL.revokeObjectURL }
const revokeObjectURL = vi.fn()

function speechResponse(): Response {
  return new Response(JSON.stringify({ audio_base64: btoa('fake mp3'), format: 'mp3', provider_used: 'azure-speech' }), {
    status: 200,
    headers: { 'content-type': 'application/json' },
  })
}

const fetchMock = vi.fn(async (_url: string, _init?: RequestInit) => speechResponse())

beforeEach(() => {
  played = []
  vi.stubGlobal('fetch', fetchMock)
  vi.stubGlobal('SpeechSynthesisUtterance', webSpeech.SpeechSynthesisUtterance)
  vi.stubGlobal('speechSynthesis', { speak: webSpeech.speak, cancel: webSpeech.speak, getVoices: webSpeech.speak })
  URL.createObjectURL = vi.fn(() => `blob:verevon/${++objectUrls}`)
  URL.revokeObjectURL = revokeObjectURL
  vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(function play(this: HTMLMediaElement) {
    played.push({ element: this, src: this.src })
    return Promise.resolve()
  })
  vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => undefined)
})

afterEach(() => {
  speechPlayer().stop()
  expect(webSpeech.SpeechSynthesisUtterance).not.toHaveBeenCalled()
  expect(webSpeech.speak).not.toHaveBeenCalled()
  URL.createObjectURL = originalUrl.create
  URL.revokeObjectURL = originalUrl.revoke
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  fetchMock.mockReset()
  fetchMock.mockImplementation(async () => speechResponse())
  window.localStorage.clear()
})

function openMenu() {
  fireEvent.click(screen.getByRole('button', { name: 'Flere handlinger' }))
  flush()
}

/**
 * Tap an item in the message menu. Returns how many play() calls had happened
 * by the end of the tap: a silent unlock clip may play inside it (only the
 * first time, since the player is shared), the reply only after synthesis.
 */
function tapMenuItem(name: string | RegExp): number {
  fireEvent.click(screen.getByRole('button', { name }))
  flush()
  return played.length
}

/** The reply's own playback, after the synthesis call came back. */
async function replyPlayback(afterTap: number): Promise<Played> {
  await waitFor(() => expect(played.length).toBeGreaterThan(afterTap))
  return played.at(-1)!
}

describe('"Les høyt" reads replies through governed speech', () => {
  it('sends the speakable text to Azure Speech in the UI language and plays it from a blob: URL', async () => {
    render(() => <AssistantMessage {...props(turn())} />)
    openMenu()
    const afterTap = tapMenuItem('Les høyt')

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const [url, init] = fetchMock.mock.calls[0]!
    expect(url).toMatch(/\/api\/v1\/ai\/speech$/)
    expect(JSON.parse(String(init?.body))).toEqual({
      operation: 'tts',
      text: 'Svaret er ferdig, se kilden.',
      language: 'nb-NO',
      voice: 'nb-NO-PernilleNeural',
      format: 'mp3',
      provider: 'azure-speech',
    })
    const playback = await replyPlayback(afterTap)
    // Everything that played, the unlock clip included, was blob: audio.
    expect(played.every((entry) => entry.src.startsWith('blob:'))).toBe(true)

    // While it plays, the same menu item stops it; once it has played, it reads again.
    openMenu()
    expect(screen.getByRole('button', { name: 'Stopp opplesing' })).toBeTruthy()
    playback.element.dispatchEvent(new Event('ended'))
    await waitFor(() => expect(speechPlayer().playingId()).toBeNull())
    await waitFor(() => expect(screen.getByRole('button', { name: 'Les høyt' })).toBeTruthy())
    expect(revokeObjectURL).toHaveBeenCalledWith(playback.src)
  })

  it('stops reading aloud on "Stopp opplesing"', async () => {
    render(() => <AssistantMessage {...props(turn())} />)
    openMenu()
    await replyPlayback(tapMenuItem('Les høyt'))

    openMenu()
    fireEvent.click(screen.getByRole('button', { name: 'Stopp opplesing' }))
    flush()
    expect(speechPlayer().playingId()).toBeNull()
    expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled()
    openMenu()
    expect(screen.getByRole('button', { name: 'Les høyt' })).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('uses the English voice for an English UI', async () => {
    window.localStorage.setItem('verevon.locale', 'en')
    render(() => (
      <I18nProvider>
        <AssistantMessage {...props(turn({ content: 'The answer is ready.' }))} />
      </I18nProvider>
    ))
    fireEvent.click(screen.getByRole('button', { name: /More actions|Flere handlinger/ }))
    flush()
    tapMenuItem('Read aloud')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    expect(JSON.parse(String(fetchMock.mock.calls[0]![1]?.body))).toMatchObject({
      text: 'The answer is ready.',
      language: 'en-GB',
      voice: 'en-GB-SoniaNeural',
      provider: 'azure-speech',
    })
  })

  it('fails closed with a notice when speech is unavailable, and the reply stays on screen', async () => {
    fetchMock.mockImplementation(async () =>
      new Response(JSON.stringify({ error: { code: 'upstream_unavailable', message: 'down' } }), {
        status: 503,
        headers: { 'content-type': 'application/json' },
      }),
    )
    const { container } = render(() => <AssistantMessage {...props(turn())} />)
    openMenu()
    const afterTap = tapMenuItem('Les høyt')

    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('Opplesing er utilgjengelig. Svaret vises i chatten.')
    expect(container.textContent).toContain('Svaret er')
    // No reply audio ever played.
    expect(played).toHaveLength(afterTap)
    expect(speechPlayer().playingId()).toBeNull()
  })

  it('is off in Temporary chat, says why to everyone, and sends nothing', async () => {
    render(() => <AssistantMessage {...props(turn(), true)} />)
    openMenu()
    const item = screen.getByRole('button', { name: 'Les høyt' }) as HTMLButtonElement
    // Focusable (aria-disabled), with the reason as its description, not only a tooltip.
    expect(item.getAttribute('aria-disabled')).toBe('true')
    expect(item.disabled).toBe(false)
    expect(item.title).toMatch(/ikke tilgjengelig i midlertidig chat/)
    const description = document.getElementById(item.getAttribute('aria-describedby') ?? '')
    expect(description?.textContent).toBe(
      'Opplesing er ikke tilgjengelig i midlertidig chat ennå. Tale kan ikke behandles uten lagring.',
    )
    expect(screen.queryByRole('status')).toBeNull()

    fireEvent.click(item)
    flush()
    await Promise.resolve()
    expect(fetchMock).not.toHaveBeenCalled()
    expect(played).toHaveLength(0)
    // A press shows the reason where a touch user can see it.
    expect(screen.getByRole('status').textContent).toBe(
      'Opplesing er ikke tilgjengelig i midlertidig chat ennå. Tale kan ikke behandles uten lagring.',
    )
  })
})
