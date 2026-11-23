import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, Show } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { roomItems } from '../lib/room-artifact-items'
import { SpaceArtifactSheet } from './SpaceArtifactSheet'

// DEPTH-WS07-S11-1: the room artifact sheet is a modal. Tab stays inside it, the
// page behind it is inert, and both are released when it closes, with focus back
// on the opener. jsdom has no tab order of its own, so the Tab presses here are
// keydown events, and the focus read back is the one the trap chose.

vi.mock('@/features/chat/components/ChatMessages', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const items = () =>
  roomItems([
    {
      messageId: 'm1',
      artifacts: [
        { id: 'a1', kind: 'document', title: 'Innkjøpsplan', content: 'Planens tekst', version: 2 },
        { id: 'a2', kind: 'code', title: 'script.py', content: 'print(1)', version: 1 },
      ],
    },
  ])

/** The page behind the sheet: a main landmark with a button, and the announcer's live region beside it. */
function buildPage() {
  const main = document.createElement('main')
  const behind = document.createElement('button')
  behind.type = 'button'
  behind.textContent = 'Bak'
  main.append(behind)
  const announcer = document.createElement('div')
  announcer.setAttribute('data-announcer-region', 'polite-a')
  document.body.append(main, announcer)
  return { main, behind, announcer }
}

/** The sheet, opened by the test and closed by its own onClose (the parent removes it, as the room does). */
function renderSheet(onClose: () => void = () => {}) {
  const [open, setOpen] = createSignal(true)
  render(() => (
    <Show when={open()}>
      <SpaceArtifactSheet
        items={items}
        selectedId="a1"
        threadTitle="Innkjøp"
        onClose={() => {
          onClose()
          setOpen(false)
        }}
      />
    </Show>
  ))
}

/** What a Tab press can land on inside the sheet: the same stops the browser would give it. */
function tabStopsOf(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>('button, a[href], input, select, textarea, [tabindex]'))
    .filter((element) => element.tabIndex >= 0 && !(element as HTMLButtonElement).disabled)
    .filter((element) => !element.hasAttribute('data-focus-sentinel'))
}

afterEach(() => {
  cleanup()
  document.body.replaceChildren()
})

describe('SpaceArtifactSheet focus and background (DEPTH-WS07-S11-1)', () => {
  it('takes focus on open and makes the page behind it inert, but not the announcer', async () => {
    const page = buildPage()
    page.behind.focus()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    await waitFor(() => expect(page.main.hasAttribute('inert')).toBe(true))
    expect(page.behind.closest('[inert]')).toBe(page.main)
    expect(page.announcer.hasAttribute('inert')).toBe(false)
    expect(dialog.closest('[inert]')).toBeNull()
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true))
  })

  it('keeps a live region speaking inside a wrapper, and makes the rest of the wrapper inert', async () => {
    const wrapper = document.createElement('section')
    const live = document.createElement('div')
    live.setAttribute('data-announcer-region', 'polite-b')
    const beside = document.createElement('button')
    beside.textContent = 'Ved siden av'
    wrapper.append(live, beside)
    document.body.append(wrapper)
    renderSheet()
    await screen.findByRole('dialog')
    await waitFor(() => expect(beside.closest('[inert]')).not.toBeNull())
    expect(wrapper.hasAttribute('inert')).toBe(false)
    expect(live.hasAttribute('inert')).toBe(false)
  })

  it('makes an element added beside the page while the sheet is open inert as well', async () => {
    buildPage()
    renderSheet()
    await screen.findByRole('dialog')
    const late = document.createElement('div')
    late.textContent = 'Ny melding'
    document.body.insertBefore(late, document.body.firstChild)
    await waitFor(() => expect(late.hasAttribute('inert')).toBe(true))
  })

  it('leaves a dialog opened over the sheet live, and does not take its focus back', async () => {
    buildPage()
    renderSheet()
    const sheet = await screen.findByRole('dialog')
    const palette = document.createElement('div')
    palette.setAttribute('role', 'dialog')
    palette.setAttribute('aria-modal', 'true')
    const input = document.createElement('input')
    palette.append(input)
    document.body.insertBefore(palette, document.body.firstChild)
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(palette.hasAttribute('inert')).toBe(false)
    input.focus()
    expect(document.activeElement).toBe(input)
    expect(sheet.contains(document.activeElement)).toBe(false)
  })

  it('leaves an element that was already inert alone, on open and on close', async () => {
    const page = buildPage()
    page.main.setAttribute('inert', '')
    renderSheet()
    await screen.findByRole('dialog')
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(page.main.hasAttribute('inert')).toBe(true)
  })

  it('releases the page and returns focus to the opener when the sheet closes', async () => {
    const page = buildPage()
    page.behind.focus()
    const onClose = vi.fn()
    renderSheet(onClose)
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(page.main.hasAttribute('inert')).toBe(true))
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(page.main.hasAttribute('inert')).toBe(false)
    expect(page.behind.hasAttribute('inert')).toBe(false)
    await waitFor(() => expect(document.activeElement).toBe(page.behind))
  })
})

