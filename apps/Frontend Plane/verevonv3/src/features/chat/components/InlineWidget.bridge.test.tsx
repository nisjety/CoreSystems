// @vitest-environment jsdom

/**
 * The widget bridge in the frame (EP-4 S3). A widget may size itself, stage a
 * follow-up the person clicks and ask to open a safe link the person confirms;
 * nothing it posts is sent or opened on its own, and with the switch off there
 * is no shim and no listener at all.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { MAX_WIDGET_HEIGHT } from '../lib/widget-bridge'
import { InlineWidget, InlineWidgetAnswer } from './InlineWidget'

const SOURCE = '<title>Lånekalkulator</title><button onclick="verevon.suggest(\'Vis for 25 år\')">25 år</button>'

/** Stands in for the frame's window; the host compares `event.source` with it by identity. */
const FRAME_WINDOW = { name: 'widget-frame' }

function code() {
  return <pre class="test-code">{SOURCE}</pre>
}

function mount(onSelectFollowUp?: (text: string) => void) {
  const view = render(() => (
    <InlineWidgetAnswer onSelectFollowUp={onSelectFollowUp}>
      <InlineWidget source={SOURCE} fallback={code()} />
    </InlineWidgetAnswer>
  ))
  const frame = view.container.querySelector('iframe') as HTMLIFrameElement
  Object.defineProperty(frame, 'contentWindow', { configurable: true, value: FRAME_WINDOW })
  return { ...view, frame }
}

function post(type: string, payload: unknown, source: unknown = FRAME_WINDOW, origin = 'null') {
  const event = new MessageEvent('message', { data: { v: 1, type, payload }, origin })
  Object.defineProperty(event, 'source', { value: source })
  window.dispatchEvent(event)
  flush()
}

afterEach(() => {
  vi.unstubAllEnvs()
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

describe('InlineWidget bridge', () => {
  it('followup is staged not sent', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const onSelectFollowUp = vi.fn()
    const { container, unmount } = mount(onSelectFollowUp)

    for (let i = 0; i < 20; i += 1) post('followup', { text: 'Vis for 25 år' })

    const chips = container.querySelectorAll('.verevon-chat-followup-chip')
    expect(chips).toHaveLength(1)
    expect(chips[0]?.textContent).toBe('Vis for 25 år')
    // The chip says where it comes from: the widget, not Verevon.
    expect(container.textContent).toContain('Forslag fra widgeten:')
    expect(onSelectFollowUp).not.toHaveBeenCalled()
    unmount()
  })

  it('clicking the chip calls onSelectFollowUp once', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const onSelectFollowUp = vi.fn()
    const { container, unmount } = mount(onSelectFollowUp)

    post('followup', { text: 'Vis for 25 år' })
    fireEvent.click(container.querySelector('.verevon-chat-followup-chip')!)
    flush()

    expect(onSelectFollowUp).toHaveBeenCalledTimes(1)
    expect(onSelectFollowUp).toHaveBeenCalledWith('Vis for 25 år')
    expect(container.querySelector('.verevon-chat-followup-chip')).toBeNull()

    // The slot is free again: the widget may stage its next suggestion.
    post('followup', { text: 'Vis for 30 år' })
    expect(container.querySelector('.verevon-chat-followup-chip')?.textContent).toBe('Vis for 30 år')
    unmount()
  })

  it('stages nothing without a follow-up handler', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const { container, unmount } = mount(undefined)
    post('followup', { text: 'Vis for 25 år' })
    expect(container.querySelector('.verevon-chat-followup-chip')).toBeNull()
    unmount()
  })

  it('unsafe link is refused', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const { container, unmount } = mount(vi.fn())

    post('open_link', { url: 'javascript:alert(1)' })
    expect(container.querySelector('a')).toBeNull()
    expect(container.querySelector('[role="note"]')?.textContent).toBe('Lenke blokkert: ukjent adresse')
    unmount()
  })

  it('a safe link waits for the person to open it', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const open = vi.spyOn(window, 'open')
    const { container, getByText, unmount } = mount(vi.fn())

    post('open_link', { url: 'https://example.com/rente' })
    // A second request does not replace the one the person is reading.
    post('open_link', { url: 'https://evil.example' })
    expect(open).not.toHaveBeenCalled()
    const link = container.querySelector('a')
    expect(link?.getAttribute('href')).toBe('https://example.com/rente')
    expect(link?.getAttribute('target')).toBe('_blank')
    expect(link?.getAttribute('rel')).toBe('noopener noreferrer')
    expect(container.textContent).toContain('https://example.com/rente')
    expect(container.textContent).not.toContain('evil.example')

    fireEvent.click(getByText('Avbryt'))
    flush()
    expect(container.querySelector('a')).toBeNull()
    unmount()
  })

  it('size sets the frame height, clamped', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const { frame, unmount } = mount(vi.fn())

    post('size', { height: 240 })
    expect(frame.style.height).toBe('240px')
    post('size', { height: 50_000 })
    expect(frame.style.height).toBe(`${MAX_WIDGET_HEIGHT}px`)
    unmount()
  })

  it('ignores another window and a non-null origin', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const onSelectFollowUp = vi.fn()
    const { container, frame, unmount } = mount(onSelectFollowUp)

    post('size', { height: 240 }, window)
    post('followup', { text: 'Fra siden' }, { name: 'another-frame' })
    post('followup', { text: 'Fra en annen origin' }, FRAME_WINDOW, 'http://localhost:5173')
    expect(frame.style.height).toBe('')
    expect(container.querySelector('.verevon-chat-followup-chip')).toBeNull()
    unmount()
  })

  it('listener removed on unmount', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const added = vi.spyOn(window, 'addEventListener')
    const removed = vi.spyOn(window, 'removeEventListener')
    const { unmount } = mount(vi.fn())

    const listeners = added.mock.calls.filter(([type]) => type === 'message').map(([, listener]) => listener)
    expect(listeners).toHaveLength(1)
    unmount()
    expect(removed.mock.calls.filter(([type, listener]) => type === 'message' && listener === listeners[0])).toHaveLength(1)
  })

  it('switch off: no shim and no listener', () => {
    const added = vi.spyOn(window, 'addEventListener')
    const onSelectFollowUp = vi.fn()
    const { container, frame, unmount } = mount(onSelectFollowUp)

    expect(frame.getAttribute('srcdoc') ?? '').not.toContain('window.verevon')
    expect(added.mock.calls.filter(([type]) => type === 'message')).toHaveLength(0)
    post('followup', { text: 'Vis for 25 år' })
    post('size', { height: 240 })
    expect(container.querySelector('.verevon-chat-followup-chip')).toBeNull()
    expect(frame.style.height).toBe('')
    unmount()
  })

  it('switch on: the shim is in the frame before the widget', () => {
    vi.stubEnv('VITE_CHAT_WIDGET_BRIDGE', 'true')
    const { frame, unmount } = mount(vi.fn())
    const srcdoc = frame.getAttribute('srcdoc') ?? ''
    expect(srcdoc).toContain('window.verevon')
    expect(srcdoc.indexOf('window.verevon')).toBeLessThan(srcdoc.indexOf('<body>'))
    unmount()
  })
})
