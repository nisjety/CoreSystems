import { Show } from 'solid-js'

import {
  spaceStatusFrom,
  type SpaceAgentActivity,
  type SpaceMemberStatus,
  type SpaceMemberStatusState,
} from '@/shared/api/spaces-client'
import { useI18n } from '@/shared/i18n'
import './space-status.css'

/** The one word each state is shown with (the dot colour never carries the meaning alone). */
export function statusStateLabel(state: SpaceMemberStatusState, tr: (no: string, en: string) => string): string {
  switch (state) {
    case 'available':
      return tr('Tilgjengelig', 'Available')
    case 'away':
      return tr('Borte', 'Away')
    case 'dnd':
      return tr('Ikke forstyrr', 'Do not disturb')
    case 'offline':
      return tr('Frakoblet', 'Offline')
  }
}

export function agentActivityLabel(activity: SpaceAgentActivity, tr: (no: string, en: string) => string): string {
  switch (activity) {
    case 'working':
      return tr('Jobber', 'Working')
    case 'waiting':
      return tr('Venter', 'Waiting')
    case 'idle':
      return tr('Ledig', 'Idle')
  }
}

/**
 * A person's status in this organization: a dot, the state word and, when the server sent one, the
 * short text (plain text only, never markup). Renders nothing when the roster carried no status:
 * absent and "none set" both leave the row exactly as it was.
 */
export function SpaceMemberStatusChip(props: { readonly status: unknown }) {
  const i18n = useI18n()
  const status = (): SpaceMemberStatus | null => spaceStatusFrom(props.status)
  return (
    <Show when={status()}>
      {(current) => (
        <span
          class="verevon-space-status-chip"
          data-state={current().state}
          title={current().text ?? undefined}
        >
          <span class="verevon-space-status-chip__dot" aria-hidden="true" />
          <span>{statusStateLabel(current().state, i18n.tr)}</span>
          <Show when={current().text}>
            {(text) => <span class="verevon-space-status-chip__text">· {text()}</span>}
          </Show>
        </span>
      )}
    </Show>
  )
}

/** An agent's availability in this room. Renders nothing when the gateway sent none. */
export function SpaceAgentActivityChip(props: { readonly activity: SpaceAgentActivity | undefined }) {
  const i18n = useI18n()
  return (
    <Show when={props.activity}>
      {(activity) => (
        <span class="verevon-space-status-chip" data-state={activity()}>
          <span class="verevon-space-status-chip__dot" aria-hidden="true" />
          <span>{agentActivityLabel(activity(), i18n.tr)}</span>
        </span>
      )}
    </Show>
  )
}
