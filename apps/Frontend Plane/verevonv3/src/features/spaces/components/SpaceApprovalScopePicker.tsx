import { For, Show } from 'solid-js'

import type { ApprovalScope } from '@/shared/api/space-approvals-client'
import { useI18n } from '@/shared/i18n'

/**
 * How far one approval reaches (build plan WS10 S4).
 *
 * The options are decided by the caller from what the server states (`offerableScopes`): the
 * Space ceiling, the member's role and whether the tool can carry a standing rule at all. This
 * component only draws them. With `once` as the sole option it draws nothing, so a card that
 * cannot be widened never shows a choice that looks like one.
 *
 * "This conversation" is not drawn: it has no store yet, and the gateway refuses it.
 */
export interface SpaceApprovalScopePickerProps {
  readonly approvalId: string
  /** `once` first; anything else only when the server allows it. */
  readonly options: readonly ApprovalScope[]
  readonly value: ApprovalScope
  readonly disabled?: boolean
  readonly onChange: (scope: ApprovalScope) => void
}

export function SpaceApprovalScopePicker(props: SpaceApprovalScopePickerProps) {
  const i18n = useI18n()
  const label = (scope: ApprovalScope): string => {
    switch (scope) {
      case 'always':
        return i18n.tr('Alltid for dette verktøyet i rommet', 'Always, for this tool in this room')
      case 'thread':
        return i18n.tr('I denne samtalen', 'In this conversation')
      default:
        return i18n.tr('Bare denne gangen', 'Just this once')
    }
  }
  return (
    <Show when={props.options.length > 1}>
      <fieldset class="verevon-room-approval__scope" disabled={props.disabled}>
        <legend>{i18n.tr('Gjelder for', 'Applies to')}</legend>
        <For each={props.options}>
          {(scope) => (
            <label class="verevon-room-approval__scope-option">
              <input
                type="radio"
                name={`approval-scope-${props.approvalId}`}
                value={scope}
                checked={props.value === scope}
                onChange={() => props.onChange(scope)}
              />
              <span>{label(scope)}</span>
            </label>
          )}
        </For>
        <Show when={props.value === 'always'}>
          <p class="verevon-room-approval__consequence">
            {i18n.tr(
              'En fast regel lar agenten bruke dette verktøyet uten å spørre igjen, til en leder trekker den tilbake eller den utløper.',
              'A standing rule lets the agent use this tool without asking again, until a manager revokes it or it expires.',
            )}
          </p>
        </Show>
      </fieldset>
    </Show>
  )
}
