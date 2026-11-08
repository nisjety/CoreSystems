// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { connectBundlesForSources } from '@/shared/integrations/connect-bundles'
import { CONNECT_INTENT_TTL_MS, readIntent, saveIntent, type ConnectIntent } from '@/shared/integrations/connect-intent'
import type { ChatConnectorSuggestion } from './chat-types'
import { ConnectorSuggestionCard, connectorSuggestionVisible } from './ConnectorSuggestionCard'

// Without a provider the card renders in its default locale, Norwegian.

const {
  listConnectorPickerTabsMock,
  listConnectionsMock,
  startConnectSessionMock,
  runDirectOauthWindowMock,
  reserveDirectOauthWindowMock,
} = vi.hoisted(() => ({
  listConnectorPickerTabsMock: vi.fn(),
  listConnectionsMock: vi.fn(),
  startConnectSessionMock: vi.fn(),
  runDirectOauthWindowMock: vi.fn(),
  reserveDirectOauthWindowMock: vi.fn(),
}))

vi.mock('@/shared/api/integrations-client', () => ({
  listConnectorPickerTabs: listConnectorPickerTabsMock,
  listConnections: listConnectionsMock,
  startConnectSession: startConnectSessionMock,
}))

vi.mock('@/shared/integrations/provider-auth-window', () => ({
  runDirectOauthWindow: runDirectOauthWindowMock,
  reserveDirectOauthWindow: reserveDirectOauthWindowMock,
}))

