// @vitest-environment jsdom

/**
 * The inline widget frame (EP-4 S1). The sandbox attribute is the security
 * boundary: `allow-scripts` alone, never `allow-same-origin` (together they let
 * the widget lift its own sandbox), never forms, popups or top navigation.
 */

import { fireEvent, render } from '@solidjs/testing-library'
import { flush, For } from 'solid-js'
import { afterEach, describe, expect, it } from 'vitest'
import { MAX_LIVE_WIDGETS, MAX_WIDGET_CHARS } from '../lib/inline-widget'
import { InlineWidget, WIDGET_SANDBOX } from './InlineWidget'

const SOURCE = '<title>Lånekalkulator</title><input type="range"><script>document.title = "x"</script>'

function code(text = SOURCE) {
  return <pre class="test-code">{text}</pre>
}

afterEach(() => {
  document.body.innerHTML = ''
})

describe('InlineWidget', () => {
  it('sandbox is allow-scripts only', () => {
    const { container, unmount } = render(() => <InlineWidget source={SOURCE} fallback={code()} />)
    const frame = container.querySelector('iframe')
    expect(WIDGET_SANDBOX).toBe('allow-scripts')
    expect(frame?.getAttribute('sandbox')).toBe('allow-scripts')
    expect(frame?.getAttribute('referrerpolicy')).toBe('no-referrer')
    expect(frame?.getAttribute('loading')).toBe('lazy')
    const srcdoc = frame?.getAttribute('srcdoc') ?? ''
    expect(srcdoc.startsWith('<!doctype html><html><head><meta http-equiv="Content-Security-Policy"')).toBe(true)
    expect(srcdoc).toContain("connect-src 'none'")
    expect(srcdoc).toContain(SOURCE)
    unmount()
  })

  it('no same origin ever', () => {
    const { container, unmount } = render(() => <InlineWidget source={SOURCE} fallback={code()} />)
    const sandbox = container.querySelector('iframe')?.getAttribute('sandbox') ?? ''
    for (const token of ['allow-same-origin', 'allow-forms', 'allow-popups', 'allow-top-navigation', 'allow-modals']) {
      expect(sandbox).not.toContain(token)
    }
    unmount()
  })

  it('show code toggles', () => {
    const { container, getByText, unmount } = render(() => <InlineWidget source={SOURCE} fallback={code()} />)
    expect(container.querySelector('.test-code')).toBeNull()
    fireEvent.click(getByText('Vis kode'))
    flush()
    expect(container.querySelector('.test-code')?.textContent).toBe(SOURCE)
    fireEvent.click(getByText('Skjul kode'))
    flush()
    expect(container.querySelector('.test-code')).toBeNull()
    unmount()
  })

  it('title falls back', () => {
    const named = render(() => <InlineWidget source={SOURCE} fallback={code()} />)
    expect(named.container.querySelector('iframe')?.getAttribute('title')).toBe('Lånekalkulator')
    named.unmount()
    const unnamed = render(() => <InlineWidget source="<p>hei</p>" fallback={code('<p>hei</p>')} />)
    expect(unnamed.container.querySelector('iframe')?.getAttribute('title')).toBe('Interaktiv widget')
    unnamed.unmount()
  })

  it('over the cap stays code with a note and no frame', () => {
    const long = 'x'.repeat(MAX_WIDGET_CHARS + 1)
    const { container, unmount } = render(() => <InlineWidget source={long} fallback={code(long)} />)
    expect(container.querySelector('iframe')).toBeNull()
    expect(container.querySelector('.test-code')).toBeTruthy()
    expect(container.querySelector('[role="note"]')?.textContent).toContain('vises som kode')
    unmount()
  })

  it('keeps only the newest widgets live and wakes an older one on click', () => {
    const slots = Array.from({ length: MAX_LIVE_WIDGETS + 2 }, (_, index) => index)
    const { container, unmount } = render(() => (
      <For each={slots}>
        {() => <InlineWidget source={SOURCE} fallback={code()} />}
      </For>
    ))
    flush()
    expect(container.querySelectorAll('iframe')).toHaveLength(MAX_LIVE_WIDGETS)
    const sleeping = container.querySelectorAll('.verevon-inline-widget__activate')
    expect(sleeping).toHaveLength(2)
    fireEvent.click(sleeping[0]!)
    flush()
    expect(container.querySelectorAll('iframe')).toHaveLength(MAX_LIVE_WIDGETS + 1)
    unmount()
  })
})
