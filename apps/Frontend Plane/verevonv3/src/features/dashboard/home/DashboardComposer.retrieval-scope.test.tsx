// @vitest-environment jsdom

/**
 * chat-wave2 scope spec: the composer's per-turn retrieval-source filter.
 * Covers §3's explicit ask — the picker disables/re-enables Browse-web and
 * Deep-research per scope, resets to 'auto' when the user switches to "Kun
 * samtalen", and the submit payload carries the right value — plus the
 * omit-when-default case.
 *
 * Drives `retrievalScope`/`onRetrievalScopeChange` as CONTROLLED props
 * (mirroring how `ChatPage.tsx` wires them) rather than through the context
 * popover's own `<select>`: the popover's toggle button is exercised
 * elsewhere, and controlling scope directly here keeps this file about the
 * scope behavior itself, not about reproducing that click interaction.
 */

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RetrievalScope } from '@/shared/actions/chat-source-scope'

const { mockListModels } = vi.hoisted(() => ({ mockListModels: vi.fn() }))

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: mockListModels,
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage(props: { retrievalScope?: RetrievalScope; sourceScope?: 'workspace' | 'conversation' }) {
  const [message, setMessage] = createSignal('hei')
  const [retrievalScope, setRetrievalScope] = createSignal<RetrievalScope>(props.retrievalScope ?? 'auto')
  const [sourceScope, setSourceScope] = createSignal<'workspace' | 'conversation'>(props.sourceScope ?? 'workspace')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      retrievalScope={retrievalScope()}
      onRetrievalScopeChange={setRetrievalScope}
      sourceScope={sourceScope()}
      onSourceScopeChange={setSourceScope}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer(props: { retrievalScope?: RetrievalScope; sourceScope?: 'workspace' | 'conversation' } = {}) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage {...props} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(routerProps) => <>{routerProps.children}</>}</TestRouter>)
}

/**
 * "Dyp research" also happens to be a response-mode dropdown option's label
 * (a different, unrelated control) — scope the query to the toolbar's own
 * research TOGGLE by its class, so this file's assertions never depend on
 * whichever one `getByRole` happens to prefer.
 */
function deepResearchToggle(): HTMLButtonElement {
  const button = document.querySelector<HTMLButtonElement>(
    'button.dashboard-composer-toolbar-icon[aria-label*="research" i], button.dashboard-composer-toolbar-icon[title*="research" i]',
  )
  if (!button) throw new Error('deep-research toolbar toggle not found')
  return button
}

function submitComposerForm() {
  const form = screen.getByRole('textbox').closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
}

beforeEach(() => {
  mockListModels.mockReset().mockResolvedValue([])
})

afterEach(() => {
  submitted.length = 0
  vi.clearAllMocks()
})

describe('composer retrieval-scope behavior', () => {
  it('leaves Search and Deep-research enabled under auto', () => {
    renderComposer({ retrievalScope: 'auto' })
    expect((screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement).disabled).toBe(false)
    expect(deepResearchToggle().disabled).toBe(false)
  })

  it('disables the Deep-research button under knowledge_only, leaving Search untouched', () => {
    renderComposer({ retrievalScope: 'knowledge_only' })
    expect(deepResearchToggle().disabled).toBe(true)
    expect((screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('forces Search on and disabled under web_only, leaving Deep-research untouched', () => {
    renderComposer({ retrievalScope: 'web_only' })
    const searchButton = screen.getByRole('button', { name: /søk på nett/i }) as HTMLButtonElement
    expect(searchButton.disabled).toBe(true)
    expect(searchButton.getAttribute('aria-pressed')).toBe('true')
    expect(deepResearchToggle().disabled).toBe(false)
  })

  it('reflects the picked scope on the pre-send pill', () => {
    renderComposer({ retrievalScope: 'web_only' })
    expect(screen.getByRole('button', { name: /kontekst for dette svaret:/i }).getAttribute('aria-label')).toMatch(/kun nett/i)
  })

  it('omits retrievalScope from the payload when left at auto', () => {
    renderComposer({ retrievalScope: 'auto' })
    submitComposerForm()
    expect(submitted[0]?.retrievalScope).toBeUndefined()
  })

  it('carries the picked scope on the submit payload', () => {
    renderComposer({ retrievalScope: 'knowledge_only' })
    submitComposerForm()
    expect(submitted[0]?.retrievalScope).toBe('knowledge_only')
  })

  it('omits retrievalScope from the payload under conversation-only, even with a restrictive scope also set', () => {
    renderComposer({ retrievalScope: 'web_only', sourceScope: 'conversation' })
    submitComposerForm()
    expect(submitted[0]?.retrievalScope).toBeUndefined()
    expect(submitted[0]?.sourceScope).toBe('conversation')
  })
})
