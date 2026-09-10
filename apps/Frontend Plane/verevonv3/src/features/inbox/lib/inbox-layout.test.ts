// @vitest-environment jsdom

import { createRoot } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ASIDE_DEFAULT, createInboxLayout, LIST_DEFAULT } from './inbox-layout'

beforeEach(() => {
  localStorage.clear()
})

afterEach(() => {
  document.body.style.userSelect = ''
  document.body.style.cursor = ''
  vi.unstubAllGlobals()
})

describe('createInboxLayout', () => {
  it('renders with defaults when an embedded runtime exposes malformed storage', () => {
    vi.stubGlobal('localStorage', {})

    createRoot((dispose) => {
      const layout = createInboxLayout()
      expect(layout.listWidth()).toBe(LIST_DEFAULT)
      expect(layout.asideWidth()).toBe(ASIDE_DEFAULT)
      dispose()
    })
  })

  it('starts an aside drag from the rendered responsive width', () => {
    let layout: ReturnType<typeof createInboxLayout> | undefined
    const dispose = createRoot((disposeRoot) => {
      layout = createInboxLayout()
      return disposeRoot
    })

    const grid = document.createElement('div')
    const handle = document.createElement('div')
    const aside = document.createElement('aside')
    aside.className = 'verevon-inbox-aside'
    aside.getBoundingClientRect = () => ({ width: 250 }) as DOMRect
    grid.append(handle, aside)
    const listeners = new Map<string, EventListenerOrEventListenerObject>()
    vi.spyOn(window, 'addEventListener').mockImplementation((type, listener) => {
      listeners.set(type, listener)
    })

    layout!.startAsideResize({
      clientX: 500,
      currentTarget: handle,
      preventDefault: vi.fn(),
    } as unknown as PointerEvent)
    const move = listeners.get('pointermove') as EventListener
    move({ clientX: 450 } as PointerEvent)
    const up = listeners.get('pointerup') as EventListener
    up({} as PointerEvent)

    expect(localStorage.getItem('verevon.inbox.asideWidth.v2')).toBe('300')
    dispose()
  })
})
