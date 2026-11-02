import { For, Show } from 'solid-js'

import { useI18n } from '@/shared/i18n'
import type { MentionCandidate } from '../lib/mention-model'
import { AgentAvatar } from './AgentAvatar'

/**
 * The `@` picker (WS08-S6): one ranked list in up to three sections, People, Agents and Add to
 * room. It is a presentational listbox; the composer keeps the textarea as the combobox
 * (`aria-controls` this list, `aria-activedescendant` the highlighted row's id) and owns the keys,
 * so focus never leaves the box. The list scrolls instead of cutting at six rows.
 */

export type MentionPickerRow =
  | { readonly kind: 'candidate'; readonly candidate: MentionCandidate }
  /** An org agent a manager can bind to this room and mention in one step. */
  | { readonly kind: 'add'; readonly agentRef: string; readonly name: string }
  /** Someone who cannot bind agents asks the room's managers to add one. */
  | { readonly kind: 'request'; readonly name: string }

type SectionId = 'people' | 'agents' | 'add'

function sectionOf(row: MentionPickerRow): SectionId {
  if (row.kind === 'add' || row.kind === 'request') return 'add'
  return row.candidate.kind === 'agent' ? 'agents' : 'people'
}

export interface MentionPickerProps {
  readonly id: string
  readonly rows: () => readonly MentionPickerRow[]
  readonly activeIndex: () => number
  readonly onHover: (index: number) => void
  readonly onPick: (row: MentionPickerRow) => void
}

export function MentionPicker(props: MentionPickerProps) {
  const i18n = useI18n()
  const sectionLabel = (section: SectionId) =>
    section === 'people'
      ? i18n.tr('Personer', 'People')
      : section === 'agents'
        ? i18n.tr('Agenter', 'Agents')
        : i18n.tr('Legg til i rommet', 'Add to room')
  return (
    <ul
      id={props.id}
      class="verevon-space-room-mentions verevon-space-mention-picker"
      role="listbox"
      aria-label={i18n.tr('Nevn noen', 'Mention someone')}
    >
      <For each={props.rows()}>
        {(row, index) => {
          const startsSection = () => index() === 0 || sectionOf(props.rows()[index() - 1] as MentionPickerRow) !== sectionOf(row)
          return (
            <>
              <Show when={startsSection()}>
                <li class="verevon-space-mention-picker__section" role="presentation">
                  {sectionLabel(sectionOf(row))}
                </li>
              </Show>
              <li role="presentation">
                <button
                  type="button"
                  role="option"
                  id={`${props.id}-${index()}`}
                  aria-selected={index() === props.activeIndex() ? 'true' : 'false'}
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseEnter={() => props.onHover(index())}
                  onClick={() => props.onPick(row)}
                >
                  <Show
                    when={row.kind === 'candidate' ? row.candidate : undefined}
                    fallback={
                      <>
                        <span class="verevon-space-room-mentions__kind" aria-hidden="true">+</span>
                        {row.kind === 'add'
                          ? i18n.tr(`Legg til ${row.name}`, `Add ${row.name}`)
                          : row.kind === 'request'
                            ? i18n.tr(`Be en leder legge til ${row.name}`, `Ask a manager to add ${row.name}`)
                            : ''}
                      </>
                    }
                  >
                    {(candidate) => (
                      <>
                        <Show
                          when={candidate().kind === 'agent'}
                          fallback={<span class="verevon-space-room-mentions__kind" aria-hidden="true">·</span>}
                        >
                          <AgentAvatar kind="agent" size={18} name={candidate().name} color={candidate().color} />
                        </Show>
                        {candidate().label}
                      </>
                    )}
                  </Show>
                </button>
              </li>
            </>
          )
        }}
      </For>
    </ul>
  )
}
