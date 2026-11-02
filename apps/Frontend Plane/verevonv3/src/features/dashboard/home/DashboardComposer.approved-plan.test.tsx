// @vitest-environment jsdom

/**
 * WS14 S2a, "Run the approved plan": the third pill of the chat composer's
 * Ask / Do control. It appears only once a plan was approved, is selected by
 * default after the approval, and is never selectable (nor shown as selected)
 * on a route or in a mode that cannot execute.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { RetrievalScope } from '@/shared/actions/chat-source-scope'

const { mockListModels, mockListSubscriptions } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockListSubscriptions: vi.fn(),
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

vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>()
  return { ...actual, listChatGptSubscriptions: mockListSubscriptions }
})

vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))

import { DashboardComposer } from './DashboardComposer'

const planModeChanges: boolean[] = []
const executeSelects: number[] = []

type PageProps = {
  appearance?: 'chat' | 'default'
  approvedPlan?: { rungLabel: string; selected: boolean }
  sourceScope?: 'workspace' | 'conversation'
  retrievalScope?: RetrievalScope
  temporaryChat?: boolean
}

function ComposerPage(props: PageProps) {
  const [message, setMessage] = createSignal('')
  const [planMode, setPlanMode] = createSignal(false)
  return (
    <DashboardComposer
      appearance={props.appearance ?? 'chat'}
      approvedPlan={props.approvedPlan}
      message={message()}
      onApprovedPlanSelect={() => {
        executeSelects.push(1)
      }}
      onMessageChange={setMessage}
      onPlanModeChange={(value) => {
        planModeChanges.push(value)
        setPlanMode(value)
      }}
      onSubmit={() => undefined}
      planMode={planMode()}
      retrievalScope={props.retrievalScope}
      sourceScope={props.sourceScope}
      temporaryChat={props.temporaryChat}
    />
  )
}

function renderComposer(props: PageProps = {}) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage {...props} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routerProps) => <>{routerProps.children}</>}</TestRouter>)
}

afterEach(() => {
  planModeChanges.length = 0
  executeSelects.length = 0
  vi.clearAllMocks()
  window.localStorage.removeItem('verevon.ai-model-selection.v1:org-1')
})

const pill = () => screen.queryByRole('button', { name: /^(kjør plan|run plan)$/i }) as HTMLButtonElement | null
const ask = () => screen.getByRole('button', { name: /^(spør|ask)$/i }) as HTMLButtonElement
const doMode = () => screen.getByRole('button', { name: /^(utfør|do)$/i }) as HTMLButtonElement
const APPROVED = { rungLabel: 'skrive i arbeidsområdet', selected: true }

describe('the approved-plan pill', () => {
  it('is absent when no plan was approved', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer()
    await screen.findByRole('textbox')

    expect(pill()).toBeNull()
    expect(ask().getAttribute('aria-pressed')).toBe('true')
  })

  it('is absent from the default appearance, which keeps its single Ask/Do chip', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ appearance: 'default', approvedPlan: APPROVED })
    await screen.findByRole('textbox')

    expect(pill()).toBeNull()
  })

  it('is shown and selected, with Ask and Do not active, and says what the agent may now do', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: APPROVED })
    await screen.findByRole('textbox')

    const button = pill()
    expect(button).not.toBeNull()
    expect(button!.disabled).toBe(false)
    expect(button!.getAttribute('aria-pressed')).toBe('true')
    expect(button!.title).toMatch(/skrive i arbeidsområdet/)
    expect(button!.title).toMatch(/godkjente planen|approved plan/i)
    expect(ask().getAttribute('aria-pressed')).toBe('false')
    expect(doMode().getAttribute('aria-pressed')).toBe('false')
    expect(ask().classList.contains('is-active')).toBe(false)
    expect(doMode().classList.contains('is-active')).toBe(false)
  })

  it('is shown unselected, with Ask active, once the person chose Ask or Do instead', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: { rungLabel: 'skrive i arbeidsområdet', selected: false } })
    await screen.findByRole('textbox')

    expect(pill()!.getAttribute('aria-pressed')).toBe('false')
    expect(ask().getAttribute('aria-pressed')).toBe('true')
    expect(ask().classList.contains('is-active')).toBe(true)
  })

  it('calls the select handler when clicked, and nothing else', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: { rungLabel: 'skrive i arbeidsområdet', selected: false } })
    await screen.findByRole('textbox')

    fireEvent.click(pill()!)
    flush()

    expect(executeSelects).toHaveLength(1)
    expect(planModeChanges).toEqual([])
  })

  it('leaves Ask and Do to the plan-mode handler, not the select handler', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: APPROVED })
    await screen.findByRole('textbox')

    fireEvent.click(ask())
    fireEvent.click(doMode())
    flush()

    expect(planModeChanges).toEqual([false, true])
    expect(executeSelects).toEqual([])
  })

  it('is disabled with its reason, and never shown as selected, on the ChatGPT subscription', async () => {
    window.localStorage.setItem(
      'verevon.ai-model-selection.v1:org-1',
      JSON.stringify({
        model: 'gpt-5.6-terra',
        label: 'Terra',
        provider: 'openai-codex-subscription',
        subscriptionConnectionId: 'conn',
      }),
    )
    mockListModels.mockResolvedValue([
      { id: 'gpt-5.6-terra', name: 'Terra', provider: 'openai-codex-subscription', capabilities: ['tools', 'agentic'] },
    ])
    mockListSubscriptions.mockResolvedValue([{ id: 'conn', providerKey: 'openai-codex-subscription', status: 'active' }])
    renderComposer({ approvedPlan: APPROVED })

    await waitFor(() => expect(pill()?.disabled).toBe(true))
    expect(pill()!.title).toMatch(/abonnementet|subscription/i)
    expect(pill()!.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(pill()!)
    expect(executeSelects).toEqual([])
  })

  it('is disabled with its reason, and never shown as selected, in a temporary chat', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: APPROVED, temporaryChat: true })
    await screen.findByRole('textbox')

    expect(pill()!.disabled).toBe(true)
    expect(pill()!.title).toMatch(/midlertidig|temporary/i)
    expect(pill()!.getAttribute('aria-pressed')).toBe('false')
  })

  it('is disabled with its reason, and never shown as selected, when only the conversation is the source', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: APPROVED, sourceScope: 'conversation' })
    await screen.findByRole('textbox')

    expect(pill()!.disabled).toBe(true)
    expect(pill()!.title).toMatch(/samtalen|conversation/i)
    expect(pill()!.getAttribute('aria-pressed')).toBe('false')
  })

  it('is disabled with its reason, and never shown as selected, under a filtered retrieval scope', async () => {
    mockListModels.mockResolvedValue([])
    mockListSubscriptions.mockResolvedValue([])
    renderComposer({ approvedPlan: APPROVED, retrievalScope: 'web_only' })
    await screen.findByRole('textbox')

    expect(pill()!.disabled).toBe(true)
    expect(pill()!.title).toMatch(/kildefilter|source filter/i)
    expect(pill()!.getAttribute('aria-pressed')).toBe('false')
  })
})
