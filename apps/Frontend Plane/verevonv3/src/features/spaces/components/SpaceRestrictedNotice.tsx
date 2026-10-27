import { Lock } from '@/shared/icons'
import { Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'

/**
 * The honest answer when a member may be in the room but may not read what a
 * thread or the room's transcript says. The organization has not enabled shared
 * reads for this Space (deny-by-default), which is a real answer and not a
 * sign-in problem, so this never tells the reader to log in again.
 *
 * One path for WS01-S7a (thread variant) and WS07-S4 (room variant), whose
 * prop contract is `{ variant, onRetry?, adminCta? }`.
 */
export interface SpaceRestrictedNoticeProps {
  /** `room` sits once above the timeline; `thread` sits inside one post or the pane. */
  readonly variant: 'room' | 'thread'
  /** Offered when the state might be transient; omit when retrying cannot help. */
  readonly onRetry?: () => void
  /** For a member who can change the Space's sharing: where to do it. */
  readonly adminCta?: { readonly label: string; readonly href: string }
}

export function SpaceRestrictedNotice(props: SpaceRestrictedNoticeProps) {
  const i18n = useI18n()
  return (
    <div
      class="verevon-space-restricted"
      data-variant={props.variant}
      role="status"
    >
      <Lock size={16} aria-hidden="true" />
      <div class="verevon-space-restricted__body">
        <strong>
          {props.variant === 'room'
            ? i18n.tr('Samtalene i dette rommet er ikke delt med deg', 'Conversations in this room are not shared with you')
            : i18n.tr('Denne samtalen er ikke delt med deg', 'This conversation is not shared with you')}
        </strong>
        <p>
          {props.variant === 'room'
            ? i18n.tr(
                'Du er medlem av rommet, men organisasjonen har ikke åpnet for at medlemmer kan lese hverandres samtaler her.',
                'You are a member of this room, but your organization has not enabled members to read each other’s conversations here.',
              )
            : i18n.tr(
                'Du er medlem av rommet, men du har ikke tilgang til å lese innholdet i denne tråden.',
                'You are a member of this room, but you cannot read the contents of this thread.',
              )}
        </p>
        <div class="verevon-space-restricted__actions">
          <Show when={props.onRetry}>
            {(retry) => (
              <button type="button" class="verevon-space-secondary-action" onClick={() => retry()()}>
                {i18n.tr('Prøv igjen', 'Try again')}
              </button>
            )}
          </Show>
          <Show when={props.adminCta}>
            {(cta) => (
              <a class="verevon-space-secondary-action" href={cta().href}>
                {cta().label}
              </a>
            )}
          </Show>
        </div>
      </div>
    </div>
  )
}