describe('SpaceArtifactSheet keyboard focus trap (DEPTH-WS07-S11-1)', () => {
  it('wraps Tab from the last stop to the first, and Shift+Tab from the first to the last', async () => {
    buildPage()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    const stops = tabStopsOf(dialog)
    expect(stops.length).toBeGreaterThan(1)
    const first = stops[0]!
    const last = stops[stops.length - 1]!
    expect(first).toBe(screen.getByRole('button', { name: 'Lukk' }))

    last.focus()
    expect(fireEvent.keyDown(last, { key: 'Tab' })).toBe(false)
    expect(document.activeElement).toBe(first)

    expect(fireEvent.keyDown(first, { key: 'Tab', shiftKey: true })).toBe(false)
    expect(document.activeElement).toBe(last)
  })

  it('leaves Tab between two inner stops to the browser', async () => {
    buildPage()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    const stops = tabStopsOf(dialog)
    const inner = stops[1]!
    expect(inner).not.toBe(stops[stops.length - 1])
    inner.focus()
    expect(fireEvent.keyDown(inner, { key: 'Tab' })).toBe(true)
    expect(document.activeElement).toBe(inner)
  })

  it('moves Shift+Tab from the sheet itself to its last stop', async () => {
    buildPage()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    const last = tabStopsOf(dialog).at(-1)!
    dialog.focus()
    expect(fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true })).toBe(false)
    expect(document.activeElement).toBe(last)
  })

  it('brings focus back into the sheet when something outside it is focused', async () => {
    const page = buildPage()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    page.behind.focus()
    expect(dialog.contains(document.activeElement)).toBe(true)
  })

  it('hands focus from the trailing sentinel (a Tab out of the preview frame) back to the first stop', async () => {
    buildPage()
    renderSheet()
    const dialog = await screen.findByRole('dialog')
    const sentinel = dialog.querySelector<HTMLElement>('[data-focus-sentinel]')!
    expect(sentinel).not.toBeNull()
    expect(dialog.lastElementChild).toBe(sentinel)
    // A frame keeps its key presses to itself, so the sheet only sees focus arrive here.
    const frame = document.createElement('iframe')
    dialog.querySelector('.verevon-space-artifact-sheet__body')!.append(frame)
    frame.focus()
    sentinel.focus()
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Lukk' }))
    // The sentinel is never the stop a wrapped Tab lands on: Shift+Tab from the first
    // stop goes to the last real stop, here the frame.
    expect(fireEvent.keyDown(document.activeElement!, { key: 'Tab', shiftKey: true })).toBe(false)
    expect(document.activeElement).toBe(frame)
  })

  it('keeps Escape closing the sheet from inside it', async () => {
    buildPage()
    const onClose = vi.fn()
    renderSheet(onClose)
    const dialog = await screen.findByRole('dialog')
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
