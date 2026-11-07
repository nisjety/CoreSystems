// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import type { GeneratedFilePreview } from './chat-types'
import { type DeckSlide, type DeckSlideState, parseDeckManifest } from './deck-manifest'
import { DeckPresentMode, pagesForSlides, presentGate } from './DeckPresentMode'

const here = dirname(fileURLToPath(import.meta.url))
const fixture = readFileSync(join(here, '..', '..', '..', 'shared', 'api', 'contracts', 'deck_manifest_v1.json'), 'utf8')

// The fixture's four slides in order: ok, warn (with notes), draft and error.
const slides = parseDeckManifest(fixture)?.slides ?? []
const PDF = 'data:application/pdf;base64,JVBERi0='

function preview(pages: number): GeneratedFilePreview {
  return { mime: 'application/pdf', url: PDF, pages, fontsSubstituted: false, thumbs: [] }
}

function slide(state: DeckSlideState): DeckSlide {
  return {
    id: 's1',
    layout: 'title_only',
    title: 'Lysbilde',
    bullets: [],
    left: [],
    right: [],
    image: null,
    notes: null,
    state,
    findings: [],
  }
}

function renderPresentation(
  options: { slides?: readonly DeckSlide[]; preview?: GeneratedFilePreview; startIndex?: number; onClose?: () => void } = {},
) {
  return render(() => (
    <I18nProvider>
      <DeckPresentMode
        slides={options.slides ?? slides}
        title="Kvartalstall Q3"
        preview={options.preview}
        startIndex={options.startIndex}
        onClose={options.onClose ?? (() => undefined)}
      />
    </I18nProvider>
  ))
}

/** The slide counter's text: the slide number over the count. */
function counter(): string | null | undefined {
  return document.querySelector('.verevon-deck-present__counter')?.textContent
}

/** The rendered frame, which is there only when the presentation draws the slides from the PDF. */
function frame(): HTMLIFrameElement | null {
  return document.querySelector<HTMLIFrameElement>('.verevon-deck-present__page')
}

/** A key press on the page, which the presentation sees before anything else. */
function press(key: string, init: KeyboardEventInit = {}): void {
  fireEvent.keyDown(document, { key, ...init })
  flush()
}

describe('DeckPresentMode', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('opens on the first slide as a modal dialog named after the deck', () => {
    renderPresentation()
    const dialog = screen.getByRole('dialog', { name: 'Presentation: Kvartalstall Q3' })
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    expect(counter()).toBe('1 / 4')
    expect(screen.getByRole('heading', { name: 'Kvartalstall Q3' })).toBeTruthy()
    expect(screen.getByText('Draft from manifest')).toBeTruthy()
  })

  it('moves one slide with the arrow and page keys, and stops at the first and the last', () => {
    renderPresentation()
    press('ArrowRight')
    expect(counter()).toBe('2 / 4')
    press('PageDown')
    expect(counter()).toBe('3 / 4')
    press('ArrowDown')
    expect(counter()).toBe('4 / 4')
    press('ArrowRight')
    expect(counter()).toBe('4 / 4')
    press('ArrowLeft')
    expect(counter()).toBe('3 / 4')
    press('PageUp')
    expect(counter()).toBe('2 / 4')
    press('ArrowUp')
    expect(counter()).toBe('1 / 4')
    press('ArrowLeft')
    expect(counter()).toBe('1 / 4')
  })

  it('jumps to the first and the last slide with Home and End', () => {
    renderPresentation({ startIndex: 1 })
    expect(counter()).toBe('2 / 4')
    press('End')
    expect(counter()).toBe('4 / 4')
    press('Home')
    expect(counter()).toBe('1 / 4')
  })

  it('shows the speaker notes of a slide only while they are open, and N toggles them', () => {
    renderPresentation({ startIndex: 1 })
    expect(screen.queryByText('Kilde: regnskapsrapport Q3, side 4.')).toBeNull()
    press('n')
    expect(screen.getByText('Kilde: regnskapsrapport Q3, side 4.')).toBeTruthy()
    press('N')
    expect(screen.queryByText('Kilde: regnskapsrapport Q3, side 4.')).toBeNull()
  })

  it('offers no notes on a slide without them, so N does nothing there', () => {
    renderPresentation()
    press('n')
    expect(screen.queryByRole('region', { name: 'Notes' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Show notes (N)' })).toBeNull()
  })

  it('closes on Escape, and keeps that key from reaching the page behind', () => {
    const onClose = vi.fn()
    renderPresentation({ onClose })
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.dispatchEvent(escape)
    expect(onClose).toHaveBeenCalledTimes(1)
    expect(escape.defaultPrevented).toBe(true)
  })

  it('draws the rendered page of each slide when the render has one page per slide', () => {
    renderPresentation({ preview: preview(4) })
    expect(frame()?.getAttribute('src')).toBe(`${PDF}#page=1`)
    press('ArrowRight')
    expect(frame()?.getAttribute('src')).toBe(`${PDF}#page=2`)
    expect(document.querySelector('.verevon-deck-present__slide')).toBeNull()
  })

  it('draws the slides from the manifest when the page count is not the slide count', () => {
    renderPresentation({ preview: preview(3) })
    expect(frame()).toBeNull()
    expect(screen.getByRole('heading', { name: 'Kvartalstall Q3' })).toBeTruthy()
    expect(screen.getByText('Draft from manifest')).toBeTruthy()
  })

  it('keeps Tab inside the dialog, from the last control to the first and back', () => {
    renderPresentation()
    const close = screen.getByRole('button', { name: 'Close presentation' })
    const next = screen.getByRole('button', { name: 'Next slide' })
    next.focus()
    fireEvent.keyDown(document, { key: 'Tab' })
    expect(document.activeElement).toBe(close)
    fireEvent.keyDown(document, { key: 'Tab', shiftKey: true })
    expect(document.activeElement).toBe(next)
  })

  it('says a deck with no slides has none to show, and does not move', () => {
    renderPresentation({ slides: [] })
    expect(screen.getByText('No slides yet')).toBeTruthy()
    expect(counter()).toBe('')
    press('ArrowRight')
    press('n')
    expect(counter()).toBe('')
  })
})

describe('presentGate', () => {
  it('holds presenting back for an error or a slide never built, and never for a warning', () => {
    expect(presentGate([slide('ok'), slide('warn'), slide('warn')])).toEqual({
      slides: 3,
      errors: 0,
      drafts: 0,
      warnings: 2,
      allowed: true,
    })
    expect(presentGate([slide('ok'), slide('error')])).toMatchObject({ errors: 1, allowed: false })
    expect(presentGate([slide('ok'), slide('draft')])).toMatchObject({ drafts: 1, allowed: false })
  })

  it('does not allow a deck without slides', () => {
    expect(presentGate([])).toEqual({ slides: 0, errors: 0, drafts: 0, warnings: 0, allowed: false })
  })
})

describe('pagesForSlides', () => {
  it('draws from the render only when its pages are the slides, one for one', () => {
    const rendered = preview(4)
    expect(pagesForSlides(rendered, 4)).toBe(rendered)
    expect(pagesForSlides(preview(3), 4)).toBeUndefined()
    expect(pagesForSlides(undefined, 4)).toBeUndefined()
    expect(pagesForSlides(preview(0), 0)).toBeUndefined()
  })

  it('frames only a PDF data URI, never a link of another kind', () => {
    expect(pagesForSlides({ ...preview(4), url: 'https://example.test/deck.pdf' }, 4)).toBeUndefined()
    expect(pagesForSlides({ ...preview(4), url: 'javascript:alert(1)' }, 4)).toBeUndefined()
  })
})
