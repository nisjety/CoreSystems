import { For, createEffect } from 'solid-js'
import { SlidersHorizontal } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import {
  chatAppearanceChoices,
  chooseChatAppearance,
  compareChatOriginal,
  comparingOriginal,
  resetChatAppearance,
  tryAllChatAppearance,
  type ChatAppearanceKey,
} from '../lib/chat-appearance-preview'
import './chat-appearance-preview.css'
import './chat-appearance-refinements.css'

/** Development-only appearance comparison. No workspace settings or chat
 * state are written, and toggles leave the mounted composer intact. */
export function ChatAppearancePreview(props: { embedded?: boolean } = {}) {
  const i18n = useI18n()
  let root: HTMLDetailsElement | undefined
  let trigger: HTMLElement | undefined
  const options = (): { key: ChatAppearanceKey; label: string; description: string }[] => [
    { key: 'welcome', label: i18n.tr('Enklere velkomst', 'Simpler welcome'), description: i18n.tr('Venstrestilt hilsen, eksempler ved behov.', 'Left-aligned greeting, prompt ideas on demand.') },
    { key: 'dock', label: i18n.tr('Skrivefelt nederst', 'Bottom composer'), description: i18n.tr('Mer luft, skrivefeltet nær bunnen.', 'More breathing room, input near the bottom.') },
    { key: 'composer', label: i18n.tr('Flatere skrivefelt', 'Flatter composer'), description: i18n.tr('Samlet flate med diskrete kanter.', 'One surface with subtle edges.') },
    { key: 'history', label: i18n.tr('Kompakt historikk', 'Compact history'), description: i18n.tr('Renere rader, datoer ved pek eller fokus.', 'Cleaner rows, dates on hover or focus.') },
    { key: 'navigation', label: i18n.tr('Roligere navigasjon', 'Softer navigation'), description: i18n.tr('Tydelig søk, sidefeltknapp og en Mer-meny.', 'Clear search, sidebar toggle and a More menu.') },
    { key: 'organizer', label: i18n.tr('Organiser samtaler', 'Chat organizer'), description: i18n.tr('Søk, sorter, grupper og samle i prosjekter.', 'Search, sort, group and organize into projects.') },
    { key: 'context', label: i18n.tr('Samlet kontekstknapp', 'Combined context button'), description: i18n.tr('Ikon og kunnskapstekst i samme knapp.', 'Knowledge icon and text inside one button.') },
    { key: 'dashboard', label: i18n.tr('Ny startside', 'Dashboard overview'), description: i18n.tr('Nylig arbeid, rolige kort og et kompakt skrivefelt.', 'Recent work, quiet panels and a compact chat dock.') },
  ]

  createEffect(() => undefined, () => {
    const closeOutside = (event: PointerEvent) => {
      if (root?.open && event.target instanceof Node && !root.contains(event.target)) root.open = false
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !root?.open) return
      root.open = false
      trigger?.focus()
    }
    document.addEventListener('pointerdown', closeOutside)
    document.addEventListener('keydown', closeOnEscape)
    return () => {
      document.removeEventListener('pointerdown', closeOutside)
      document.removeEventListener('keydown', closeOnEscape)
    }
  })

  return (
    <details ref={root} class={`chat-appearance-preview${props.embedded ? ' chat-appearance-preview--embedded' : ''}`}>
      <summary ref={trigger} title={i18n.tr('Sammenlign chat-utseende', 'Compare chat appearance')}>
        <SlidersHorizontal size={16} aria-hidden="true" />
        <span>{i18n.tr('Utseende', 'Appearance')}</span>
      </summary>
      <section class="chat-appearance-preview__panel" aria-label={i18n.tr('Forhåndsvis chat-utseende', 'Chat appearance preview')}>
        <h2>{i18n.tr('Finn din balanse', 'Find your balance')}</h2>
        <p>{i18n.tr('Prøv én endring om gangen. Valgene lagres bare i denne nettleseren.', 'Try one change at a time. Choices stay in this browser.')}</p>
        <div class="chat-appearance-preview__compare" role="group" aria-label={i18n.tr('Sammenlign utseende', 'Compare appearance')}>
          <button type="button" aria-pressed={comparingOriginal() ? 'true' : 'false'} onClick={() => compareChatOriginal(true)}>{i18n.tr('Original', 'Original')}</button>
          <button type="button" aria-pressed={comparingOriginal() ? 'false' : 'true'} onClick={() => compareChatOriginal(false)}>{i18n.tr('Ditt valg', 'Your mix')}</button>
        </div>
        <fieldset>
          <legend class="sr-only">{i18n.tr('Visuelle endringer', 'Visual changes')}</legend>
          <For each={options()}>{(option) => (
            <label class="chat-appearance-preview__option">
              <span><strong>{option.label}</strong><small>{option.description}</small></span>
              <input type="checkbox" checked={chatAppearanceChoices()[option.key]} onChange={(event) => chooseChatAppearance(option.key, event.currentTarget.checked)} />
            </label>
          )}</For>
        </fieldset>
        <div class="chat-appearance-preview__footer">
          <button type="button" onClick={resetChatAppearance}>{i18n.tr('Tilbakestill', 'Reset all')}</button>
          <button type="button" onClick={tryAllChatAppearance}>{i18n.tr('Prøv alle', 'Try all')}</button>
        </div>
      </section>
    </details>
  )
}
