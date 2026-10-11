/**
 * Context-aware start screen (CONTRACT.md amendment F): the suggestion card
 * says what happened, where it came from and when, a click only FILLS the
 * composer, and nothing is ever sent or acted on by the click.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { COMPOSER_TEXTAREA, fillComposer, installPromptSuggestions } from './suggestions'
import { nordlysSuggestion } from './fixtures/fjordform'
import { undersokSuggestion } from './fixtures/fjordform-undersok'

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

  it('puts one suggestion with source and time into the start screen heading', async () => {
    mountStartScreen()
    installPromptSuggestions({
      label: nordlysSuggestion.label,
      items: [{ text: nordlysSuggestion.text, source: nordlysSuggestion.source, time: nordlysSuggestion.time, icon: 'package' }],
    })
    await flush()
    const block = document.querySelector('.verevon-chat-empty__heading .v-suggestions')
    expect(block).not.toBeNull()
    expect(block!.querySelectorAll('.v-suggestion')).toHaveLength(1)
    expect(block!.querySelector('.v-suggestions__label')?.textContent).toBe('Dette har skjedd')
    expect(block!.querySelector('.v-suggestion__text')?.textContent).toBe(
      'Bring melder at Nordlys-leveransen er forsinket. Skal jeg sjekke konsekvensene og lage en plan for oppfølging?',
    )
    expect(block!.querySelector('.v-suggestion__source')?.textContent).toBe('Bring · sporingsvarsel')
    expect(block!.querySelector('.v-suggestion__time')?.textContent).toBe('i dag 08:12')
  })

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

  it('a click fills the composer with the question and neither sends nor acts', async () => {
    const { textarea, send } = mountStartScreen()
    const sent = vi.fn()
    send.addEventListener('click', sent)
    const submitted = vi.fn()
    document.addEventListener('submit', submitted)
    installPromptSuggestions({
      label: undersokSuggestion.label,
      items: [{ text: undersokSuggestion.text, source: undersokSuggestion.source, time: undersokSuggestion.time, icon: 'receipt' }],
    })
    await flush()
    document.querySelector<HTMLButtonElement>('.v-suggestion')!.click()
    expect(textarea.value).toBe('Denne fakturaen ser ut til å avvike fra avtalen. Skal jeg undersøke hvorfor?')
    expect(document.querySelector('.v-suggestion')?.classList.contains('is-chosen')).toBe(true)
    expect(sent).not.toHaveBeenCalled()
    expect(submitted).not.toHaveBeenCalled()
    document.removeEventListener('submit', submitted)
  })
})

describe('the films\' suggestion fixtures', () => {
  it('state what happened, the source and a time, and propose a next action', () => {
    for (const suggestion of [nordlysSuggestion, undersokSuggestion]) {
      expect(suggestion.text).toMatch(/\?$/)
      expect(suggestion.source.length).toBeGreaterThan(3)
      expect(suggestion.time).toMatch(/i dag \d{2}:\d{2}/)
    }
  })

  it('use Bring, not the order system, as the tracking source in scene 02', () => {
    expect(nordlysSuggestion.source).toContain('Bring')
    expect(nordlysSuggestion.source).not.toMatch(/Visma/i)
  })
})
