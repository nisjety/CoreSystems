// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

// Coverage for the mid-run submit opt-in. The regression this pins: the chat
// page passes `submitting={isStreaming()}`, and the composer's submit guard
// returned SILENTLY for the entire stream — so the whole queued-input arc
// (deliverMidRun → /v1/invoke/{id}/queue → QueuedInputStrip) had no reachable
// client, and a message typed during a live answer was still dropped, which is
// the exact bug that machinery was built to fix. With `allowMidRunSubmit`, a
// submit during `submitting` must reach `onSubmit`; without it, the dashboard's
// own double-send protection must keep working.

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
  }
})

import { DashboardComposer } from './DashboardComposer'

const submitted: Array<Record<string, unknown>> = []

function ComposerPage(props: { allowMidRunSubmit?: boolean }) {
  const [message, setMessage] = createSignal('')
  return (
    <DashboardComposer
      allowMidRunSubmit={props.allowMidRunSubmit}
      message={message()}
      onMessageChange={setMessage}
      onStop={() => {}}
      onSubmit={(payload) => {
        submitted.push(payload as unknown as Record<string, unknown>)
      }}
      // The window under test: the chat page holds this true for the whole
      // model stream, not just its own send round-trip.
      submitting
    />
  )
}

function renderComposer(allowMidRunSubmit?: boolean) {
  const TestRouter = createRouter({
    routes: [{ path: '/*all', component: () => <ComposerPage allowMidRunSubmit={allowMidRunSubmit} /> }],
    history: memoryHistory(),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function typeAndSubmit(text: string) {
  const textbox = screen.getByRole('textbox')
  fireEvent.input(textbox, { target: { value: text } })
  const form = textbox.closest('form')
  if (!form) throw new Error('composer form not found')
  fireEvent.submit(form)
}

afterEach(() => {
  submitted.length = 0
})

describe('mid-run submit', () => {
  it('delivers a submit during a live stream when the page opts in', () => {
    renderComposer(true)
    typeAndSubmit('og husk å ta med fraktkostnadene')
    expect(submitted).toHaveLength(1)
    expect(submitted[0]?.text).toBe('og husk å ta med fraktkostnadene')
  })

  it('still swallows the submit without the opt-in (double-send protection)', () => {
    renderComposer(false)
    typeAndSubmit('denne skal ikke sendes')
    expect(submitted).toHaveLength(0)
  })

  it('keeps the stop control while mid-run submit is allowed', () => {
    // Opting in must not trade away the ability to stop the stream: the two
    // affordances coexist — Enter queues a mid-run message, the button stops.
    renderComposer(true)
    expect(screen.getByRole('button', { name: /stopp/i })).toBeTruthy()
  })
})
