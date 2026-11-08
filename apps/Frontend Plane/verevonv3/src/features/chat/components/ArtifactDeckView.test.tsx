// @vitest-environment jsdom

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { ArtifactDeckView } from './ArtifactDeckView'
import type { ArtifactRendererProps } from './artifact-renderers'
import type { ChatArtifact, GeneratedFile, GeneratedFilePreview } from './chat-types'

const here = dirname(fileURLToPath(import.meta.url))
const fixture = readFileSync(join(here, '..', '..', '..', 'shared', 'api', 'contracts', 'deck_manifest_v1.json'), 'utf8')

const PPTX = 'application/vnd.openxmlformats-officedocument.presentationml.presentation'

type Json = Record<string, unknown>

function manifestWith(change: (manifest: Json) => void): string {
  const manifest = JSON.parse(fixture) as Json
  change(manifest)
  return JSON.stringify(manifest)
}

function artifact(content: string = fixture): ChatArtifact {
  return { content, id: 'artifact-deck', kind: 'deck', title: 'Kvartalstall Q3', version: 5 }
}

function generatedFile(overrides: Partial<GeneratedFile> = {}): GeneratedFile {
  return { id: 'call-1-file-1', name: 'kvartal.pptx', mime: PPTX, size: 3, url: `data:${PPTX};base64,QUFB`, ...overrides }
}

function preview(): GeneratedFilePreview {
  return {
    mime: 'application/pdf',
    url: 'data:application/pdf;base64,JVBERi0=',
    pages: 4,
    fontsSubstituted: false,
    thumbs: ['data:image/png;base64,iVBORw0KGgo='],
  }
}

function previewWith(pages: number, thumbs: string[]): GeneratedFilePreview {
  return { ...preview(), pages, thumbs }
}

function renderDeck(
  options: { content?: string; file?: GeneratedFile; fallback?: ArtifactRendererProps['fallback'] } = {},
) {
  return render(() => (
    <I18nProvider>
      <ArtifactDeckView artifact={artifact(options.content)} file={options.file} fallback={options.fallback} />
    </I18nProvider>
  ))
}

/** The strip's cards, in slide order. */
function cards(container: HTMLElement): HTMLButtonElement[] {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('.verevon-deck__card'))
}

function titles(container: HTMLElement): Array<string | null | undefined> {
  return cards(container).map((card) => card.querySelector('.verevon-deck__title')?.textContent)
}

/** The picture each card draws, or null where it draws none. */
function pictures(container: HTMLElement): Array<string | null> {
  return cards(container).map((card) => card.querySelector('img.verevon-deck__thumb')?.getAttribute('src') ?? null)
}

