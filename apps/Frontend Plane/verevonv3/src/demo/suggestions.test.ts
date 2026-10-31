/**
 * Context-aware start screen (CONTRACT.md amendment F): the suggestion card
 * says what happened, where it came from and when, a click only FILLS the
 * composer, and nothing is ever sent or acted on by the click. Each of the
 * demo's three cases gets the same card, so each is tested the same way.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { anbudSuggestion } from './fixtures/fjordform-anbud'
import { reklamasjonSuggestion } from './fixtures/fjordform-reklamasjon'
import { tilbudSuggestion } from './fixtures/fjordform-tilbud'
import { COMPOSER_TEXTAREA, fillComposer, installPromptSuggestions, type SuggestionIcon } from './suggestions'

type Card = { label: string; text: string; source: string; time: string }

/** The three cases' start-screen cards, each with the icon its case shows on the start screen. */
const CASE_CARDS: ReadonlyArray<{ name: string; card: Card; icon: SuggestionIcon }> = [
  { name: 'Tilbud til Solhaug Hotell', card: tilbudSuggestion, icon: 'mail' },
  { name: 'Reklamasjon fra Havnebrygga Kafé', card: reklamasjonSuggestion, icon: 'package' },
  { name: 'Dokumentasjon til Nordhavn kommune', card: anbudSuggestion, icon: 'receipt' },
]

function mountStartScreen(): { textarea: HTMLTextAreaElement; send: HTMLButtonElement } {
  document.body.innerHTML = `
    <div class="verevon-chat-empty"><div class="verevon-chat-empty__inner">
      <div class="verevon-chat-empty__heading"><h1>God dag, Ida</h1><p>Spør om noe dere jobber med.</p></div>
      <div class="verevon-chat-empty__composer">
        <textarea id="verevon-dashboard-input"></textarea>
        <button type="button" aria-label="Send melding">Send</button>
      </div>
    </div></div>`
  return {
    textarea: document.querySelector<HTMLTextAreaElement>(COMPOSER_TEXTAREA)!,
    send: document.querySelector<HTMLButtonElement>('button[aria-label="Send melding"]')!,
  }
}

async function flush(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0))
}

describe('fillComposer', () => {
  beforeEach(() => { document.body.innerHTML = '' })

  it('sets the real textarea value and fires an input event', () => {
    const { textarea } = mountStartScreen()
    const heard = vi.fn()
    textarea.addEventListener('input', heard)
    expect(fillComposer('Hei')).toBe(true)
    expect(textarea.value).toBe('Hei')
    expect(heard).toHaveBeenCalledTimes(1)
  })

  it('reports false when the start screen has no composer', () => {
    expect(fillComposer('Hei')).toBe(false)
  })
})

describe('installPromptSuggestions', () => {
  afterEach(() => { document.body.innerHTML = '' })

  it('does not insert the block twice when the screen re-renders', async () => {
    const { textarea } = mountStartScreen()
    installPromptSuggestions({
      label: 'Dette har skjedd',
      items: [{ text: 'Spørsmål?', source: 'Kilde', time: 'i dag 08:00', icon: 'receipt' }],
    })
    await flush()
    textarea.parentElement!.append(document.createElement('span'))
    await flush()
    expect(document.querySelectorAll('.v-suggestions')).toHaveLength(1)
  })

  describe.each(CASE_CARDS)('the card of $name', ({ card, icon }) => {
    it('puts the card with its source and time into the start screen heading', async () => {
      mountStartScreen()
      installPromptSuggestions({
        label: card.label,
        items: [{ text: card.text, source: card.source, time: card.time, icon }],
      })
      await flush()
      const block = document.querySelector('.verevon-chat-empty__heading .v-suggestions')
      expect(block).not.toBeNull()
      expect(block!.querySelectorAll('.v-suggestion')).toHaveLength(1)
      expect(block!.querySelector('.v-suggestions__label')?.textContent).toBe(card.label)
      expect(block!.querySelector('.v-suggestion__text')?.textContent).toBe(card.text)
      expect(block!.querySelector('.v-suggestion__source')?.textContent).toBe(card.source)
      expect(block!.querySelector('.v-suggestion__time')?.textContent).toBe(card.time)
    })

    it('a click fills the composer with the question and neither sends nor acts', async () => {
      const { textarea, send } = mountStartScreen()
      const sent = vi.fn()
      send.addEventListener('click', sent)
      const submitted = vi.fn()
      document.addEventListener('submit', submitted)
      installPromptSuggestions({
        label: card.label,
        items: [{ text: card.text, source: card.source, time: card.time, icon }],
      })
      await flush()
      document.querySelector<HTMLButtonElement>('.v-suggestion')!.click()
      expect(textarea.value).toBe(card.text)
      expect(document.querySelector('.v-suggestion')?.classList.contains('is-chosen')).toBe(true)
      expect(sent).not.toHaveBeenCalled()
      expect(submitted).not.toHaveBeenCalled()
      document.removeEventListener('submit', submitted)
    })

    it('states what happened, names its source and a time, and proposes a next action', () => {
      expect(card.text).toMatch(/\?$/)
      expect(card.source.length).toBeGreaterThan(3)
      expect(card.time).toMatch(/i dag \d{2}:\d{2}/)
    })
  })
})
