import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flush } from 'solid-js'
import { ApiError } from '@/shared/api/api-error'
import {
  SideChatPanel,
  isPermanentSideChatRefusal,
  sideChatConnectionModel,
  type SideChatPanelProps,
} from './SideChatPanel'
import type { ChatTurn } from './chat-types'

const { askSideChatMock } = vi.hoisted(() => ({ askSideChatMock: vi.fn() }))
const { language } = vi.hoisted(() => ({ language: { value: 'en' as 'en' | 'no' } }))
// The chat client's other exports stay real: the panel's words come from the chat normalizers, which read them when they load.
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  askSideChat: askSideChatMock,
}))
vi.mock('@/shared/i18n', () => ({
  useI18n: () => ({
    tr: (noText: string, enText: string) => (language.value === 'no' ? noText : enText),
    locale: () => language.value,
  }),
}))

const ANSWER = { answer: 'Det er et svar.', modelUsed: 'gpt-5-codex', usage: { inputTokens: 1, outputTokens: 1, costUsd: null } }

afterEach(() => {
  cleanup()
  askSideChatMock.mockReset()
  language.value = 'en'
})

function turn(overrides: Partial<ChatTurn> & Pick<ChatTurn, 'id' | 'role'>): ChatTurn {
  return {
    content: '',
    createdAt: '2026-11-13T10:00:00.000Z',
    streaming: false,
    tools: [],
    attachments: [],
    ...overrides,
  }
}

function renderPanel(overrides: Partial<SideChatPanelProps> = {}) {
  const props: SideChatPanelProps = {
    threadId: 'thread-1',
    hidden: false,
    connectionModel: null,
    onSpinOff: vi.fn(),
    ...overrides,
  }
  return render(() => <SideChatPanel {...props} />)
}

/**
 * Opens the drawer, types the question and sends it. Solid applies each change on the next flush, so each step is flushed.
 * The labels are found in either language, so a test may render in Norwegian.
 */
function askQuestion(text: string) {
  fireEvent.click(screen.getByRole('button', { name: /Ask aside|Spør ved siden av/ }))
  flush()
  const box = screen.getByRole('textbox', { name: /Question|Spørsmål/ }) as HTMLTextAreaElement
  fireEvent.input(box, { target: { value: text } })
  flush()
  fireEvent.submit(box.closest('form') as HTMLFormElement)
  flush()
  return box
}

function refusal(code: string, status: number, message = 'English ops text'): ApiError {
  return new ApiError(message, status, code)
}

describe('the model the side question names', () => {
  it('names the connection thread model with the question', async () => {
    askSideChatMock.mockResolvedValue(ANSWER)
    renderPanel({ connectionModel: 'gpt-5-codex' })
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(askSideChatMock).toHaveBeenCalledWith('thread-1', 'Hva ble sagt?', 'gpt-5-codex'))
    expect(await screen.findByText('Det er et svar.')).toBeTruthy()
  })

  it('sends the question alone on a platform thread, with no model', async () => {
    askSideChatMock.mockResolvedValue(ANSWER)
    renderPanel({ connectionModel: null })
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(askSideChatMock).toHaveBeenCalledTimes(1))
    expect(askSideChatMock.mock.calls[0]?.[0]).toBe('thread-1')
    expect(askSideChatMock.mock.calls[0]?.[2]).toBeUndefined()
  })
})

describe('sideChatConnectionModel', () => {
  it('is null for a platform thread, whose turns name no connection', () => {
    expect(
      sideChatConnectionModel([
        turn({ id: 'u1', role: 'user', model: 'verevon-balance' }),
        turn({ id: 'a1', role: 'assistant', model: 'verevon-balance', modelUsed: 'verevon-balance' }),
      ]),
    ).toBeNull()
    expect(sideChatConnectionModel([])).toBeNull()
  })

  it('is the model of the newest turn written under a customer gateway', () => {
    expect(
      sideChatConnectionModel([
        turn({ id: 'u1', role: 'user', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
      ]),
    ).toBe('gateway-a')
  })

  it('keeps the connection model when a later turn went to the platform', () => {
    // The thread still holds its connection's turns, so the side question must name that connection's model.
    expect(
      sideChatConnectionModel([
        turn({ id: 'u1', role: 'user', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'u2', role: 'user', model: 'verevon-balance' }),
        turn({ id: 'a2', role: 'assistant', model: 'verevon-balance' }),
      ]),
    ).toBe('gateway-a')
  })

  it('takes the newer model when the connection moved to another one', () => {
    expect(
      sideChatConnectionModel([
        turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'u2', role: 'user', model: 'gateway-b', modelConnectionId: 'conn-gw-1' }),
      ]),
    ).toBe('gateway-b')
  })

  it('reads a ChatGPT subscription turn by its subscription connection', () => {
    expect(
      sideChatConnectionModel([
        turn({
          id: 'a1',
          role: 'assistant',
          model: 'gpt-5-codex',
          provider: 'openai-codex-subscription',
          subscriptionConnectionId: 'conn-sub-1',
        }),
      ]),
    ).toBe('gpt-5-codex')
  })

  it('is null when the connection turn names no model, and a blank connection id is no connection', () => {
    expect(sideChatConnectionModel([turn({ id: 'a1', role: 'assistant', modelConnectionId: 'conn-gw-1' })])).toBeNull()
    expect(sideChatConnectionModel([turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: '  ' })])).toBeNull()
  })

  it('reads the model of the newest turn on the same connection that names one, when the newest names none', () => {
    expect(
      sideChatConnectionModel([
        turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'u2', role: 'user', modelConnectionId: 'conn-gw-1' }),
      ]),
    ).toBe('gateway-a')
  })

  it('never reads a turn on another connection for the model', () => {
    // A thread written under two connections is refused by the gateway, so the newest connection's own turns decide the model.
    expect(
      sideChatConnectionModel([
        turn({ id: 'a1', role: 'assistant', model: 'gateway-a', modelConnectionId: 'conn-gw-1' }),
        turn({ id: 'u2', role: 'user', modelConnectionId: 'conn-gw-2' }),
      ]),
    ).toBeNull()
  })
})

