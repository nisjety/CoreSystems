import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  clampAuxPaneWidth,
  readStoredAuxPaneWidth,
  SPACE_AUX_PANE_DEFAULT_WIDTH,
  SPACE_AUX_PANE_MAX_WIDTH,
  SPACE_AUX_PANE_MIN_WIDTH,
  SPACE_AUX_PANE_STEP,
  SPACE_AUX_PANE_WIDTH_KEY,
  SpaceAuxPane,
} from './SpaceAuxPane'

// Each key press is its own task in a browser, so Solid has flushed the new width before the next one.
function press(element: HTMLElement, key: string) {
  fireEvent.keyDown(element, { key })
  flush()
}

function renderPane(initialWidth = SPACE_AUX_PANE_DEFAULT_WIDTH) {
  const onClose = vi.fn()
  const widths: number[] = []
  const [width, setWidth] = createSignal(initialWidth)
  const view = render(() => (
    <SpaceAuxPane
      label="Innkjøp"
      width={width()}
      onWidthChange={(next) => {
        widths.push(next)
        setWidth(next)
      }}
      onClose={onClose}
    >
      <p>Body</p>
    </SpaceAuxPane>
  ))
  return { view, onClose, widths, width }
}

beforeEach(() => {
  try {
    window.localStorage.clear()
  } catch {
    // Storage unavailable in this environment; the tests that need it say so.
  }
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  document.body.innerHTML = ''
})

describe('clampAuxPaneWidth and the stored width (WS01-S7a)', () => {
  it('keeps the width inside 320..720 and rounds it', () => {
    expect(SPACE_AUX_PANE_MIN_WIDTH).toBe(320)
    expect(SPACE_AUX_PANE_MAX_WIDTH).toBe(720)
    expect(clampAuxPaneWidth(100)).toBe(320)
    expect(clampAuxPaneWidth(5000)).toBe(720)
    expect(clampAuxPaneWidth(400.4)).toBe(400)
  })

  it('uses the default for a width that is not a finite number', () => {
    expect(clampAuxPaneWidth(Number.NaN)).toBe(SPACE_AUX_PANE_DEFAULT_WIDTH)
    expect(clampAuxPaneWidth(Number.POSITIVE_INFINITY)).toBe(SPACE_AUX_PANE_DEFAULT_WIDTH)
  })

  it('reads a stored width under verevon.space.auxpane.width.v1, clamped', () => {
    expect(SPACE_AUX_PANE_WIDTH_KEY).toBe('verevon.space.auxpane.width.v1')
    window.localStorage.setItem(SPACE_AUX_PANE_WIDTH_KEY, '500')
    expect(readStoredAuxPaneWidth()).toBe(500)
    window.localStorage.setItem(SPACE_AUX_PANE_WIDTH_KEY, '9999')
    expect(readStoredAuxPaneWidth()).toBe(720)
  })

  it('falls back to the default for nothing stored or a stored value that is not a number', () => {
    expect(readStoredAuxPaneWidth()).toBe(SPACE_AUX_PANE_DEFAULT_WIDTH)
    window.localStorage.setItem(SPACE_AUX_PANE_WIDTH_KEY, 'wide')
    expect(readStoredAuxPaneWidth()).toBe(SPACE_AUX_PANE_DEFAULT_WIDTH)
  })

  it('falls back to the default when reading storage throws', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('blocked', 'SecurityError')
    })
    expect(readStoredAuxPaneWidth()).toBe(SPACE_AUX_PANE_DEFAULT_WIDTH)
  })
})

describe('SpaceAuxPane (WS01-S7a)', () => {
  it('is a labelled landmark with a title, a close control and a resize separator', () => {
    renderPane()
    const pane = screen.getByRole('complementary', { name: 'Innkjøp' })
    expect(pane.style.width).toBe(`${SPACE_AUX_PANE_DEFAULT_WIDTH}px`)
    expect(screen.getByRole('heading', { name: 'Innkjøp' })).toBeTruthy()
    expect(screen.getByRole('button', { name: /Lukk sidepanelet/ })).toBeTruthy()
    const separator = screen.getByRole('separator')
    expect(separator.getAttribute('aria-valuemin')).toBe('320')
    expect(separator.getAttribute('aria-valuemax')).toBe('720')
    expect(separator.getAttribute('aria-valuenow')).toBe(String(SPACE_AUX_PANE_DEFAULT_WIDTH))
  })

  describe('closing', () => {
    it('closes on Escape and hands focus back to whatever opened it', async () => {
      const opener = document.createElement('button')
      opener.textContent = 'Open thread'
      document.body.appendChild(opener)
      opener.focus()
      expect(document.activeElement).toBe(opener)

      const { onClose } = renderPane()
      fireEvent.keyDown(screen.getByRole('complementary'), { key: 'Escape' })
      expect(onClose).toHaveBeenCalledTimes(1)
      await Promise.resolve()
      await Promise.resolve()
      expect(document.activeElement).toBe(opener)
    })

    it('closes from the close button and returns focus the same way', async () => {
      const opener = document.createElement('button')
      document.body.appendChild(opener)
      opener.focus()
      const { onClose } = renderPane()
      fireEvent.click(screen.getByRole('button', { name: /Lukk sidepanelet/ }))
      expect(onClose).toHaveBeenCalledTimes(1)
      await Promise.resolve()
      await Promise.resolve()
      expect(document.activeElement).toBe(opener)
    })

    it('does not close on other keys', () => {
      const { onClose } = renderPane()
      fireEvent.keyDown(screen.getByRole('complementary'), { key: 'Enter' })
      expect(onClose).not.toHaveBeenCalled()
    })
  })

  describe('resizing from the keyboard', () => {
    it('ArrowLeft widens and ArrowRight narrows in 32px steps', () => {
      expect(SPACE_AUX_PANE_STEP).toBe(32)
      const { widths } = renderPane(416)
      const separator = screen.getByRole('separator')
      press(separator, 'ArrowLeft')
      press(separator, 'ArrowRight')
      press(separator, 'ArrowRight')
      expect(widths).toEqual([448, 416, 384])
    })

    it('stays within 320..720 and says nothing when it cannot move', () => {
      const atMax = renderPane(720)
      press(screen.getByRole('separator'), 'ArrowLeft')
      expect(atMax.widths).toEqual([])
      cleanup()
      const nearMin = renderPane(330)
      press(screen.getByRole('separator'), 'ArrowRight')
      expect(nearMin.widths).toEqual([320])
      press(screen.getByRole('separator'), 'ArrowRight')
      expect(nearMin.widths).toEqual([320])
    })

    it('Home and End jump to the limits', () => {
      const { widths } = renderPane(416)
      const separator = screen.getByRole('separator')
      press(separator, 'Home')
      press(separator, 'End')
      expect(widths).toEqual([320, 720])
    })

    it('persists the width under verevon.space.auxpane.width.v1', () => {
      renderPane(416)
      press(screen.getByRole('separator'), 'ArrowLeft')
      expect(window.localStorage.getItem(SPACE_AUX_PANE_WIDTH_KEY)).toBe('448')
    })

    it('still resizes for this page view when storage refuses the write', () => {
      vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
        throw new DOMException('quota', 'QuotaExceededError')
      })
      const { widths } = renderPane(416)
      expect(() => press(screen.getByRole('separator'), 'ArrowLeft')).not.toThrow()
      expect(widths).toEqual([448])
    })
  })
})
