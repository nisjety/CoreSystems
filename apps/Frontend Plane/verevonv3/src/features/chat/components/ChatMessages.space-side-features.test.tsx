// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError } from '@/shared/api/api-error'

// A reasoning translation of a Space thread is not offered while the BYO gateway switch is on: the thread's turns may be
// written under a customer gateway or a subscription, and a translation there would run on the platform chain (DEC-V-R8).
// With the switch off, and on any thread that is not a Space thread, the translation is offered as before.

const translateMock = vi.hoisted(() =>
  vi.fn<(text: string, targetLocale: string, scope?: unknown) => Promise<string | null>>(),
)

vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/chat-client')>()),
  translateText: (text: string, targetLocale: string, scope?: unknown) => translateMock(text, targetLocale, scope),
}))

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

/**
 * Opens the reasoning's Insight tab, where the translate control is, and reports whether Translate is offered there.
 * The reasoning text is checked first, so the tab is open before the control is looked for.
 */
function translateOffered(props: { spaceThread?: boolean }): boolean {
  render(() => <ReasoningPopover message={turn} spaceThread={props.spaceThread} />)
  fireEvent.click(screen.getByRole('button', { name: /Detaljer|Details/ }))
  flush()
  fireEvent.click(screen.getByRole('button', { name: 'Innsikt' }))
  flush()
  expect(screen.getByText(REASONING)).toBeTruthy()
  return screen.queryByRole('button', { name: 'Oversett' }) !== null
}

describe('ReasoningPopover translation on a Space thread', () => {
  it('withholds the translation on a Space thread while the BYO gateway switch is on', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(translateOffered({ spaceThread: true })).toBe(false)
    expect(translateMock).not.toHaveBeenCalled()
  })

  it('still offers the translation on a Space thread while the switch is off, as before', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', '')
    expect(translateOffered({ spaceThread: true })).toBe(true)
  })

  it('still offers the translation on a thread that is not a Space thread while the switch is on', () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    expect(translateOffered({ spaceThread: false })).toBe(true)
  })

  it('shows the gateway refusal of the thread as its reason, with no retry', async () => {
    vi.stubEnv('VITE_BYO_GATEWAY', 'true')
    translateMock.mockRejectedValueOnce(new ApiError('m', 409, 'side_chat_thread_retention_refused'))
    render(() => <ReasoningPopover message={turn} />)
    fireEvent.click(screen.getByRole('button', { name: /Detaljer|Details/ }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Innsikt' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Oversett' }))
    await vi.waitFor(() => {
      flush()
      expect(screen.getByText(/ikke tilgjengelig for denne samtalen/)).toBeTruthy()
    })
    expect(screen.queryByRole('button', { name: 'Oversett' })).toBeNull()
    expect(translateMock).toHaveBeenCalledTimes(1)
  })
})
