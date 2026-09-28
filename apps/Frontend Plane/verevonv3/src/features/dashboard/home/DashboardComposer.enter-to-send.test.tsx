// @vitest-environment jsdom

// F-03 (CHAT_PARITY_AUDIT_2026-09-15.md §3.1, §3.6): Enter did not submit the
// composer in 3/3 automated attempts. Root cause traced to `detectTrigger`'s
// ambient "date" suggestion (below the slash-command menu in
// DashboardComposer.tsx): unlike the slash menu, which only opens after an
// explicit "/", it opens on ANY typed word of 3+ letters that happens to
// prefix a day name — "man", "fri", "tor"… all ordinary, common Norwegian
// words on their own. Two things conspired to swallow Enter whenever that
// popup happened to be open:
//   1. `handleAutocompleteKeyDown`'s "Enter" case unconditionally consumed
//      the keystroke to confirm the suggestion (inserting a date) instead of
//      letting it reach the submit branch.
//   2. `submitComposer`'s own guard separately bailed out whenever
//      `autocomplete()` was truthy at all, so even after (1) was fixed the
//      submit was still silently dropped.
// Both are covered here; the fix must leave the deliberately-opened "/"
// actions menu still consuming Enter (that one really is a command palette).

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

vi.mock('@/shared/api/chat-actions-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-actions-client')>()
  return { ...actual, loadSpecializedActions: vi.fn().mockResolvedValue(actual.BUILTIN_ACTIONS) }
})

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      message={message()}
      onMessageChange={setMessage}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
    />
  )
}

function renderComposer() {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: ComposerPage }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function textbox() {
  return screen.getByRole('textbox') as HTMLTextAreaElement
}

function typeText(text: string) {
  fireEvent.input(textbox(), { target: { value: text } })
  flush()
}

function pressEnter() {
  fireEvent.keyDown(textbox(), { key: 'Enter', code: 'Enter' })
  flush()
}

afterEach(() => {
  submitted.length = 0
})

describe('Enter sends the composer (F-03)', () => {
  it('submits a plain message with no autocomplete open', () => {
    renderComposer()
    typeText('Hva er dagens vær i Oslo')
    pressEnter()
    expect(submitted).toHaveLength(1)
    expect(submitted[0]?.text).toBe('Hva er dagens vær i Oslo')
  })

  it('submits a message ending in a word that prefixes a day name, instead of confirming the ambient date suggestion', async () => {
    renderComposer()
    typeText('Send meg en oppsummering man')
    // Confirm the reproduction precondition: the ambient date popup really is
    // open at this point (typing "man" prefix-matches "mandag"/"Monday").
    expect(document.querySelector('.dashboard-composer-autocomplete')).toBeTruthy()

    pressEnter()

    await waitFor(() => expect(submitted).toHaveLength(1))
    expect(submitted[0]?.text).toBe('Send meg en oppsummering man')
    // Before the fix, Enter rewrote the draft to "…man. <next Monday>."
    // instead of sending it.
    await waitFor(() => expect(textbox().value).toBe(''))
  })

  it('submits a message ending in "fri" — a common standalone Norwegian word that also prefixes "fredag"', () => {
    renderComposer()
    typeText('Jeg har fri')
    expect(document.querySelector('.dashboard-composer-autocomplete')).toBeTruthy()

    pressEnter()

    expect(submitted).toHaveLength(1)
    expect(submitted[0]?.text).toBe('Jeg har fri')
  })

  it('still lets Enter confirm a deliberately-opened "/" actions menu instead of submitting', async () => {
    renderComposer()
    typeText('/web')
    expect(await screen.findByText('Web search')).toBeTruthy()

    pressEnter()

    // The menu consumed Enter: nothing was sent, and the "/web" trigger text
    // was cleared from the draft as part of selecting the action.
    expect(submitted).toHaveLength(0)
    expect(textbox().value).toBe('')
    // The selected action ran (builtin "web_search" flips Søk on).
    expect(screen.getByRole('button', { name: /søk på nett/i }).getAttribute('aria-pressed')).toBe('true')
  })
})
