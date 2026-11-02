import { Show } from 'solid-js'

import { Lock } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'

import './space-entry-redirect.css'

/**
 * WS13-S4b: what a `/spaces/:spaceId/m/:messageId` link shows when the entry cannot be opened.
 * Hidden, missing, foreign and "not a member" all look the same on purpose (the resolver answers
 * `entry_not_found` for every one of them), so this view never carries a title, a preview, the
 * message id or a reason. It only offers the way back to the room and, for a failure that can pass,
 * a retry.
 */
export interface SpaceEntryUnavailableProps {
  /** The room the link named, for the way back. Omitted when the link did not name a readable one. */
  readonly spaceRef?: string
  /** `retry` is a transient failure (503, no network): the link may still work. */
  readonly variant: 'unavailable' | 'retry'
  readonly onRetry?: () => void
}

export function SpaceEntryUnavailable(props: SpaceEntryUnavailableProps) {
  const i18n = useI18n()
  return (
    <main class="verevon-space-entry" aria-labelledby="verevon-space-entry-title">
      <div class="verevon-space-restricted verevon-space-entry__card" role="status">
        <Lock size={16} aria-hidden="true" />
        <div class="verevon-space-restricted__body">
          <strong id="verevon-space-entry-title">
            {props.variant === 'retry'
              ? i18n.tr('Meldingen kunne ikke åpnes akkurat nå', 'This message could not be opened right now')
              : i18n.tr('Denne meldingen er ikke tilgjengelig for deg.', 'This message is not available to you.')}
          </strong>
          <p>
            {props.variant === 'retry'
              ? i18n.tr('Tilgangen kunne ikke kontrolleres. Prøv igjen om litt.', 'Access could not be checked. Try again shortly.')
              : i18n.tr(
                  'Lenken kan være utløpt, eller du har ikke tilgang til samtalen den peker på.',
                  'The link may have expired, or you may not have access to the conversation it points to.',
                )}
          </p>
          <div class="verevon-space-restricted__actions">
            <Show when={props.variant === 'retry' && props.onRetry}>
              {(retry) => (
                <button type="button" class="verevon-space-secondary-action" onClick={() => retry()()}>
                  {i18n.tr('Prøv igjen', 'Try again')}
                </button>
              )}
            </Show>
            <a
              class="verevon-space-secondary-action"
              href={props.spaceRef ? `/spaces/${encodeURIComponent(props.spaceRef)}` : '/spaces'}
            >
              {props.spaceRef ? i18n.tr('Gå til rommet', 'Go to the room') : i18n.tr('Gå til rommene', 'Go to your rooms')}
            </a>
          </div>
        </div>
      </div>
    </main>
  )
}

export default SpaceEntryUnavailable