describe('ArtifactDeckView', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('draws one card per slide, in order, each with its state word', () => {
    const { container } = renderDeck()
    expect(titles(container)).toEqual([
      'Kvartalstall Q3',
      'Salget økte i alle regioner',
      'Mål og resultat',
      'Omsetning per måned',
    ])
    expect(cards(container).map((card) => card.querySelector('.verevon-deck__chip')?.textContent)).toEqual([
      'OK',
      'Warning',
      'Not built',
      'Error',
    ])
  })

  it('opens on the first slide and moves the findings and the notes with the pick', () => {
    const { container } = renderDeck()
    expect(cards(container)[0]?.getAttribute('aria-current')).toBe('true')
    expect(cards(container)[1]?.getAttribute('aria-current')).toBeNull()
    expect(screen.getByText('No findings on this slide')).toBeTruthy()

    fireEvent.click(cards(container)[1] as HTMLButtonElement)
    flush()
    expect(cards(container)[0]?.getAttribute('aria-current')).toBeNull()
    expect(cards(container)[1]?.getAttribute('aria-current')).toBe('true')
    expect(screen.getByText('text may not fit its box; shorten it or split the slide')).toBeTruthy()
  })

  it('shows the notes of a slide only when asked, and the toggle says which state it is in', () => {
    const { container } = renderDeck()
    fireEvent.click(cards(container)[1] as HTMLButtonElement)
    flush()
    expect(screen.queryByText('Kilde: regnskapsrapport Q3, side 4.')).toBeNull()

    const toggle = screen.getByRole('button', { name: 'Show notes' })
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(toggle)
    flush()
    expect(screen.getByText('Kilde: regnskapsrapport Q3, side 4.')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Hide notes' }).getAttribute('aria-expanded')).toBe('true')
  })

  it('lists every line of the picked slide, not only the two the card shows', () => {
    const { container } = renderDeck()
    fireEvent.click(cards(container)[1] as HTMLButtonElement)
    flush()
    const lines = container.querySelector('.verevon-deck__lines')
    expect(Array.from(lines?.querySelectorAll('li') ?? []).map((item) => item.textContent)).toEqual([
      'Omsetning 12,4 MNOK, opp 8 %',
      'Nord vokste mest',
      'Marginen holdt seg',
    ])
    expect(cards(container)[1]?.querySelectorAll('.verevon-deck__line')).toHaveLength(2)
  })

  it('says it is a draft until a render exists, and draws the preview only when the file has one', () => {
    const { container, unmount } = renderDeck()
    expect(screen.getByText('Draft from manifest')).toBeTruthy()
    expect(container.querySelector('iframe')).toBeNull()
    unmount()

    const rendered = renderDeck({ file: generatedFile({ preview: preview() }) })
    expect(screen.queryByText('Draft from manifest')).toBeNull()
    const frame = rendered.container.querySelector('iframe')
    expect(frame?.getAttribute('title')).toBe('Preview of Kvartalstall Q3')
  })

  it('draws the generated file card below the strip when the turn has one', () => {
    renderDeck({ file: generatedFile(), fallback: () => <p>kvartal.pptx download</p> })
    expect(screen.getByText('kvartal.pptx download')).toBeTruthy()
  })

  it('keeps the cards first in tab order, one per slide, and the notes toggle after them', () => {
    const { container } = renderDeck()
    fireEvent.click(cards(container)[1] as HTMLButtonElement)
    flush()
    const buttons = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
    expect(buttons.slice(0, 4).every((button) => button.classList.contains('verevon-deck__card'))).toBe(true)
    const toggle = buttons[buttons.length - 1]
    expect(toggle?.classList.contains('verevon-deck__toggle')).toBe(true)
    expect(buttons.every((button) => button.tabIndex >= 0)).toBe(true)
  })

  it('draws slide text as text, never as markup', () => {
    const { container } = renderDeck({
      content: manifestWith((manifest) => {
        const slides = manifest.slides as Json[]
        slides[0] = { ...slides[0], title: '<img src="x">' }
      }),
    })
    expect(container.querySelector('img')).toBeNull()
    expect(titles(container)[0]).toBe('<img src="x">')
  })

  it('shows the raw text, not a deck, for a manifest it cannot read', () => {
    const { container } = renderDeck({ content: 'not json' })
    expect(screen.getByText('Could not read the slides')).toBeTruthy()
    expect(container.querySelector('pre')?.textContent).toBe('not json')
    expect(container.querySelector('.verevon-deck__strip')).toBeNull()
  })

  it('refuses another manifest version the same way', () => {
    const { container } = renderDeck({
      content: manifestWith((manifest) => {
        manifest.version = 2
      }),
    })
    expect(screen.getByText('Could not read the slides')).toBeTruthy()
    expect(container.querySelector('.verevon-deck__strip')).toBeNull()
  })

  it('says a deck with no slides has none yet', () => {
    const { container } = renderDeck({
      content: manifestWith((manifest) => {
        manifest.slides = []
      }),
    })
    expect(screen.getByText('No slides yet')).toBeTruthy()
    expect(container.querySelectorAll('.verevon-deck__card')).toHaveLength(0)
  })

  it('draws each slide render as its card picture when the render has one per slide, in order', () => {
    const thumbs = [
      'data:image/png;base64,AAAA',
      'data:image/png;base64,BBBB',
      'data:image/png;base64,CCCC',
      'data:image/png;base64,DDDD',
    ]
    const { container } = renderDeck({ file: generatedFile({ preview: previewWith(4, thumbs) }) })
    expect(pictures(container)).toEqual(thumbs)
    expect(cards(container).every((card) => card.classList.contains('verevon-deck__card--picture'))).toBe(true)
  })

  it('draws no card picture when the render has a different page count from the slides', () => {
    const thumbs = ['data:image/png;base64,AAAA', 'data:image/png;base64,BBBB', 'data:image/png;base64,CCCC']
    const { container } = renderDeck({ file: generatedFile({ preview: previewWith(3, thumbs) }) })
    expect(pictures(container)).toEqual([null, null, null, null])
    expect(cards(container).some((card) => card.classList.contains('verevon-deck__card--picture'))).toBe(false)
  })

  it('draws no card picture when the render kept only some of its pages', () => {
    const thumbs = ['data:image/png;base64,AAAA', 'data:image/png;base64,BBBB']
    const { container } = renderDeck({ file: generatedFile({ preview: previewWith(4, thumbs) }) })
    expect(pictures(container)).toEqual([null, null, null, null])
  })
})
