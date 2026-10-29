// @vitest-environment jsdom

/**
 * WS23 S6 composer behaviour: the connected-apps chip row in the context panel, the reason it is
 * off on a turn that cannot take connected apps, and what the submit payload carries. Mirrors
 * DashboardComposer.retrieval-scope.test.tsx's harness; the Space binding is a controlled prop, as
 * ChatPage wires it.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RetrievalScope } from '@/shared/actions/chat-source-scope'

const { mockListModels, mockListConnectorPickerTabs } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockListConnectorPickerTabs: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/integrations-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/integrations-client')>()
  return { ...actual, listConnectorPickerTabs: mockListConnectorPickerTabs }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

// A catalogue that advertises connected-app reads on a chat model, as the gateway does while its switch is on.
const advertisingCatalogue = [
  { id: 'verevon-test-model', name: 'Test model', modality: 'chat', capabilities: ['connector_scope'] },
]

const connectedAccounts = [
  { connectionId: 'conn-ms-1', providerKey: 'microsoft', label: 'Ada (Outlook)' },
  { connectionId: 'conn-g-1', providerKey: 'google', label: 'Ada Gmail' },
]

const submitted: Array<Record<string, unknown>> = []

type ComposerPageProps = {
  temporaryChat?: boolean
  retrievalScope?: RetrievalScope
  spaceScoped?: boolean
}

function ComposerPage(props: ComposerPageProps) {
  const [message, setMessage] = createSignal('Hva sendte Ada meg?')
  const [retrievalScope, setRetrievalScope] = createSignal<RetrievalScope>(props.retrievalScope ?? 'auto')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      retrievalScope={retrievalScope()}
      onRetrievalScopeChange={setRetrievalScope}
      temporaryChat={props.temporaryChat}
      spaceScoped={props.spaceScoped}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer(props: ComposerPageProps = {}) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage {...props} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routerProps) => <>{routerProps.children}</>}</TestRouter>)
}

function openContextPanel() {
  fireEvent.click(screen.getByRole('button', { name: /kontekst for dette svaret:/i }))
}

function submitComposerForm() {
  const form = screen.getByRole('textbox').closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
}

/** Lets the catalogue and the account list settle before a negative assertion. */
async function settle() {
  await new Promise((resolve) => setTimeout(resolve, 20))
}

beforeEach(() => {
  mockListModels.mockReset().mockResolvedValue(advertisingCatalogue)
  mockListConnectorPickerTabs.mockReset().mockResolvedValue(connectedAccounts)
})

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('composer connected apps (WS23 S6)', () => {
  it('shows no connected-app row, and reads no account, while the catalogue does not advertise the reads', async () => {
    mockListModels.mockResolvedValue([
      { id: 'verevon-test-model', name: 'Test model', modality: 'chat', capabilities: ['vision'] },
    ])
    renderComposer()
    openContextPanel()
    await settle()

    expect(screen.queryByText('Tilkoblede apper for denne meldingen')).toBeNull()
    expect(mockListConnectorPickerTabs).not.toHaveBeenCalled()
  })

  it('offers each connected account as a chip and sends only the picked connection', async () => {
    renderComposer()
    openContextPanel()
    const chip = await screen.findByRole('button', { name: /Ada \(Outlook\)/ })

    expect(chip.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(chip)
    flush()
    expect(chip.getAttribute('aria-pressed')).toBe('true')

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.connectorScope).toEqual(['conn-ms-1'])
  })

  it('omits connectorScope from the payload when no account is picked', async () => {
    renderComposer()
    openContextPanel()
    await screen.findByRole('button', { name: /Ada Gmail/ })

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).not.toHaveProperty('connectorScope')
  })

  it('clears the picks once the turn is sent, because a selection is consent for one turn only', async () => {
    renderComposer()
    openContextPanel()
    const chip = await screen.findByRole('button', { name: /Ada Gmail/ })
    fireEvent.click(chip)
    flush()

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.connectorScope).toEqual(['conn-g-1'])
    // The send closes the context panel, so the chip is read again from a freshly opened panel.
    await waitFor(() => {
      flush()
      expect(screen.queryByRole('button', { name: /Ada Gmail/ })).toBeNull()
    })
    openContextPanel()
    const reopened = await screen.findByRole('button', { name: /Ada Gmail/ })
    expect(reopened.getAttribute('aria-pressed')).toBe('false')
  })

  it.each([
    ['a temporary chat (zero retention)', { temporaryChat: true }, /lesing fra tilkoblede apper/i],
    ['a Space conversation', { spaceScoped: true }, /ikke tilgjengelige i Space-rom/i],
    ['a web-only source filter', { retrievalScope: 'web_only' as const }, /Tilkoblede apper er av når kildefilteret er kun nett/i],
    [
      'a knowledge-only source filter',
      { retrievalScope: 'knowledge_only' as const },
      /Tilkoblede apper er av når kildefilteret er kun organisasjon/i,
    ],
  ])('takes the chips away, and says why, on %s', async (_label, props, reason) => {
    renderComposer(props)
    openContextPanel()

    expect(await screen.findByText(reason)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /Ada Gmail/ })).toBeNull()
    expect(mockListConnectorPickerTabs).not.toHaveBeenCalled()
  })

  it('keeps the chips off a zero-retention turn even when the catalogue advertises the reads', async () => {
    renderComposer({ temporaryChat: true })
    openContextPanel()
    await screen.findByText(/lesing fra tilkoblede apper/i)

    submitComposerForm()
    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]).not.toHaveProperty('connectorScope')
  })
})
