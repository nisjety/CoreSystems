// @vitest-environment jsdom

// chat-wave2 "schedule" spec §2.1, §3 — the `/schedule` (and Norwegian
// `/planlegg`) composer command. Unlike "/image"/"/dyp" above, which just set
// a flag on an otherwise-normal chat send, "/schedule" never becomes a chat
// turn: it opens `ScheduleDialog` seeded with the typed goal and clears the
// draft instead of submitting.

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

// A plain `getByRole('textbox')` becomes ambiguous once the dialog is open —
// its "Navn" field is a text `<input>`, which shares the same implicit role
// as the composer's own `<textarea>` — so this targets the composer's
// textarea specifically by tag, which the dialog never has one of.
function textbox() {
  return document.querySelector('textarea') as HTMLTextAreaElement
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

describe('"/" menu offers Schedule', () => {
  it('lists it with a description and matches the Norwegian alias', async () => {
    renderComposer()
    typeText('/')
    expect(await screen.findByText('Schedule')).toBeTruthy()

    typeText('/planlegg')
    expect(await screen.findByText('Schedule')).toBeTruthy()
  })
})

describe('"/schedule" never becomes a chat turn', () => {
  it('opens the dialog seeded from the typed goal, clears the draft, and sends nothing to chat', async () => {
    renderComposer()
    typeText('/schedule daglig oppsummering av salg')

    pressEnter()

    // The dialog opens with the argument text as its seeded goal/title.
    expect(await screen.findByText('Planlegg denne forespørselen')).toBeTruthy()
    await waitFor(() => expect(textbox().value).toBe(''))
    expect(submitted).toHaveLength(0)
  })

  it('applies the Norwegian alias the same way', async () => {
    renderComposer()
    typeText('/planlegg ukentlig rapport')

    pressEnter()

    expect(await screen.findByText('Planlegg denne forespørselen')).toBeTruthy()
    expect(submitted).toHaveLength(0)
  })

  it('does nothing on a bare "/schedule" with no goal typed yet', () => {
    renderComposer()
    typeText('/schedule ')

    pressEnter()

    expect(screen.queryByText('Planlegg denne forespørselen')).toBeNull()
    expect(submitted).toHaveLength(0)
  })
})
