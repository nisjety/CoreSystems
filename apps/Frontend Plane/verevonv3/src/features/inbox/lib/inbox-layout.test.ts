// @vitest-environment jsdom

import { createRoot } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { ASIDE_DEFAULT, createInboxLayout, LIST_DEFAULT } from './inbox-layout'

afterEach(() => {
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
})
