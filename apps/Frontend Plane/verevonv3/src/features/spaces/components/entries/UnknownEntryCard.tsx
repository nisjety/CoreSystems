import { Info } from '@/shared/icons'

import { useI18n } from '@/shared/i18n'
import type { TurnEntryCardProps } from '../../lib/entry-registry'
import { registerFallbackTurnEntryCard } from '../../lib/entry-registry'
import './space-entries.css'

/**
 * The neutral card for an entry this version of Verevon cannot draw (a newer kind or version, or a
 * payload that did not parse). It names nothing from the entry: the kind is a closed word from the
 * server and is kept in a data attribute for support, not shown.
 */
export function UnknownEntryCard(props: TurnEntryCardProps) {
  const i18n = useI18n()
  return (
    <div
      class="verevon-room-entry verevon-room-entry--unknown"
      data-entry-kind={props.entry.kind === 'unknown' ? props.entry.rawKind : props.entry.kind}
      role="note"
    >
      <span class="verevon-room-entry__icon" aria-hidden="true">
        <Info size={14} />
      </span>
      <span class="verevon-room-entry__text">
        {i18n.tr(
          'Dette elementet støttes ikke i denne versjonen av Verevon.',
          'This item is not supported in this version of Verevon.',
        )}
      </span>
    </div>
  )
}

registerFallbackTurnEntryCard(UnknownEntryCard)