beforeEach(() => {
  listConnectorPickerTabsMock.mockReset().mockResolvedValue([])
  listConnectionsMock.mockReset().mockResolvedValue([])
  startConnectSessionMock.mockReset()
  runDirectOauthWindowMock.mockReset().mockResolvedValue(undefined)
  reserveDirectOauthWindowMock.mockReset().mockReturnValue(null)
  window.sessionStorage.clear()
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const suggestion = (overrides: Partial<ChatConnectorSuggestion> = {}): ChatConnectorSuggestion => ({
  suggestionId: 'sug_01',
  provider: 'google',
  reason: 'Your mail would answer this.',
  ...overrides,
})

const gmail = { connectionId: 'conn_gmail', providerKey: 'google', label: 'kari@example.test' }
const outlook = { connectionId: 'conn_outlook', providerKey: 'microsoft', label: 'Outlook' }

describe('where the card may show', () => {
  it('never in a Space thread or a temporary chat', () => {
    expect(connectorSuggestionVisible({})).toBe(true)
    expect(connectorSuggestionVisible({ spaceScoped: true })).toBe(false)
    expect(connectorSuggestionVisible({ temporary: true })).toBe(false)
  })

  it('renders nothing in a Space thread or a temporary chat, and reads no connections', () => {
    for (const hidden of [{ spaceScoped: true }, { temporary: true }]) {
      const { container, unmount } = render(() => (
        <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={vi.fn()} {...hidden} />
      ))
      expect(container.textContent).toBe('')
      unmount()
    }
    expect(listConnectorPickerTabsMock).not.toHaveBeenCalled()
  })
})

describe('ConnectorSuggestionCard', () => {
  it('offers Connect when the provider has no active connection, and runs the existing connect flow', async () => {
    startConnectSessionMock.mockResolvedValueOnce({ connectUrl: 'https://accounts.google.test/auth', sessionToken: 'sess_1' })
    listConnectorPickerTabsMock.mockResolvedValueOnce([outlook]).mockResolvedValueOnce([gmail, outlook])
    const onConnected = vi.fn()
    render(() => (
      <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={vi.fn()} onConnected={onConnected} />
    ))
    expect(screen.getByText('Your mail would answer this.')).toBeTruthy()
    const connect = await screen.findByRole('button', { name: 'Koble til Google' })
    await waitFor(() => expect((connect as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(connect)
    await waitFor(() =>
      expect(runDirectOauthWindowMock).toHaveBeenCalledWith({ connectUrl: 'https://accounts.google.test/auth', sessionToken: 'sess_1' }),
    )
    expect(startConnectSessionMock).toHaveBeenCalledWith('org-1', 'google', { bundles: connectBundlesForSources('google') })
    await waitFor(() => expect(onConnected).toHaveBeenCalledWith('google'))
    // Once connected, the new account is offered for the question.
    expect(await screen.findByRole('button', { name: 'Bruk kari@example.test for dette spørsmålet' })).toBeTruthy()
    expect(screen.getByRole('status').textContent).toContain('Google er koblet til.')
  })

  it('offers the connected account for this question and hands its id to the caller', async () => {
    listConnectorPickerTabsMock.mockResolvedValue([gmail])
    const onUse = vi.fn()
    render(() => <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={onUse} />)
    fireEvent.click(await screen.findByRole('button', { name: 'Bruk kari@example.test for dette spørsmålet' }))
    expect(onUse).toHaveBeenCalledWith('conn_gmail')
    expect(onUse).toHaveBeenCalledTimes(1)
    expect(startConnectSessionMock).not.toHaveBeenCalled()
    // The question goes again once; the card no longer offers it.
    await waitFor(() => expect(screen.queryByRole('button', { name: /Bruk/ })).toBeNull())
  })

  it('does not count another provider\'s account', async () => {
    listConnectorPickerTabsMock.mockResolvedValue([outlook])
    render(() => <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={vi.fn()} />)
    expect(await screen.findByRole('button', { name: 'Koble til Google' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Bruk Outlook/ })).toBeNull()
  })

  it('shows a failed connect and keeps the card', async () => {
    startConnectSessionMock.mockRejectedValueOnce(new Error('boom'))
    render(() => <ConnectorSuggestionCard suggestion={suggestion({ provider: 'microsoft' })} orgId="org-1" onUse={vi.fn()} />)
    const connect = await screen.findByRole('button', { name: 'Koble til Microsoft' })
    await waitFor(() => expect((connect as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(connect)
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(runDirectOauthWindowMock).not.toHaveBeenCalled()
    expect(screen.getByRole('button', { name: 'Koble til Microsoft' })).toBeTruthy()
  })

  it('falls back to Connect when the connections cannot be read', async () => {
    listConnectorPickerTabsMock.mockRejectedValue(new Error('unavailable'))
    render(() => <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={vi.fn()} />)
    expect(await screen.findByRole('button', { name: 'Koble til Google' })).toBeTruthy()
  })

  it('closes on Not now', async () => {
    const onDismiss = vi.fn()
    const { container } = render(() => (
      <ConnectorSuggestionCard suggestion={suggestion()} orgId="org-1" onUse={vi.fn()} onDismiss={onDismiss} />
    ))
    fireEvent.click(await screen.findByRole('button', { name: 'Ikke nå' }))
    expect(onDismiss).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(container.textContent).toBe(''))
  })

  it('renders a restored suggestion as a note with no actions, and reads no connections', () => {
    const { container } = render(() => (
      <ConnectorSuggestionCard suggestion={suggestion({ restored: true })} orgId="org-1" onUse={vi.fn()} />
    ))
    expect(container.querySelector('.verevon-connector-suggestion__note')?.textContent).toContain(
      'Your mail would answer this.',
    )
    expect(container.querySelector('button')).toBeNull()
    expect(listConnectorPickerTabsMock).not.toHaveBeenCalled()
  })
})

describe('a connect that resumes its question (EP-10 S9)', () => {
  const turn = { threadId: 'thr_1', prompt: 'What did Kari send me?' }
  const newGmail = { id: 'conn_new', providerKey: 'google', status: 'active' }
  const savedIntent = (createdAt: number): ConnectIntent => ({
    threadId: 'thr_1',
    suggestionId: 'sug_01',
    provider: 'google',
    prompt: turn.prompt,
    createdAt,
  })

  const connectAndWait = async () => {
    const connect = await screen.findByRole('button', { name: 'Koble til Google' })
    await waitFor(() => expect((connect as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(connect)
  }

  it('resends the question once after a successful connect', async () => {
    startConnectSessionMock.mockResolvedValueOnce({ connectUrl: 'https://accounts.google.test/auth', sessionToken: 'sess_1' })
    listConnectionsMock.mockResolvedValue([newGmail])
    const resume = vi.fn(() => true)
    render(() => (
      <ConnectorSuggestionCard
        suggestion={suggestion()}
        orgId="org-1"
        onUse={vi.fn()}
        connectTurn={() => turn}
        pendingConnectIntent={() => readIntent('thr_1')}
        resumeConnectIntent={resume}
      />
    ))
    await connectAndWait()
    await waitFor(() => expect(resume).toHaveBeenCalledTimes(1))
    expect(resume).toHaveBeenCalledWith(expect.objectContaining({ threadId: 'thr_1', prompt: turn.prompt }), 'conn_new', 'Google')
    expect(reserveDirectOauthWindowMock).toHaveBeenCalledTimes(1)
    // The connect owned its question: nothing is left to finish.
    expect(readIntent('thr_1')).toBeNull()
  })

  it('still runs the connect when storage throws', async () => {
    vi.spyOn(window, 'sessionStorage', 'get').mockImplementation(() => {
      throw new Error('blocked')
    })
    startConnectSessionMock.mockResolvedValueOnce({ connectUrl: 'https://accounts.google.test/auth', sessionToken: 'sess_1' })
    listConnectionsMock.mockResolvedValue([newGmail])
    const resume = vi.fn(() => true)
    render(() => (
      <ConnectorSuggestionCard
        suggestion={suggestion()}
        orgId="org-1"
        onUse={vi.fn()}
        connectTurn={() => turn}
        pendingConnectIntent={() => null}
        resumeConnectIntent={resume}
      />
    ))
    await connectAndWait()
    await waitFor(() => expect(resume).toHaveBeenCalledTimes(1))
    expect(runDirectOauthWindowMock).toHaveBeenCalledTimes(1)
  })

  it('shows Continue after a reload with a saved connect and sends nothing on render', async () => {
    saveIntent(savedIntent(Date.now()))
    listConnectionsMock.mockResolvedValue([newGmail])
    const resume = vi.fn(() => true)
    render(() => (
      <ConnectorSuggestionCard
        suggestion={suggestion({ restored: true })}
        orgId="org-1"
        onUse={vi.fn()}
        connectTurn={() => turn}
        pendingConnectIntent={() => readIntent('thr_1')}
        resumeConnectIntent={resume}
      />
    ))
    expect(await screen.findByRole('button', { name: 'Fortsett' })).toBeTruthy()
    expect(resume).not.toHaveBeenCalled()
    expect(listConnectionsMock).not.toHaveBeenCalled()
  })

  it('sends once on two quick Continue clicks', async () => {
    saveIntent(savedIntent(Date.now()))
    listConnectionsMock.mockResolvedValue([newGmail])
    const resume = vi.fn(() => true)
    render(() => (
      <ConnectorSuggestionCard
        suggestion={suggestion({ restored: true })}
        orgId="org-1"
        onUse={vi.fn()}
        connectTurn={() => turn}
        pendingConnectIntent={() => readIntent('thr_1')}
        resumeConnectIntent={resume}
      />
    ))
    const continueButton = await screen.findByRole('button', { name: 'Fortsett' })
    fireEvent.click(continueButton)
    fireEvent.click(continueButton)
    await waitFor(() => expect(resume).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Fortsett' })).toBeNull())
    expect(resume).toHaveBeenCalledTimes(1)
  })

  it('shows no finish state for a connect saved 16 minutes ago', () => {
    const now = Date.now()
    saveIntent(savedIntent(now - 16 * 60 * 1000))
    expect(16 * 60 * 1000).toBeGreaterThan(CONNECT_INTENT_TTL_MS)
    render(() => (
      <ConnectorSuggestionCard
        suggestion={suggestion({ restored: true })}
        orgId="org-1"
        onUse={vi.fn()}
        connectTurn={() => turn}
        pendingConnectIntent={() => readIntent('thr_1')}
        resumeConnectIntent={vi.fn(() => true)}
      />
    ))
    expect(screen.queryByRole('button', { name: 'Fortsett' })).toBeNull()
  })
})
