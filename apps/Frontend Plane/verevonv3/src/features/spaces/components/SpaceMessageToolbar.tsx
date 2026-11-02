import { Show } from 'solid-js'

import { Pencil } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { SpaceConfirmButton } from './SpaceConfirmButton'
import './space-message-lifecycle.css'

/**
 * The hover toolbar of one message (WS12-S7): Edit and Delete, each shown only when the viewer may
 * use it. The page decides what to offer from the room role and the organisation's policy hints; the
 * server still decides every change. Delete asks in place (SpaceConfirmButton) and says plainly that
 * the text is no longer available afterwards.
 */
export function SpaceMessageToolbar(props: {
  readonly canEdit: boolean
  readonly canDelete: boolean
  /** The message belongs to someone else (a moderator removal): the confirmation says so. */
  readonly moderating?: boolean
  readonly busy?: boolean
  readonly onEdit: () => void
  readonly onDelete: () => void
}) {
  const i18n = useI18n()
  return (
    <Show when={props.canEdit || props.canDelete}>
      <div
        class="verevon-message-toolbar"
        role="toolbar"
        aria-label={i18n.tr('Meldingshandlinger', 'Message actions')}
      >
        <Show when={props.canEdit}>
          <button
            type="button"
            class="verevon-message-toolbar__button"
            disabled={props.busy}
            onClick={() => props.onEdit()}
          >
            <Pencil size={14} aria-hidden="true" />
            <span>{i18n.tr('Rediger', 'Edit')}</span>
          </button>
        </Show>
        <Show when={props.canDelete}>
          <SpaceConfirmButton
            class="verevon-message-toolbar__button verevon-message-toolbar__button--danger"
            label={i18n.tr('Slett', 'Delete')}
            confirmLabel={i18n.tr('Bekreft sletting av meldingen', 'Confirm deleting the message')}
            consequence={
              props.moderating
                ? i18n.tr(
                    'Alle ser at en leder fjernet meldingen. Teksten er ikke tilgjengelig lenger.',
                    'Everyone sees that a manager removed the message. The text is no longer available.',
                  )
                : i18n.tr(
                    'Alle ser at meldingen er slettet. Teksten er ikke tilgjengelig lenger.',
                    'Everyone sees that the message was deleted. The text is no longer available.',
                  )
            }
            disabled={props.busy}
            onConfirm={() => props.onDelete()}
          />
        </Show>
      </div>
    </Show>
  )
}
