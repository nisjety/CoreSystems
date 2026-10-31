// @vitest-environment jsdom

// D1-815 (ZDR live proof, 2026-10-28): Søk is on by default, and model-gateway
// refuses web search on a zero-retention turn (`zdr_feature_not_supported`,
// feature `web_search`) before any model call. So every ordinary turn of an org
// with ZDR on, and every Temporary chat turn, went out with web search and was
// refused. Under zero retention the composer now sends no web search and shows
// Søk disabled with the reason, as it already does on the subscription route.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

const { mockGetOrganizationZdr } = vi.hoisted(() => ({
  mockGetOrganizationZdr: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: vi.fn().mockResolvedValue([]) }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

vi.mock('@/shared/api/organization-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/organization-client')>()
  return { ...actual, getOrganizationZdr: mockGetOrganizationZdr }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function renderComposer(options: { temporaryChat?: boolean } = {}) {
  function ComposerPage() {
    const [message, setMessage] = createSignal('')
    // The chat controller's default (`readBrowseWebPreference`): Søk on.
    const [browseWeb, setBrowseWeb] = createSignal(true)
    return (
      <DashboardComposer
        appearance="chat"
        browseWeb={browseWeb()}
        message={message()}
        onBrowseWebChange={setBrowseWeb}
        onMessageChange={setMessage}
        onSubmit={(payload) => {
          submitted.push(payload as unknown as Record<string, unknown>)
        }}
        temporaryChat={options.temporaryChat ?? false}
      />
    )
  }
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routed) => <>{routed.children}</>}</TestRouter>)
}

const searchButton = () => screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement

async function send(text: string): Promise<Record<string, unknown>> {
  const textbox = screen.getByRole('textbox')
  fireEvent.input(textbox, { target: { value: text } })
  flush()
  fireEvent.submit(textbox.closest('form')!)
  await waitFor(() => expect(submitted).toHaveLength(1))
  return submitted[0] ?? {}
}

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('zero retention sends no web search (D1-815)', () => {
  it('an org with ZDR on: Søk is off and disabled with the reason, and the turn carries no web search', async () => {
    mockGetOrganizationZdr.mockResolvedValue(true)
    renderComposer()

    await waitFor(() => expect(searchButton().disabled).toBe(true))
    expect(mockGetOrganizationZdr).toHaveBeenCalledWith('org-1')
    expect(searchButton().getAttribute('aria-pressed')).toBe('false')
    expect(searchButton().title).toMatch(/nettsøk er ikke tilgjengelig med nullagring/i)

    // The controller reads web search from `tools` (`payload.tools.includes('search')`).
    expect(await send('Hei')).not.toHaveProperty('tools', expect.arrayContaining(['search']))
  })

  it('an org with ZDR off keeps Søk as the user set it', async () => {
    mockGetOrganizationZdr.mockResolvedValue(false)
    renderComposer()

    await waitFor(() => expect(mockGetOrganizationZdr).toHaveBeenCalledWith('org-1'))
    flush()
    expect(searchButton().disabled).toBe(false)
    expect(searchButton().getAttribute('aria-pressed')).toBe('true')

    expect(await send('Hei')).toHaveProperty('tools', expect.arrayContaining(['search']))
  })

  it('a Temporary chat turn carries no web search, also in an org without ZDR', async () => {
    mockGetOrganizationZdr.mockResolvedValue(false)
    renderComposer({ temporaryChat: true })

    await waitFor(() => expect(mockGetOrganizationZdr).toHaveBeenCalledWith('org-1'))
    flush()
    expect(searchButton().disabled).toBe(true)
    expect(searchButton().getAttribute('aria-pressed')).toBe('false')
    expect(searchButton().title).toMatch(/nettsøk er ikke tilgjengelig med nullagring/i)
  })

  it('a failed org read leaves Søk as the user set it: the gateway refusal stays the guard', async () => {
    mockGetOrganizationZdr.mockRejectedValue(new Error('org read failed'))
    renderComposer()

    await waitFor(() => expect(mockGetOrganizationZdr).toHaveBeenCalledWith('org-1'))
    flush()
    expect(searchButton().disabled).toBe(false)
    expect(searchButton().getAttribute('aria-pressed')).toBe('true')
  })
})
