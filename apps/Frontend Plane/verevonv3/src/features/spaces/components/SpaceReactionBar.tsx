import { createSignal, For, Show } from 'solid-js'

import { Plus } from '@/shared/icons'
import { useI18n } from '@/shared/i18n'
import { SPACE_QUICK_REACTIONS } from '@/shared/api/spaces-client'
import type { SpaceReactionChip } from '@/shared/api/spaces-client'
import './space-message-lifecycle.css'

/**
 * The reaction chips of one message (WS12-S8): one chip per emoji with its count, pressed when the
 * viewer used it, plus an add button that opens a small picker of 24 emoji. Whether the controls are
 * offered comes from the page (room role and the `features.reactions` flag); the server decides each
 * change and a refusal is shown by the caller, never swallowed here. A viewer sees the chips as
 * plain, disabled marks.
 */
export function SpaceReactionBar(props: {
  readonly chips: readonly SpaceReactionChip[]
  readonly canReact: boolean
  readonly busy?: boolean
  /** Adds (`present`) or removes the viewer's emoji. */
  readonly onToggle: (emoji: string, present: boolean) => void
}) {
  const i18n = useI18n()
  const [open, setOpen] = createSignal(false)
  const label = (chip: SpaceReactionChip) =>
    chip.reacted
      ? i18n.tr(
          `${chip.emoji} fra deg og ${Math.max(chip.count - 1, 0)} andre. Trykk for å fjerne.`,
          `${chip.emoji} from you and ${Math.max(chip.count - 1, 0)} others. Press to remove.`,
        )
      : i18n.tr(`${chip.emoji} fra ${chip.count}. Trykk for å legge til.`, `${chip.emoji} from ${chip.count}. Press to add.`)
  const pick = (emoji: string) => {
    setOpen(false)
    const mine = props.chips.some((chip) => chip.emoji === emoji && chip.reacted)
    if (!mine) props.onToggle(emoji, true)
  }
  return (
    <Show when={props.chips.length > 0 || props.canReact}>
      <div
        class="verevon-reaction-bar"
        role="group"
        aria-label={i18n.tr('Reaksjoner', 'Reactions')}
        onKeyDown={(event) => {
          if (event.key === 'Escape' && open()) {
            event.stopPropagation()
            setOpen(false)
          }
        }}
      >
        <For each={props.chips}>
          {(chip) => (
            <button
              type="button"
              class="verevon-reaction-chip"
              aria-pressed={chip.reacted ? 'true' : 'false'}
              aria-label={label(chip)}
              disabled={!props.canReact || props.busy}
              onClick={() => props.onToggle(chip.emoji, !chip.reacted)}
            >
              <span aria-hidden="true">{chip.emoji}</span>
              <span class="verevon-reaction-chip__count">{chip.count}</span>
            </button>
          )}
        </For>
        <Show when={props.canReact}>
          <span class="verevon-reaction-picker">
            <button
              type="button"
              class="verevon-reaction-chip verevon-reaction-chip--add"
              aria-haspopup="true"
              aria-expanded={open() ? 'true' : 'false'}
              aria-label={i18n.tr('Legg til reaksjon', 'Add reaction')}
              disabled={props.busy}
              onClick={() => setOpen(!open())}
            >
              <Plus size={14} aria-hidden="true" />
            </button>
            <Show when={open()}>
              <div
                class="verevon-reaction-picker__panel"
                role="group"
                aria-label={i18n.tr('Velg emoji', 'Choose an emoji')}
              >
                <For each={SPACE_QUICK_REACTIONS}>
                  {(emoji) => (
                    <button
                      type="button"
                      class="verevon-reaction-picker__emoji"
                      aria-label={emoji}
                      onClick={() => pick(emoji)}
                    >
                      {emoji}
                    </button>
                  )}
                </For>
              </div>
            </Show>
          </span>
        </Show>
      </div>
    </Show>
  )
}
