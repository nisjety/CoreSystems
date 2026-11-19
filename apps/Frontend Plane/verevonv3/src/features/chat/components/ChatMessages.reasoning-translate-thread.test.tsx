// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// A translation of a chat thread's reasoning names the thread and the model its connection serves (DEC-V-R8). The popover
// passes both to the translation, and a translation that names no thread is sent as it was before the feature named one.

type ThreadScope = { threadId?: string; model?: string }

const translateMock = vi.hoisted(() =>
  vi.fn<(text: string, targetLocale: string, scope?: ThreadScope) => Promise<string | null>>(),
)

vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  translateText: (text: string, targetLocale: string, scope?: ThreadScope) => translateMock(text, targetLocale, scope),
}))

import { ApiError } from '@/shared/api/api-error'
import { ReasoningPopover } from './ChatMessages'
import type { ChatTurn } from './chat-types'

const REASONING = 'The model compares the two offers and checks that the price includes VAT.'

const turn: ChatTurn = {
  id: 'asst-1',
  role: 'assistant',
  content: 'Svaret.',
  createdAt: '2026-11-17T08:00:00.000Z',
  streaming: false,
  tools: [],
  attachments: [],
  modelUsed: 'gpt-5-codex',
  reasoning: REASONING,
}

beforeEach(() => {
  vi.stubEnv('VITE_REASONING_TRANSLATE', 'true')
  translateMock.mockResolvedValue('Modellen sammenligner ...')
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
  translateMock.mockReset()
})

/** Opens the reasoning's Insight tab, where the translate control is, and presses Translate. */
async function translateReasoning(props: { featureThreadId?: string; connectionModel?: string | null }) {
  render(() => <ReasoningPopover message={turn} featureThreadId={props.featureThreadId} connectionModel={props.connectionModel} />)
  fireEvent.click(screen.getByRole('button', { name: /Detaljer|Details/ }))
  flush()
  fireEvent.click(screen.getByRole('button', { name: 'Innsikt' }))
  flush()
  fireEvent.click(screen.getByRole('button', { name: 'Oversett' }))
  await vi.waitFor(() => expect(translateMock).toHaveBeenCalledTimes(1))
  const call = translateMock.mock.calls[0]
  if (!call) throw new Error('the translation was not started')
  return call
}

describe('ReasoningPopover translation thread', () => {
  it('names the chat thread and the connection model the reasoning was written under', async () => {
    const [text, , scope] = await translateReasoning({ featureThreadId: 'thread-1', connectionModel: 'gpt-5-codex' })
    expect(text).toBe(REASONING)
    expect(scope).toEqual({ threadId: 'thread-1', model: 'gpt-5-codex' })
  })

  it('names no thread when the chat names none, so the translation runs as before', async () => {
    const [, , scope] = await translateReasoning({ featureThreadId: undefined, connectionModel: null })
    expect(scope?.threadId).toBeUndefined()
    expect(scope?.model).toBeUndefined()
  })

  // A zero-retention refusal (412 `zdr_*`) of the translation is the reason it is not available: its own words are shown, and
  // the translation is not offered again. It is not the generic "not available right now" that a switched-off route shows.
  it('shows the zero-retention refusal of the translation in its own words, with no retry', async () => {
    translateMock.mockRejectedValueOnce(new ApiError('Refused.', 412, 'zdr_connection_not_acknowledged'))
    render(() => <ReasoningPopover message={turn} featureThreadId="thread-1" connectionModel="gpt-5-codex" />)
    fireEvent.click(screen.getByRole('button', { name: /Detaljer|Details/ }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Innsikt' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Oversett' }))
    await vi.waitFor(() => expect(screen.getByText(/ikke bekreftet for Zero Data Retention/)).toBeTruthy())
    expect(screen.queryByText(/Oversettelse er ikke tilgjengelig akkurat nå/)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Oversett' })).toBeNull()
    expect(translateMock).toHaveBeenCalledTimes(1)
  })
})
