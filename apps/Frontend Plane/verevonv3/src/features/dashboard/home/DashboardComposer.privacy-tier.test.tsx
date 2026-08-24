// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ModelInfo } from '@/shared/api/chat-client'
import type { PrivacyTier } from '@/shared/api/privacy-tier'

// Coverage for the model-picker privacy-tier surface: badges per tier
// (claimed tiers tinted, `global` plain, unknown/unspecified absent), the
// honest smaller-catalog notice when a sovereign model is selected, and the
// tier riding on the submit payload only for a real catalog selection.

const { mockListModels, mockWritePendingChatLaunch } = vi.hoisted(() => ({
  mockListModels: vi.fn(),
  mockWritePendingChatLaunch: vi.fn(),
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

vi.mock('@/features/chat/lib/pending-chat-launch', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/features/chat/lib/pending-chat-launch')>()
  return {
    ...actual,
    writePendingChatLaunch: mockWritePendingChatLaunch,
  }
})

import { DashboardComposer } from './DashboardComposer'

function catalogModel(overrides: Partial<ModelInfo>): ModelInfo {
  return { id: 'm-1', name: 'Test Model', ...overrides }
}

const submittedPayloads: Array<Record<string, unknown>> = []

// The composer renders an `<A>` link (agents shortcut), so it must mount
// inside a routed context like the rest of the app. It also owns the draft
// text like its real parent does — the submit path refuses empty messages.
function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submittedPayloads.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer() {
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path='/*all' component={ComposerPage} />
    </Router>
  ))
}

/** Opens the model picker and resolves once its menu is actually mounted. */
async function openModelMenu(models: ModelInfo[]) {
  mockListModels.mockResolvedValueOnce(models)
  renderComposer()
  const trigger = await screen.findByRole('button', { name: /velg ai-modell/i })
  fireEvent.click(trigger)
  await waitFor(() =>
    expect(document.querySelector('.dashboard-composer-model-menu')).toBeTruthy(),
  )
}

/** Reopens the picker after a selection closed it. */
async function reopenModelMenu() {
  fireEvent.click(screen.getByRole('button', { name: /velg ai-modell/i }))
  await waitFor(() =>
    expect(document.querySelector('.dashboard-composer-model-menu')).toBeTruthy(),
  )
}

function submitComposerForm() {
  // The picker button sits OUTSIDE the composer <form>; reach the form via
  // the textarea instead of closest('form') from the trigger.
  const form = screen.getByRole('textbox').closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
}

afterEach(() => {
  submittedPayloads.length = 0
  vi.unstubAllGlobals()
})

beforeEach(() => {
  mockWritePendingChatLaunch.mockReset().mockResolvedValue(undefined)
})

describe('model picker privacy badges', () => {
  it('shows a claimed-tone badge for eu_resident and zdr_contractual models', async () => {
    await openModelMenu([
      catalogModel({ id: 'm-eu', name: 'EU Model', privacyTier: 'eu_resident' }),
      catalogModel({ id: 'm-zdr', name: 'ZDR Model', privacyTier: 'zdr_contractual' }),
    ])

    const euBadge = screen.getByTitle(/EU\/EØS-jurisdiksjon/i)
    expect(euBadge.textContent).toBe('EU')
    expect(euBadge.getAttribute('data-tone')).toBe('claimed')

    const zdrBadge = screen.getByTitle(/null datalagring/i)
    expect(zdrBadge.textContent).toBe('ZDR')
    expect(zdrBadge.getAttribute('data-tone')).toBe('claimed')
  })

  it('renders a global tier plainly (no claimed tone)', async () => {
    await openModelMenu([
      catalogModel({ id: 'm-glob', name: 'Global Model', privacyTier: 'global' }),
    ])

    const badge = screen.getByTitle(/ingen regionsbegrensning/i)
    expect(badge.textContent).toBe('Global')
    expect(badge.getAttribute('data-tone')).toBe('plain')
  })

  it('never renders a badge for unspecified tiers — no fabricated claims', async () => {
    // The normalizer already degrades garbage to undefined; this guards the
    // JSX gate itself (an unspecified tier must not light a chip).
    await openModelMenu([
      catalogModel({ id: 'm-uns', name: 'Unspecified Model', privacyTier: 'unspecified' as PrivacyTier }),
    ])

    expect(screen.queryByTitle(/jurisdiksjon|datalagring|regionsbegrensning/i)).toBeNull()
    expect(screen.getByText('Unspecified Model')).toBeTruthy()
  })

  it('adds no tier badge to the pinned Verevon intent modes', async () => {
    await openModelMenu([])
    expect(screen.queryByTitle(/jurisdiksjon|datalagring|regionsbegrensning/i)).toBeNull()
    // The intent modes themselves still render above the (empty) catalog.
    expect(screen.getAllByText(/Verevon Balance|Verevon Genius/).length).toBeGreaterThan(0)
  })
})

describe('sovereign selection notice', () => {
  it('shows the honest smaller-catalog notice once a sovereign model is selected', async () => {
    await openModelMenu([
      catalogModel({ id: 'm-sov', name: 'Sovereign Model', privacyTier: 'sovereign' }),
    ])

    // Selecting closes the menu; the picker keeps showing the note on reopen.
    fireEvent.click(screen.getByText('Sovereign Model'))
    await reopenModelMenu()

    expect(screen.getByRole('note').textContent).toMatch(/færre modeller/i)
    // The trigger reflects the selection too.
    expect(
      screen.getByRole('button', { name: /velg ai-modell/i }).textContent,
    ).toContain('Sovereign Model')
  })

  it('shows nothing for non-sovereign selections', async () => {
    await openModelMenu([
      catalogModel({ id: 'm-eu2', name: 'EU Model B', privacyTier: 'eu_resident' }),
    ])

    fireEvent.click(screen.getByText('EU Model B'))
    await reopenModelMenu()

    // No other tier narrows the catalog → no notice, ever.
    expect(screen.queryByRole('note')).toBeNull()
  })
})

describe('tier threading into the submit payload', () => {
  it('carries minPrivacyTier only when a tiered catalog model is selected', async () => {
    await openModelMenu([
      catalogModel({ id: 'm-sov2', name: 'Sovereign Model B', privacyTier: 'sovereign' }),
    ])

    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Hei' } })
    submitComposerForm()
    await waitFor(() => expect(submittedPayloads.length).toBe(1))

    // Default selection is Verevon Balance (an intent mode → NO tier).
    expect(submittedPayloads[0]).not.toHaveProperty('minPrivacyTier')

    await reopenModelMenu()
    fireEvent.click(await screen.findByText('Sovereign Model B'))
    fireEvent.input(screen.getByRole('textbox'), { target: { value: 'Hei igjen' } })
    submitComposerForm()

    await waitFor(() => expect(submittedPayloads.length).toBe(2))
    expect(submittedPayloads[1]?.minPrivacyTier).toBe('sovereign')
  })
})