describe('which refusals are permanent', () => {
  it.each([
    'model_not_entitled',
    'side_chat_model_required',
    'connection_ambiguous',
    'side_chat_thread_retention_refused',
    'scope_not_available_in_space',
    'zdr_posture_mismatch',
    'zdr_connection_not_acknowledged',
    'zdr_waiver_route_mismatch',
    'subscription_reauth_required',
    'subscription_usage_limit',
    'subscription_bad_request',
    'prompt_too_long',
  ])('treats %s as permanent: asking again is refused the same way', (code) => {
    expect(isPermanentSideChatRefusal(refusal(code, 409))).toBe(true)
  })

  it.each([
    'zdr_decision_unavailable',
    'zdr_inference_failed',
    'zdr_some_code_this_spa_does_not_know',
    'model_entitlement_unavailable',
    'delegated_auth_unavailable',
    'rate_limited',
    'side_chat_no_answer',
    'subscription_upstream_unavailable',
    'subscription_route_unavailable',
  ])('treats %s as transient: the retry line stands', (code) => {
    expect(isPermanentSideChatRefusal(refusal(code, 503))).toBe(false)
  })

  it('treats a failure with no code, and a plain network error, as transient', () => {
    expect(isPermanentSideChatRefusal(new ApiError('x', 500, null))).toBe(false)
    expect(isPermanentSideChatRefusal(new TypeError('Failed to fetch'))).toBe(false)
    expect(isPermanentSideChatRefusal(undefined)).toBe(false)
  })
})

describe('the refusal the panel shows', () => {
  // A connection thread, the case whose refusals name the connection's model.
  beforeEach(() => {
    renderPanel({ connectionModel: 'gateway-a' })
  })

  it('shows the model-entitlement words for model_not_entitled, with no retry, and keeps the question', async () => {
    askSideChatMock.mockRejectedValue(refusal('model_not_entitled', 403))
    const box = askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'This model is not available to you. The request was not sent to a model. Choose another model and send again.',
      ),
    )
    expect(screen.queryByText(/try again/i)).toBeNull()
    expect(box.value).toBe('Hva ble sagt?')
  })

  it('shows the zero-retention words for a permanent zdr refusal, with no retry', async () => {
    askSideChatMock.mockRejectedValue(refusal('zdr_posture_mismatch', 403))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'The zero data retention setting on the credentials for this request does not match. The request was not sent to any model.',
      ),
    )
    expect(screen.queryByText(/try again/i)).toBeNull()
  })

  it.each([
    [
      'side_chat_model_required',
      400,
      'A side question in a chat on its own connection needs the model that connection serves, and none was given. The request was not sent to a model.',
    ],
    [
      'connection_ambiguous',
      409,
      'Side questions are not available in a chat written under more than one connection. The request was not sent to a model.',
    ],
    [
      'side_chat_thread_retention_refused',
      409,
      'Side questions are not available for a chat that holds a run under zero data retention or under a waiver. The request was not sent to a model.',
    ],
    [
      'scope_not_available_in_space',
      409,
      'Side questions are not available in a chat in a shared Space. The request was not sent to a model.',
    ],
  ])('shows the catalogue words for %s, with no retry', async (code, status, copy) => {
    askSideChatMock.mockRejectedValue(refusal(code, status))
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe(copy))
    expect(screen.queryByText(/try again/i)).toBeNull()
  })

  it('shows the Norwegian words for a permanent refusal in Norwegian', async () => {
    cleanup()
    language.value = 'no'
    renderPanel({ connectionModel: 'gateway-a' })
    askSideChatMock.mockRejectedValueOnce(refusal('model_not_entitled', 403))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'Denne modellen er ikke tilgjengelig for deg. Forespørselen ble ikke sendt til en modell. Velg en annen modell og send på nytt.',
      ),
    )
  })

  it('keeps the retry line for a transient zero-retention refusal, and keeps the question', async () => {
    askSideChatMock.mockRejectedValue(refusal('zdr_decision_unavailable', 503))
    const box = askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('Could not answer right now. Try again.'))
    expect(box.value).toBe('Hva ble sagt?')
  })

  it('keeps the retry line for a transient credential outage, and the retry sends the question again', async () => {
    askSideChatMock
      .mockRejectedValueOnce(refusal('delegated_auth_unavailable', 503))
      .mockResolvedValueOnce(ANSWER)
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('Could not answer right now. Try again.'))
    fireEvent.submit(screen.getByRole('textbox', { name: 'Question' }).closest('form') as HTMLFormElement)
    flush()
    expect(await screen.findByText('Det er et svar.')).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(askSideChatMock).toHaveBeenCalledTimes(2)
  })

  it('keeps the retry line for a network failure', async () => {
    askSideChatMock.mockRejectedValue(new TypeError('Failed to fetch'))
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('Could not answer right now. Try again.'))
  })
})

