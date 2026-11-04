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
import type { ChatArtifact } from './chat-types'

const here = dirname(fileURLToPath(import.meta.url))
const fixture = readFileSync(join(here, '..', '..', '..', 'shared', 'api', 'contracts', 'deck_manifest_v1.json'), 'utf8')

type Json = Record<string, unknown>

function manifestWith(change: (manifest: Json) => void): string {
  const manifest = JSON.parse(fixture) as Json
  change(manifest)
  return JSON.stringify(manifest)
}

/** The fixture with each slide's state set in order. The present gate reads nothing else of a slide. */
function withStates(...states: string[]): string {
  return manifestWith((manifest) => {
    const slides = manifest.slides as Json[]
    manifest.slides = slides.map((slide, index) => ({ ...slide, state: states[index] }))
  })
}

function artifact(content: string): ChatArtifact {
  return { content, id: 'artifact-deck', kind: 'deck', title: 'Kvartalstall Q3', version: 5 }
}

function renderDeck(content: string) {
  return render(() => (
    <I18nProvider>
      <ArtifactDeckView artifact={artifact(content)} />
    </I18nProvider>
  ))
}

function presenter(): HTMLButtonElement {
  return screen.getByRole('button', { name: 'Present' }) as HTMLButtonElement
}

describe('ArtifactDeckView present gate', () => {
  beforeEach(() => {
    window.localStorage.setItem(localeStorageKey, 'en')
  })

  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('blocks Presenter while a slide has an error or was never built, and says why', () => {
    renderDeck(fixture)
    expect(presenter().disabled).toBe(true)
    const reason = screen.getByText(
      '1 slide has an error. 1 slide is not built. Ask in the chat to fix and build the slides before you present.',
    )
    expect(presenter().getAttribute('aria-describedby')).toBe(reason.id)
  })

  it('blocks Presenter for a slide that was never built, when no slide has an error', () => {
    renderDeck(withStates('ok', 'ok', 'draft', 'ok'))
    expect(presenter().disabled).toBe(true)
    expect(
      screen.getByText('1 slide is not built. Ask in the chat to fix and build the slides before you present.'),
    ).toBeTruthy()
  })

  it('lets Presenter open with warnings only, and counts the warnings', () => {
    renderDeck(withStates('ok', 'warn', 'ok', 'ok'))
    expect(presenter().disabled).toBe(false)
    expect(presenter().getAttribute('aria-describedby')).toBeNull()
    expect(screen.getByText('1 slide has warnings')).toBeTruthy()

    fireEvent.click(presenter())
    flush()
    expect(screen.getByRole('dialog', { name: 'Presentation: Kvartalstall Q3' })).toBeTruthy()
  })

  it('gives the focus back to Presenter when the presentation closes', () => {
    renderDeck(withStates('ok', 'warn', 'ok', 'ok'))
    presenter().focus()
    fireEvent.click(presenter())
    flush()
    fireEvent.keyDown(document, { key: 'Escape' })
    flush()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(document.activeElement).toBe(presenter())
  })

  it('keeps Presenter off for a deck with no slides, and says what is missing', () => {
    renderDeck(
      manifestWith((manifest) => {
        manifest.slides = []
      }),
    )
    expect(presenter().disabled).toBe(true)
    expect(screen.getByText('A presentation needs at least one slide.')).toBeTruthy()
  })
})
