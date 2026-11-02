// @vitest-environment jsdom

// A stale build reloads the page once (WS09 S5, src/app/chunk-recovery.ts). The
// composer keeps unsent attachments, and a submission whose handoff has not
// finished, only in memory. While it holds either, the reload is replaced by the
// stale-build notice; once both are gone the reload goes ahead again.

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { dismissStaleBuildNotice, installChunkRecovery, staleBuildNotice } from '@/app/chunk-recovery'

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

import { DashboardComposer, PASTE_TEXT_ATTACHMENT_THRESHOLD } from './DashboardComposer'

let uninstall: () => void = () => undefined
let reload: ReturnType<typeof vi.fn<() => void>>
// Settles the submission a test is holding open.
let finishSubmit: () => void = () => undefined

/**
 * Dispatches the event Vite fires for a failed lazy import. Returns true when a
 * listener prevented the default, which is what an automatic reload does.
 */
const failChunk = () => {
  const event = new Event('vite:preloadError', { cancelable: true })
  window.dispatchEvent(event)
  flush()
  return event.defaultPrevented
}

function renderComposer(onSubmit: () => Promise<void> | void = () => {}) {
  function ComposerPage() {
    const [message, setMessage] = createSignal('')
    return <DashboardComposer message={message()} onMessageChange={setMessage} onSubmit={onSubmit} />
  }
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

/** A long paste becomes a "pasted text" attachment, held in the composer's files. */
function pasteLongText() {
  fireEvent.paste(textbox(), {
    clipboardData: {
      getData: (type: string) => (type === 'text/plain' ? 'a'.repeat(PASTE_TEXT_ATTACHMENT_THRESHOLD + 1) : ''),
      files: [] as File[],
    },
  })
  flush()
}

function removeAttachment() {
  const remove = document.querySelector<HTMLButtonElement>('.dashboard-composer-attachment__remove')
  if (!remove) throw new Error('expected an attachment to remove')
  fireEvent.click(remove)
  flush()
}

/** A submission the test settles by hand, so the handoff stays in flight until then. */
function heldSubmission() {
  return vi.fn(
    () =>
      new Promise<void>((resolve) => {
        finishSubmit = () => resolve()
      }),
  )
}

beforeEach(() => {
  reload = vi.fn()
  uninstall = installChunkRecovery(reload)
})

afterEach(() => {
  uninstall()
  uninstall = () => undefined
  // Unmounting runs the composer's cleanup, which unregisters its reload guard.
  cleanup()
  dismissStaleBuildNotice()
  flush()
})

describe('DashboardComposer and the stale-build reload', () => {
  it('shows the notice instead of reloading while an unsent attachment is held', () => {
    renderComposer()
    pasteLongText()
    expect(document.querySelector('.dashboard-composer-attachment__file')).toBeTruthy()

    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    expect(staleBuildNotice()).toBe(true)
  })

  it('reloads on a stale build when the composer holds nothing unsent', () => {
    renderComposer()

    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
    expect(staleBuildNotice()).toBe(false)
  })

  it('reloads once the held attachment has been removed', () => {
    renderComposer()
    pasteLongText()
    expect(failChunk()).toBe(false)

    removeAttachment()

    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('holds the reload while a submission is being handed off, even without attachments', () => {
    const onSubmit = heldSubmission()
    renderComposer(onSubmit)
    typeText('Oppsummer møtene mine')
    pressEnter()
    expect(onSubmit).toHaveBeenCalledTimes(1)

    expect(failChunk()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
    expect(staleBuildNotice()).toBe(true)
  })

  it('reloads again once the handoff has finished', async () => {
    const onSubmit = heldSubmission()
    renderComposer(onSubmit)
    typeText('Oppsummer møtene mine')
    pressEnter()
    finishSubmit()
    await new Promise((resolve) => setTimeout(resolve, 0))
    flush()

    expect(failChunk()).toBe(true)
    expect(reload).toHaveBeenCalledTimes(1)
  })
})