describe('the ChatGPT subscription refusals the panel shows', () => {
  // A ChatGPT subscription thread: the case whose refusals come from the subscription route.
  beforeEach(() => {
    renderPanel({ connectionModel: 'gpt-5-codex' })
  })

  it('shows the reconnect words for subscription_reauth_required, with no retry line of the panel, and keeps the question', async () => {
    askSideChatMock.mockRejectedValue(refusal('subscription_reauth_required', 409))
    const box = askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'Your ChatGPT connection needs to be renewed. Reconnect ChatGPT under Profile → ChatGPT subscription, then try again. No other model was used.',
      ),
    )
    expect(screen.queryByText('Could not answer right now. Try again.')).toBeNull()
    expect(box.value).toBe('Hva ble sagt?')
  })

  it('shows the Norwegian reconnect words for subscription_reauth_required, with no retry line of the panel', async () => {
    cleanup()
    language.value = 'no'
    renderPanel({ connectionModel: 'gpt-5-codex' })
    askSideChatMock.mockRejectedValueOnce(refusal('subscription_reauth_required', 409))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt under Profil → ChatGPT-abonnement, og prøv igjen. Ingen annen modell ble brukt.',
      ),
    )
    expect(screen.queryByText('Kunne ikke svare nå. Prøv igjen.')).toBeNull()
  })

  it('shows the words for subscription_bad_request, with no retry line of the panel', async () => {
    askSideChatMock.mockRejectedValue(refusal('subscription_bad_request', 422))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'ChatGPT rejected the request. Change or shorten the message, then try again. No other model was used.',
      ),
    )
    expect(screen.queryByText('Could not answer right now. Try again.')).toBeNull()
  })

  it('shows the neutral usage-limit words when the refusal carries no reset time', async () => {
    askSideChatMock.mockRejectedValue(refusal('subscription_usage_limit', 429))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'The connected ChatGPT plan has reached its Codex usage limit. The reset time is unknown; check your usage in ChatGPT and try again later. No other model was used.',
      ),
    )
  })

  it('names the reset time of a usage limit whose refusal carries a wait, with no retry line of the panel', async () => {
    askSideChatMock.mockRejectedValue(new ApiError('English ops text', 429, 'subscription_usage_limit', { retryAfterSeconds: 3600 }))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toMatch(
        /^The connected ChatGPT plan has reached its Codex usage limit\. It resets .+\. No other model was used\.$/,
      ),
    )
    expect(screen.queryByText('Could not answer right now. Try again.')).toBeNull()
  })

  it('shows the words for prompt_too_long, with no retry line of the panel', async () => {
    askSideChatMock.mockRejectedValue(refusal('prompt_too_long', 413))
    askQuestion('Hva ble sagt?')
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'This conversation is too long for the model, even after compacting it. Start a new thread.',
      ),
    )
  })

  it('keeps the retry line for a transient ChatGPT outage, and keeps the question', async () => {
    askSideChatMock.mockRejectedValue(refusal('subscription_upstream_unavailable', 503))
    const box = askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe('Could not answer right now. Try again.'))
    expect(box.value).toBe('Hva ble sagt?')
  })
})

describe("the side chat's own words, in Norwegian, for the refusals the global table does not word", () => {
  it.each([
    [
      'connection_ambiguous',
      'Sidespørsmål er ikke tilgjengelig i en samtale som er skrevet under mer enn én tilkobling. Forespørselen ble ikke sendt til en modell.',
    ],
    [
      'scope_not_available_in_space',
      'Sidespørsmål er ikke tilgjengelig i en samtale i et delt Space. Forespørselen ble ikke sendt til en modell.',
    ],
  ])('shows the Norwegian words for %s, with no retry line', async (code, copy) => {
    language.value = 'no'
    renderPanel({ connectionModel: 'gateway-a' })
    askSideChatMock.mockRejectedValueOnce(refusal(code, 409))
    askQuestion('Hva ble sagt?')
    await waitFor(() => expect(screen.getByRole('alert').textContent).toBe(copy))
    expect(screen.queryByText('Kunne ikke svare nå. Prøv igjen.')).toBeNull()
  })
})
