// @vitest-environment jsdom

// Long-paste → "pasted text" attachment (CHAT_PARITY_AUDIT_2026-09-15.md §3.6,
// F-... "Long-paste handling (ChatGPT, Claude)"). ChatGPT and Claude both
// convert a sufficiently long paste into an attachment instead of dumping it
// inline into the composer; this pins that Verevon's `onPaste` now does the
// same above `PASTE_TEXT_ATTACHMENT_THRESHOLD`, while leaving an ordinary
// short paste's inline behavior completely unchanged.

import { createRouter, memoryHistory } from '@solidjs/router'
import { fireEvent, render, screen } from '@solidjs/testing-library'
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

import { DashboardComposer, PASTE_TEXT_ATTACHMENT_THRESHOLD } from './DashboardComposer'

function ComposerPage() {
  const [message, setMessage] = createSignal('')
  return <DashboardComposer message={message()} onMessageChange={setMessage} onSubmit={() => {}} />
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

/**
 * Fires a plain-text paste (no files on the clipboard, matching a real
 * copy-from-elsewhere paste). Returns `dispatchEvent`'s result: `true` unless
 * the handler called `preventDefault()`, exactly mirroring what the browser's
 * own native "insert into the field" step would check.
 */
function pasteText(text: string) {
  const notPrevented = fireEvent.paste(textbox(), {
    clipboardData: {
      getData: (type: string) => (type === 'text/plain' ? text : ''),
      files: [] as File[],
    },
  })
  flush()
  return notPrevented
}

afterEach(() => {
  flush()
})

describe('long paste becomes a "pasted text" attachment', () => {
  it('leaves a short paste inline and untouched (unchanged behavior below the threshold)', () => {
    renderComposer()
    const shortText = 'Husk å bestille møterom til fredag.'
    expect(shortText.length).toBeLessThan(PASTE_TEXT_ATTACHMENT_THRESHOLD)

    const notPrevented = pasteText(shortText)

    // The handler did not call preventDefault, so the browser's native paste
    // (inserting the text into the textarea) still runs exactly as before.
    expect(notPrevented).toBe(true)
    expect(document.querySelector('.dashboard-composer-attachments')).toBeNull()
  })

  it('leaves a paste exactly at the threshold inline (boundary is exclusive)', () => {
    renderComposer()
    const boundaryText = 'a'.repeat(PASTE_TEXT_ATTACHMENT_THRESHOLD)

    const notPrevented = pasteText(boundaryText)

    expect(notPrevented).toBe(true)
    expect(document.querySelector('.dashboard-composer-attachments')).toBeNull()
  })

  it('converts a paste one character past the threshold into a text attachment', () => {
    renderComposer()
    const longText = 'a'.repeat(PASTE_TEXT_ATTACHMENT_THRESHOLD + 1)

    const notPrevented = pasteText(longText)

    // preventDefault was called: the textarea must NOT receive the pasted text.
    expect(notPrevented).toBe(false)
    expect(textbox().value).toBe('')

    // Instead, a "pasted text" attachment chip appears, reusing the same
    // attachment pipeline (and preview markup) as an uploaded .txt file.
    const attachment = document.querySelector('.dashboard-composer-attachment__file')
    expect(attachment).toBeTruthy()
    expect(screen.getByText(/Limt inn tekst/)).toBeTruthy()
  })

  it('converts a realistic long paste (a pasted paragraph dump) into an attachment with a readable name', () => {
    renderComposer()
    const longText = 'Lorem ipsum dolor sit amet, consectetur adipiscing elit. '.repeat(100)
    expect(longText.length).toBeGreaterThan(PASTE_TEXT_ATTACHMENT_THRESHOLD)

    const notPrevented = pasteText(longText)

    expect(notPrevented).toBe(false)
    expect(textbox().value).toBe('')
    expect(screen.getByText(/^Limt inn tekst: Lorem ipsum/)).toBeTruthy()
  })

  it('still converts a pasted image to an attachment instead of a text one when both are on the clipboard', () => {
    renderComposer()
    const image = new File(['fake-image-bytes'], 'screenshot.png', { type: 'image/png' })

    const notPrevented = fireEvent.paste(textbox(), {
      clipboardData: {
        getData: () => 'a'.repeat(PASTE_TEXT_ATTACHMENT_THRESHOLD + 1),
        files: [image],
      },
    })
    flush()

    expect(notPrevented).toBe(false)
    // An image attachment renders as an <img alt="…">, not the .txt-style file
    // chip a pasted-text attachment gets (see `AttachmentPreview`) — either way
    // this tells the pasted image apart from a pasted-text attachment.
    expect(screen.getByAltText('screenshot.png')).toBeTruthy()
    expect(screen.queryByText(/Limt inn tekst/)).toBeNull()
  })
})
