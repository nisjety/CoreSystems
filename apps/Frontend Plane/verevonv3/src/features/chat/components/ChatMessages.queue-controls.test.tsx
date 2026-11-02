// @vitest-environment jsdom

/**
 * Edit and Remove on a message sent mid-run, in the queue strip.
 *
 * The strip stays read-only unless `VITE_CHAT_QUEUE_RETRACT` is the literal `true` AND its
 * parent hands it a handler. Only a message the gateway accepted and still holds can be taken
 * back; every other entry renders exactly as the tray always has.
 */

import { cleanup, fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { QueuedInputStrip } from './ChatMessages'
import type { QueuedInput } from './chat-types'

const queued = (overrides: Partial<QueuedInput> = {}): QueuedInput => ({
  id: 'queued-1',
  content: 'Sjekk også faktura 42',
  state: 'pending',
  accepted: true,
  clientMessageId: 'msg_pending_1',
  ...overrides,
})

const buttonByLabel = (container: HTMLElement, label: string) =>
  container.querySelector<HTMLButtonElement>(`.verevon-chat-message-actions button[aria-label="${label}"]`)

const buttonByText = (container: HTMLElement, text: string) =>
  Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === text) ?? null

/** Solid applies a handler's updates on flush, so the DOM is read only after one. */
const act = (fire: () => void) => {
  fire()
  flush()
}

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

describe('queue strip take-back controls', () => {
  it('renders no controls while the switch is off, even with handlers', () => {
    const view = render(() => <QueuedInputStrip entries={[queued()]} onEdit={vi.fn()} onRetract={vi.fn()} />)

    expect(view.container.querySelector('.verevon-chat-message-actions')).toBeNull()
    expect(view.container.querySelector('button')).toBeNull()
  })

  it('keeps the tray markup byte-identical while the switch is off', () => {
    const view = render(() => <QueuedInputStrip entries={[queued()]} onEdit={vi.fn()} onRetract={vi.fn()} />)

    expect(view.container.innerHTML).toBe(
      '<ul class="verevon-chat-queued" aria-label="Meldinger sendt underveis">'
      + '<li class="verevon-chat-queued-item" data-state="pending">'
      + '<p class="verevon-chat-queued-text">Sjekk også faktura 42</p>'
      + '<p class="verevon-chat-queued-state"><span>venter</span></p>'
      + '</li></ul>',
    )
  })

  it('reads the switch only as the literal true', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', '1')
    const view = render(() => <QueuedInputStrip entries={[queued()]} onRetract={vi.fn()} />)

    expect(view.container.querySelector('button')).toBeNull()
  })

  it('offers Edit and Remove on an accepted, pending message', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const view = render(() => <QueuedInputStrip entries={[queued()]} onEdit={vi.fn()} onRetract={vi.fn()} />)

    expect(buttonByLabel(view.container, 'Rediger')).not.toBeNull()
    expect(buttonByLabel(view.container, 'Fjern')).not.toBeNull()
  })

  it('shows only the control whose handler was passed', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const view = render(() => <QueuedInputStrip entries={[queued()]} onRetract={vi.fn()} />)

    expect(buttonByLabel(view.container, 'Rediger')).toBeNull()
    expect(buttonByLabel(view.container, 'Fjern')).not.toBeNull()
  })

  it('renders no controls without a handler', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const view = render(() => <QueuedInputStrip entries={[queued()]} />)

    expect(view.container.querySelector('button')).toBeNull()
  })

  it('hands Remove the entry it belongs to', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const onRetract = vi.fn()
    const entry = queued()
    const view = render(() => <QueuedInputStrip entries={[entry]} onRetract={onRetract} />)

    act(() => fireEvent.click(buttonByLabel(view.container, 'Fjern')!))

    expect(onRetract).toHaveBeenCalledWith(entry)
  })

  it('sends the trimmed edited text and leaves the decision to the parent', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const onEdit = vi.fn()
    const entry = queued()
    const view = render(() => <QueuedInputStrip entries={[entry]} onEdit={onEdit} />)

    act(() => fireEvent.click(buttonByLabel(view.container, 'Rediger')!))
    const box = view.container.querySelector('textarea')!
    expect(box.value).toBe(entry.content)
    act(() => fireEvent.input(box, { target: { value: '  Sjekk faktura 43  ' } }))
    act(() => fireEvent.click(buttonByText(view.container, 'Send på nytt')!))

    expect(onEdit).toHaveBeenCalledTimes(1)
    expect(onEdit).toHaveBeenCalledWith(entry, 'Sjekk faktura 43')
    expect(view.container.querySelector('textarea')).toBeNull()
  })

  it('does not send a blank edit', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const onEdit = vi.fn()
    const view = render(() => <QueuedInputStrip entries={[queued()]} onEdit={onEdit} />)

    act(() => fireEvent.click(buttonByLabel(view.container, 'Rediger')!))
    act(() => fireEvent.input(view.container.querySelector('textarea')!, { target: { value: '   ' } }))

    expect(buttonByText(view.container, 'Send på nytt')!.disabled).toBe(true)
    expect(onEdit).not.toHaveBeenCalled()
  })

  it('Escape cancels an edit without sending it', () => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const onEdit = vi.fn()
    const view = render(() => <QueuedInputStrip entries={[queued()]} onEdit={onEdit} />)

    act(() => fireEvent.click(buttonByLabel(view.container, 'Rediger')!))
    act(() => fireEvent.keyDown(view.container.querySelector('textarea')!, { key: 'Escape' }))

    expect(view.container.querySelector('textarea')).toBeNull()
    expect(buttonByLabel(view.container, 'Rediger')).not.toBeNull()
    expect(onEdit).not.toHaveBeenCalled()
  })

  it.each<[string, Partial<QueuedInput>]>([
    ['delivered', { state: 'delivered' }],
    ['resent', { state: 'resent' }],
    ['refused', { state: 'refused' }],
    ['not accepted by the gateway', { accepted: false }],
    ['without a client message id', { clientMessageId: undefined }],
  ])('renders no controls for a message that is %s', (_case, overrides) => {
    vi.stubEnv('VITE_CHAT_QUEUE_RETRACT', 'true')
    const view = render(() => <QueuedInputStrip entries={[queued(overrides)]} onEdit={vi.fn()} onRetract={vi.fn()} />)

    expect(view.container.querySelector('button')).toBeNull()
  })
})
