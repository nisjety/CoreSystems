import { useI18n } from '@/shared/i18n'

/**
 * Type-to-find for a model menu. Escape clears a query and keeps the menu open; on an empty query
 * Escape is left alone, so the menu closes as it always did.
 */
export function ModelSearchField(props: {
  value: string
  /** The id of the list this field filters, so assistive technology can follow the results. */
  controlsId: string
  onChange: (value: string) => void
}) {
  const i18n = useI18n()
  const label = () => i18n.tr('Søk etter modell', 'Search models')
  return (
    <input
      type="search"
      class="dashboard-composer-model-search"
      value={props.value}
      placeholder={label()}
      aria-label={label()}
      aria-controls={props.controlsId}
      autocomplete="off"
      spellcheck={false}
      onInput={(event) => props.onChange(event.currentTarget.value)}
      onKeyDown={(event) => {
        if (event.key !== 'Escape' || props.value === '') return
        event.preventDefault()
        event.stopPropagation()
        props.onChange('')
      }}
    />
  )
}
