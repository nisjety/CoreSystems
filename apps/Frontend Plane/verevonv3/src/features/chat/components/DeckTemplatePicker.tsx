import { createEffect, createSignal, For, Show } from 'solid-js'
import { listDeckTemplates, type DeckTemplate } from '@/shared/api/chat-client'
import { useI18n } from '@/shared/i18n'

/** A template's swatches in the order the picker shows them: the four named colours, then the accents. */
export function deckTemplateSwatches(template: DeckTemplate): string[] {
  const { primary, secondary, text, background, accent } = template.colors
  return [primary, secondary, text, background, ...accent]
}

/**
 * The brand deck template picker (EP-5 S7; actions `chat.list_deck_templates` and `chat.apply_deck_template`).
 * "Standard" (the neutral default theme, `null`) comes first, then the organisation's templates with their
 * colour swatches and font names; there are no thumbnails in W3. A pick hands the caller the template, which
 * sends it as `deckTemplate`, a skill id, on the next turn: the server checks the id again and the model
 * never chooses one.
 *
 * The picker hides itself while the gateway's deck templates are off (the list route answers 404), for a
 * zero-retention session (refused), and when the organisation has no template, so it is never an empty
 * choice.
 */
export function DeckTemplatePicker(props: {
  selected: string | null
  onSelect: (template: DeckTemplate | null) => void
  disabled?: boolean
}) {
  const i18n = useI18n()
  const [templates, setTemplates] = createSignal<DeckTemplate[] | null>(null)
  const [failed, setFailed] = createSignal(false)

  // Solid v2 has no onMount: a two-phase effect with a constant compute runs once after mount.
  createEffect(() => undefined, () => {
    let live = true
    listDeckTemplates()
      .then((listed) => {
        if (live) setTemplates(listed)
      })
      .catch(() => {
        if (live) setFailed(true)
      })
    return () => {
      live = false
    }
  })

  const choose = (template: DeckTemplate | null) => {
    if (props.disabled) return
    props.onSelect(template)
  }

  return (
    <>
      <Show when={failed()}>
        <p class="verevon-deck-template-picker__error" role="status">
          {i18n.tr('Kunne ikke hente malene nå', 'Could not load the templates right now')}
        </p>
      </Show>
      <Show when={(templates()?.length ?? 0) > 0}>
        <div
          class="verevon-deck-template-picker"
          role="radiogroup"
          aria-label={i18n.tr('Mal for presentasjonen', 'Presentation template')}
        >
          <button
            type="button"
            role="radio"
            class="verevon-deck-template-picker__option"
            aria-checked={props.selected === null ? 'true' : 'false'}
            disabled={props.disabled}
            onClick={() => choose(null)}
          >
            <span class="verevon-deck-template-picker__name">{i18n.tr('Standard', 'Standard')}</span>
            <span class="verevon-deck-template-picker__fonts">{i18n.tr('Nøytralt tema', 'Neutral theme')}</span>
          </button>
          <For each={templates() ?? []}>
            {(template) => (
              <button
                type="button"
                role="radio"
                class="verevon-deck-template-picker__option"
                aria-checked={props.selected === template.skillId ? 'true' : 'false'}
                disabled={props.disabled}
                title={template.description || undefined}
                onClick={() => choose(template)}
              >
                <span class="verevon-deck-template-picker__name">{template.name}</span>
                <span class="verevon-deck-template-picker__swatches" aria-hidden="true">
                  <For each={deckTemplateSwatches(template)}>
                    {(colour) => (
                      <span class="verevon-deck-template-picker__swatch" style={{ 'background-color': colour }} />
                    )}
                  </For>
                </span>
                <span class="verevon-deck-template-picker__fonts">
                  {template.fonts.heading === template.fonts.body
                    ? template.fonts.heading
                    : `${template.fonts.heading} / ${template.fonts.body}`}
                </span>
              </button>
            )}
          </For>
        </div>
      </Show>
    </>
  )
}
